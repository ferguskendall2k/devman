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

const MANAGER_SYSTEM_PROMPT: &str = r#"You are DevMan's manager agent. Your job is to triage, route, and orchestrate.

RULES:
1. For quick questions (status, lookups, simple answers) — answer directly using your tools.
2. For substantial work (code changes, research, multi-step tasks) — spawn a sub-agent.
3. Pick the right model tier:
   - quick (Haiku): simple lookups, file searches, status checks
   - standard (Sonnet): code changes, debugging, writing, standard dev work
   - complex (Opus): architecture decisions, complex refactors, novel problem solving
4. When a sub-agent finishes, relay its output to the user.
5. Keep your own responses lightweight — you're a router, not a worker.
6. Track and report costs when asked (/cost).

TOOLS:
- All standard tools (shell, read/write/edit files, web search/fetch)
- spawn_agent: Start a sub-agent for a task
- list_agents: Show active sub-agents  
- kill_agent: Stop a running sub-agent

Be concise and helpful. Use tools proactively."#;
