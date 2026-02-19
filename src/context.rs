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

    /// Compact conversation when approaching context limit.
    /// Extracts text from recent messages as a summary, then starts fresh.
    /// This avoids orphaned tool_use/tool_result blocks that break the API.
    pub fn compact(&mut self, keep_recent: usize) {
        if self.messages.len() <= keep_recent + 1 {
            return;
        }

        // Extract text content from recent messages for context
        let recent_start = self.messages.len().saturating_sub(keep_recent);
        let mut recent_text = String::new();
        for msg in &self.messages[recent_start..] {
            let role = match msg.role {
                Role::User => "User",
                Role::Assistant => "Assistant",
            };
            for block in &msg.content {
                match block {
                    ContentBlock::Text { text } => {
                        if !text.is_empty() {
                            recent_text.push_str(&format!("{role}: {text}\n"));
                        }
                    }
                    ContentBlock::ToolUse { name, .. } => {
                        recent_text.push_str(&format!("{role}: [used tool: {name}]\n"));
                    }
                    ContentBlock::ToolResult { content, .. } => {
                        let preview = if content.len() > 200 {
                            format!("{}...", &content[..200])
                        } else {
                            content.clone()
                        };
                        recent_text.push_str(&format!("{role}: [tool result: {preview}]\n"));
                    }
                    _ => {}
                }
            }
        }

        let summary = format!(
            "[Conversation compacted — {} messages removed. {} input tokens, {} output tokens used so far.]\n\
             Recent context:\n{}",
            self.messages.len(),
            self.total_input_tokens,
            self.total_output_tokens,
            recent_text.trim(),
        );

        // Start completely fresh — no orphaned tool blocks
        self.messages.clear();
        self.messages.push(Message {
            role: Role::User,
            content: vec![ContentBlock::Text { text: summary }],
        });
        self.messages.push(Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "Understood, I have the context from our conversation so far. How can I help?".into(),
            }],
        });
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
