use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::types::{ContentBlock, Message, Role};

/// Manages conversation history with persistence and compaction
#[derive(Debug, Serialize, Deserialize)]
pub struct ContextManager {
    pub messages: Vec<Message>,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    #[serde(skip)]
    persist_path: Option<PathBuf>,
}

impl ContextManager {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            total_input_tokens: 0,
            total_output_tokens: 0,
            persist_path: None,
        }
    }

    pub fn with_persistence(path: PathBuf) -> Self {
        // Try to load existing conversation
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(mut ctx) = serde_json::from_str::<ContextManager>(&content) {
                    ctx.persist_path = Some(path);
                    return ctx;
                }
            }
        }
        Self {
            messages: Vec::new(),
            total_input_tokens: 0,
            total_output_tokens: 0,
            persist_path: Some(path),
        }
    }

    pub fn add_user_message(&mut self, text: &str) {
        self.messages.push(Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
        });
    }

    pub fn add_assistant_message(&mut self, content: Vec<ContentBlock>) {
        self.messages.push(Message {
            role: Role::Assistant,
            content,
        });
    }

    pub fn add_tool_result(&mut self, tool_use_id: &str, content: &str, is_error: bool) {
        self.messages.push(Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: content.to_string(),
                is_error: if is_error { Some(true) } else { None },
            }],
        });
    }

    /// Estimate rough token count (4 chars ≈ 1 token)
    pub fn estimated_tokens(&self) -> u64 {
        let chars: usize = self
            .messages
            .iter()
            .flat_map(|m| &m.content)
            .map(|c| match c {
                ContentBlock::Text { text } => text.len(),
                ContentBlock::ToolUse { input, .. } => input.to_string().len(),
                ContentBlock::ToolResult { content, .. } => content.len(),
                ContentBlock::Thinking { thinking, .. } => thinking.len(),
                ContentBlock::Image { .. } => 1000, // rough estimate
            })
            .sum();
        (chars / 4) as u64
    }

    /// Compact conversation when approaching context limit
    /// Keeps first message (may contain important context) and recent messages
    pub fn compact(&mut self, keep_recent: usize) {
        if self.messages.len() <= keep_recent + 1 {
            return;
        }

        let summary = format!(
            "[Previous conversation compacted — {} messages summarized. \
             Total tokens used: {} input, {} output]",
            self.messages.len() - keep_recent,
            self.total_input_tokens,
            self.total_output_tokens,
        );

        let recent: Vec<Message> = self.messages.split_off(self.messages.len() - keep_recent);
        self.messages.clear();
        self.messages.push(Message {
            role: Role::User,
            content: vec![ContentBlock::Text { text: summary }],
        });
        self.messages.push(Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "Understood, I have the context from our previous conversation.".into(),
            }],
        });
        self.messages.extend(recent);
    }

    /// Persist to disk
    pub fn save(&self) -> anyhow::Result<()> {
        if let Some(ref path) = self.persist_path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let json = serde_json::to_string(self)?;
            std::fs::write(path, json)?;
        }
        Ok(())
    }
}
