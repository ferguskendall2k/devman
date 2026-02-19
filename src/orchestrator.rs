use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::mpsc;

use crate::agent::{AgentLoop, TurnResult};
use crate::client::AnthropicClient;
use crate::config::Config;
use crate::context::ContextManager;
use crate::cost::CostTracker;
use crate::tools;
use crate::types::{Thinking, ToolDefinition, Usage};

/// Sub-agent status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SubAgentStatus {
    Running,
    WaitingForUser,
    Complete,
    Failed(String),
    Killed,
}

/// A running or completed sub-agent
#[derive(Debug)]
pub struct SubAgentRecord {
    pub run_id: String,
    pub task_id: String,
    pub model: String,
    pub status: SubAgentStatus,
    pub started: chrono::DateTime<Utc>,
    pub output: Option<String>,
    pub usage: Usage,
}

/// Messages from sub-agent → orchestrator
#[derive(Debug)]
pub enum SubAgentMessage {
    /// Sub-agent completed with output
    Done {
        run_id: String,
        output: String,
        usage: Usage,
    },
    /// Sub-agent hit an error
    Error {
        run_id: String,
        error: String,
    },
    /// Progress update
    Progress {
        run_id: String,
        text: String,
    },
}

/// Sub-agent pool — spawn, track, and manage worker agents
pub struct Orchestrator {
    config: Config,
    api_key: String,
    brave_api_key: Option<String>,
    github_token: Option<String>,
    pub cost_tracker: CostTracker,
    pub agents: HashMap<String, SubAgentRecord>,
    state_dir: PathBuf,
    result_rx: mpsc::Receiver<SubAgentMessage>,
    result_tx: mpsc::Sender<SubAgentMessage>,
}

impl Orchestrator {
    pub fn new(config: Config, api_key: String, brave_api_key: Option<String>, github_token: Option<String>) -> Self {
        let state_dir = PathBuf::from(".devman/agents");
        let (result_tx, result_rx) = mpsc::channel(32);
        Self {
            config,
            api_key,
            brave_api_key,
            github_token,
            cost_tracker: CostTracker::new(),
            agents: HashMap::new(),
            state_dir,
            result_rx,
            result_tx,
        }
    }

    /// Spawn a sub-agent for a task
    pub async fn spawn(
        &mut self,
        task_id: &str,
        message: &str,
        model: &str,
        system_prompt: &str,
        thinking: Thinking,
    ) -> Result<String> {
        let run_id = format!(
            "run-{}-{}",
            Utc::now().format("%Y%m%d-%H%M%S"),
            &task_id[..task_id.len().min(20)]
        );

        // Create state directory
        let run_dir = self.state_dir.join(&run_id);
        std::fs::create_dir_all(&run_dir)?;

        let record = SubAgentRecord {
            run_id: run_id.clone(),
            task_id: task_id.to_string(),
            model: model.to_string(),
            status: SubAgentStatus::Running,
            started: Utc::now(),
            output: None,
            usage: Usage::default(),
        };
        self.agents.insert(run_id.clone(), record);

        // Spawn the agent loop in a background task
        let client = AnthropicClient::new(self.api_key.clone());
        let context = ContextManager::with_persistence(run_dir.join("conversation.json"));
        let tool_defs = tools::builtin_tool_definitions(self.config.tools.web_enabled, self.config.github.is_some());
        let brave_key = self.brave_api_key.clone();
        let gh_token = self.github_token.clone();
        let max_turns = self.config.agents.max_turns;
        let max_tokens = self.config.agents.max_tokens;
        let model_owned = model.to_string();
        let system_owned = system_prompt.to_string();
        let message_owned = message.to_string();
        let run_id_clone = run_id.clone();
        let tx = self.result_tx.clone();

        tokio::spawn(async move {
            let mut agent = AgentLoop::new(
                client,
                context,
                model_owned,
                system_owned,
                tool_defs,
                max_turns,
                max_tokens,
                thinking,
                brave_key,
                gh_token,
            );

            match agent.run_turn(&message_owned).await {
                Ok(result) => {
                    // Save output
                    let output_path = run_dir.join("output.md");
                    let _ = std::fs::write(&output_path, &result.text);

                    let _ = tx
                        .send(SubAgentMessage::Done {
                            run_id: run_id_clone,
                            output: result.text,
                            usage: result.usage,
                        })
                        .await;
                }
                Err(e) => {
                    let _ = tx
                        .send(SubAgentMessage::Error {
                            run_id: run_id_clone,
                            error: e.to_string(),
                        })
                        .await;
                }
            }
        });

        Ok(run_id)
    }

    /// Check for completed sub-agents (non-blocking)
    pub fn try_recv(&mut self) -> Option<SubAgentMessage> {
        match self.result_rx.try_recv() {
            Ok(msg) => {
                // Update agent record
                match &msg {
                    SubAgentMessage::Done {
                        run_id,
                        output,
                        usage,
                    } => {
                        if let Some(record) = self.agents.get_mut(run_id) {
                            record.status = SubAgentStatus::Complete;
                            record.output = Some(output.clone());
                            record.usage = usage.clone();
                            // Track cost
                            self.cost_tracker.record(
                                &record.model,
                                Some(&record.task_id),
                                usage.input_tokens,
                                usage.output_tokens,
                            );
                        }
                    }
                    SubAgentMessage::Error { run_id, error } => {
                        if let Some(record) = self.agents.get_mut(run_id) {
                            record.status = SubAgentStatus::Failed(error.clone());
                        }
                    }
                    _ => {}
                }
                Some(msg)
            }
            Err(_) => None,
        }
    }

    /// List active agents
    pub fn list_active(&self) -> Vec<&SubAgentRecord> {
        self.agents
            .values()
            .filter(|a| a.status == SubAgentStatus::Running)
            .collect()
    }

    /// Kill a sub-agent (marks as killed — the tokio task will finish its current API call)
    pub fn kill(&mut self, run_id: &str) -> Result<()> {
        if let Some(record) = self.agents.get_mut(run_id) {
            record.status = SubAgentStatus::Killed;
            Ok(())
        } else {
            anyhow::bail!("No agent with run_id: {run_id}")
        }
    }

    /// Assess task complexity for model selection
    pub fn assess_complexity(message: &str) -> TaskComplexity {
        let lower = message.to_lowercase();

        // Quick signals
        let quick_words = [
            "status", "list", "find", "show", "check", "what is", "where is", "how many",
        ];
        if quick_words.iter().any(|w| lower.contains(w)) && message.len() < 100 {
            return TaskComplexity::Quick;
        }

        // Complex signals
        let complex_words = [
            "redesign",
            "architect",
            "refactor",
            "review",
            "design",
            "plan",
            "strategy",
            "why is",
            "debug",
            "investigate",
        ];
        if complex_words.iter().any(|w| lower.contains(w)) {
            return TaskComplexity::Complex;
        }

        TaskComplexity::Standard
    }

    /// Get model name for a complexity tier
    pub fn model_for_complexity(&self, complexity: TaskComplexity) -> &str {
        match complexity {
            TaskComplexity::Quick => &self.config.models.quick,
            TaskComplexity::Standard => &self.config.models.standard,
            TaskComplexity::Complex => &self.config.models.complex,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TaskComplexity {
    Quick,
    Standard,
    Complex,
}
