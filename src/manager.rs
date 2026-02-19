use anyhow::Result;
use colored::Colorize;

use crate::agent::AgentLoop;
use crate::client::AnthropicClient;
use crate::config::Config;
use crate::context::ContextManager;
use crate::cost::CostTracker;
use crate::orchestrator::{Orchestrator, SubAgentMessage, TaskComplexity};
use crate::tools;
use crate::types::{Thinking, ToolDefinition};

/// Manager-only tool definitions (spawn, steer, kill, list agents)
fn manager_tool_definitions() -> Vec<ToolDefinition> {
    use serde_json::json;
    vec![
        ToolDefinition {
            name: "assign_bot".into(),
            description: "Assign a Telegram bot to a task. Creates a scoped bot entry in config.toml. The bot token must be provided (create via @BotFather first). After adding, DevMan needs a restart to pick up the new bot.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Internal name for this bot (e.g. 'marketing', 'dev', 'research')"
                    },
                    "bot_token": {
                        "type": "string",
                        "description": "Telegram bot token from @BotFather"
                    },
                    "tasks": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Task slugs this bot can access (e.g. ['hyperpilot-marketing']). Use ['*'] for all tasks."
                    },
                    "allowed_users": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "description": "Telegram user IDs allowed to use this bot. Empty = same as manager."
                    },
                    "default_model": {
                        "type": "string",
                        "enum": ["quick", "standard", "complex"],
                        "description": "Model tier for this bot. Default: standard."
                    },
                    "memory_access": {
                        "type": "string",
                        "enum": ["scoped", "full"],
                        "description": "Memory access: 'scoped' = only listed tasks, 'full' = all tasks. Default: scoped."
                    },
                    "system_prompt": {
                        "type": "string",
                        "description": "Optional custom system prompt for this bot"
                    }
                },
                "required": ["name", "bot_token", "tasks"]
            }),
        },
        ToolDefinition {
            name: "list_bots".into(),
            description: "List all configured scoped bots with their task assignments.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: "remove_bot".into(),
            description: "Remove a scoped bot by name from config.toml. Requires restart to take effect.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the bot to remove"
                    }
                },
                "required": ["name"]
            }),
        },
        ToolDefinition {
            name: "spawn_agent".into(),
            description: "Spawn a sub-agent to work on a task. Choose the right model tier based on complexity.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "Task identifier"
                    },
                    "message": {
                        "type": "string",
                        "description": "Instructions for the sub-agent"
                    },
                    "model_tier": {
                        "type": "string",
                        "enum": ["quick", "standard", "complex"],
                        "description": "Model tier: quick (Haiku), standard (Sonnet), complex (Opus). Default: auto-detect."
                    }
                },
                "required": ["task_id", "message"]
            }),
        },
        ToolDefinition {
            name: "list_agents".into(),
            description: "List active and recent sub-agents with their status and cost.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: "kill_agent".into(),
            description: "Stop a running sub-agent.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "run_id": {
                        "type": "string",
                        "description": "Run ID of the agent to kill"
                    }
                },
                "required": ["run_id"]
            }),
        },
    ]
}

/// The Manager — triage, routing, sub-agent orchestration
pub struct Manager {
    config: Config,
    orchestrator: Orchestrator,
    agent: AgentLoop,
}

impl Manager {
    pub fn new(config: Config, api_key: String, brave_api_key: Option<String>, github_token: Option<String>) -> Self {
        let client = AnthropicClient::new(api_key.clone());

        let state_dir = dirs::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("devman");
        let _ = std::fs::create_dir_all(&state_dir);
        let context = ContextManager::with_persistence(state_dir.join("manager-conversation.json"));

        // Combine built-in tools + manager-only tools
        let mut tool_defs = tools::builtin_tool_definitions(config.tools.web_enabled, config.github.is_some());
        tool_defs.extend(manager_tool_definitions());

        let system_prompt = MANAGER_SYSTEM_PROMPT.to_string();

        // Manager gets global storage (can see all task storage)
        let mm = crate::memory::MemoryManager::new(crate::memory::MemoryManager::default_root());
        let global_storage = mm.global_storage();

        let agent = AgentLoop::new(
            client,
            context,
            config.models.manager.clone(),
            system_prompt,
            tool_defs,
            config.agents.max_turns,
            config.agents.max_tokens,
            Thinking::Off,
            brave_api_key.clone(),
            github_token.clone(),
        ).with_storage(global_storage);

        let orchestrator = Orchestrator::new(config.clone(), api_key, brave_api_key, github_token);

        Self {
            config,
            orchestrator,
            agent,
        }
    }

    /// Process a user message through the manager
    pub async fn handle_message(&mut self, message: &str) -> Result<String> {
        // Check for completed sub-agents first
        while let Some(msg) = self.orchestrator.try_recv() {
            match msg {
                SubAgentMessage::Done {
                    run_id, output, ..
                } => {
                    eprintln!("{}", format!("✅ Sub-agent {run_id} completed").green());
                    // Inject result into manager context
                    let summary = if output.len() > 2000 {
                        format!("{}...\n(truncated)", &output[..2000])
                    } else {
                        output
                    };
                    self.agent
                        .context
                        .add_user_message(&format!("[Sub-agent {run_id} result]\n{summary}"));
                }
                SubAgentMessage::Error { run_id, error } => {
                    eprintln!("{}", format!("❌ Sub-agent {run_id} failed: {error}").red());
                    self.agent
                        .context
                        .add_user_message(&format!("[Sub-agent {run_id} error: {error}]"));
                }
                _ => {}
            }
        }

        // Run through the manager agent
        let result = self.agent.run_turn(message).await?;

        // Track manager's own cost
        self.orchestrator.cost_tracker.record(
            &self.config.models.manager,
            None,
            result.usage.input_tokens,
            result.usage.output_tokens,
            0,
            0,
        );

        Ok(result.text)
    }

    /// Get cost summary
    pub fn cost_summary(&self) -> String {
        self.orchestrator.cost_tracker.summary()
    }
}

const MANAGER_SYSTEM_PROMPT: &str = r#"You are DevMan, an AI assistant running as a Telegram bot. You ARE the system — you have tools to manage yourself.

RULES:
1. For quick questions — answer directly using your tools.
2. For substantial work — spawn a sub-agent.
3. Pick the right model tier:
   - quick (Haiku): simple lookups, file searches, status checks
   - standard (Sonnet): code changes, debugging, writing
   - complex (Opus): architecture decisions, complex refactors
4. When a sub-agent finishes, relay its output.
5. Keep responses lightweight — you're a router, not a worker.
6. ALWAYS use your tools to take action. Never give CLI commands or setup instructions — YOU do it.

BOT MANAGEMENT:
When the user asks you to assign/add/create a bot for a task:
1. You need a Telegram bot token. If the user hasn't provided one, ask them to create one via @BotFather and send you the token.
2. Once you have the token, use the assign_bot tool immediately. Don't explain how to do it — just do it.
3. Tell the user the bot is configured and needs a restart to activate.

TOOLS:
- Standard: shell, read/write/edit files, web search/fetch, storage
- Agents: spawn_agent, list_agents, kill_agent
- Bot management: assign_bot (add a scoped Telegram bot), list_bots, remove_bot
- Memory: memory_search, memory_read, memory_write, memory_load_task, memory_create_task
- Storage: storage_write, storage_read, storage_list, storage_delete

Be concise. Use tools proactively. You ARE DevMan — act, don't instruct."#;
