use anyhow::{Context, Result};
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::types::{ContentBlock, Message, Thinking, ToolDefinition, Usage};

const API_BASE: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

/// Anthropic API client with SSE streaming
pub struct AnthropicClient {
    client: Client,
    api_key: String,
    /// Track if we've already retried with a fresh token this session
    retried_auth: bool,
}

/// Request body for the Messages API
#[derive(Debug, Serialize)]
struct CreateMessageRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<ApiMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<ToolDefinition>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingConfig>,
}

#[derive(Debug, Serialize)]
struct ThinkingConfig {
    #[serde(rename = "type")]
    thinking_type: String,
    budget_tokens: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct ApiMessage {
    role: String,
    content: serde_json::Value,
}

/// Parsed SSE event from the streaming response
#[derive(Debug)]
pub enum StreamEvent {
    ContentBlockStart {
        index: usize,
        block: ContentBlockInfo,
    },
    ContentBlockDelta {
        index: usize,
        delta: DeltaInfo,
    },
    ContentBlockStop {
        index: usize,
    },
    MessageStart {
        usage: Option<Usage>,
    },
    MessageDelta {
        usage: Option<Usage>,
        stop_reason: Option<String>,
    },
    MessageStop,
    Ping,
    Error {
        message: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlockInfo {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
    #[serde(rename = "tool_use")]
    ToolUse { id: String, name: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum DeltaInfo {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "thinking_delta")]
    ThinkingDelta { thinking: String },
    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
    #[serde(rename = "signature_delta")]
    SignatureDelta { signature: String },
}

/// Accumulated result from a streaming response
#[derive(Debug)]
pub struct StreamedResponse {
    pub content: Vec<ContentBlock>,
    pub usage: Usage,
    pub stop_reason: Option<String>,
}

impl AnthropicClient {
    pub fn new(api_key: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .expect("failed to build HTTP client");
        Self { client, api_key, retried_auth: false }
    }

    /// Try to refresh the API key from credentials
    pub fn refresh_api_key(&mut self) -> bool {
        match crate::auth::AuthStore::load().and_then(|a| a.anthropic_api_key()) {
            Ok(new_key) if new_key != self.api_key => {
                tracing::info!("Refreshed API key from credentials");
                self.api_key = new_key;
                true
            }
            _ => false,
        }
    }

    /// Send a streaming request and collect the full response
    pub async fn send_message(
        &mut self,
        model: &str,
        system: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
        max_tokens: u32,
        thinking: Thinking,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<StreamedResponse> {
        let api_messages = Self::convert_messages(messages);

        let mut request = CreateMessageRequest {
            model: model.to_string(),
            max_tokens,
            system: system.to_string(),
            messages: api_messages,
            tools: tools.to_vec(),
            stream: true,
            thinking: None,
        };

        // Set thinking config
        match thinking {
            Thinking::Off => {}
            Thinking::Low => {
                request.thinking = Some(ThinkingConfig {
                    thinking_type: "enabled".into(),
                    budget_tokens: 2048,
                });
                // max_tokens must exceed budget_tokens by at least 4096 for output
                request.max_tokens = request.max_tokens.max(8192);
            }
            Thinking::Medium => {
                request.thinking = Some(ThinkingConfig {
                    thinking_type: "enabled".into(),
                    budget_tokens: 8192,
                });
                request.max_tokens = request.max_tokens.max(16384);
            }
            Thinking::High => {
                request.thinking = Some(ThinkingConfig {
                    thinking_type: "enabled".into(),
                    budget_tokens: 32768,
                });
                request.max_tokens = request.max_tokens.max(65536);
            }
        }

        // Determine auth header based on key prefix
        let is_oauth = self.api_key.starts_with("sk-ant-oat");

        let mut req_builder = self
            .client
            .post(API_BASE)
            .header("content-type", "application/json")
            .header("anthropic-version", API_VERSION);

        if is_oauth {
            // OAuth tokens require Bearer auth + Claude Code identity headers
            req_builder = req_builder
                .header("authorization", format!("Bearer {}", self.api_key))
                .header("anthropic-beta", "claude-code-20250219,oauth-2025-04-20")
                .header("user-agent", "claude-cli/1.0.0 (external, cli)")
                .header("x-app", "cli");
        } else {
            req_builder = req_builder.header("x-api-key", &self.api_key);
        }

        let mut response = req_builder
            .json(&request)
            .send()
            .await
            .context("sending request to Anthropic API")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();

            // Auto-refresh OAuth token on 401
            if status == reqwest::StatusCode::UNAUTHORIZED && !self.retried_auth {
                tracing::warn!("Got 401 — attempting token refresh");
                self.retried_auth = true;
                if self.refresh_api_key() {
                    // Retry: rebuild the request with new key
                    let is_oauth = self.api_key.starts_with("sk-ant-oat");
                    let mut retry = self
                        .client
                        .post(API_BASE)
                        .header("content-type", "application/json")
                        .header("anthropic-version", API_VERSION);

                    if is_oauth {
                        retry = retry
                            .header("authorization", format!("Bearer {}", self.api_key))
                            .header("anthropic-beta", "claude-code-20250219,oauth-2025-04-20")
                            .header("user-agent", "claude-cli/1.0.0 (external, cli)")
                            .header("x-app", "cli");
                    } else {
                        retry = retry.header("x-api-key", &self.api_key);
                    }

                    let retry_resp = retry.json(&request).send().await
                        .context("retry after token refresh")?;

                    if retry_resp.status().is_success() {
                        response = retry_resp;
                        self.retried_auth = false;
                        // Fall through to stream parsing below
                    } else {
                        let s = retry_resp.status();
                        let b = retry_resp.text().await.unwrap_or_default();
                        anyhow::bail!("API error {s} (after token refresh): {b}");
                    }
                } else {
                    anyhow::bail!("API error 401 Unauthorized (token refresh failed): {body}");
                }
            } else {
                anyhow::bail!("API error {status}: {body}");
            }
        } else {
            self.retried_auth = false;
        }

        // Parse SSE stream
        let mut result = StreamedResponse {
            content: Vec::new(),
            usage: Usage::default(),
            stop_reason: None,
        };

        // Track in-progress blocks
        let mut current_blocks: Vec<InProgressBlock> = Vec::new();

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = tokio::time::timeout(
            Duration::from_secs(60),
            stream.next(),
        )
        .await
        .context("stream chunk timeout (60s)")?
        {
            let chunk = chunk.context("reading stream chunk")?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            // Parse SSE events from buffer
            while let Some(event) = Self::parse_next_sse_event(&mut buffer) {
                match &event {
                    StreamEvent::MessageStart { usage } => {
                        if let Some(u) = usage {
                            result.usage = u.clone();
                        }
                    }
                    StreamEvent::ContentBlockStart { index, block } => {
                        while current_blocks.len() <= *index {
                            current_blocks.push(InProgressBlock::default());
                        }
                        match block {
                            ContentBlockInfo::Text { .. } => {
                                current_blocks[*index].block_type = BlockType::Text;
                            }
                            ContentBlockInfo::Thinking { .. } => {
                                current_blocks[*index].block_type = BlockType::Thinking;
                            }
                            ContentBlockInfo::ToolUse { id, name } => {
                                current_blocks[*index].block_type = BlockType::ToolUse;
                                current_blocks[*index].tool_id = Some(id.clone());
                                current_blocks[*index].tool_name = Some(name.clone());
                            }
                        }
                    }
                    StreamEvent::ContentBlockDelta { index, delta } => {
                        if let Some(block) = current_blocks.get_mut(*index) {
                            match delta {
                                DeltaInfo::TextDelta { text } => {
                                    block.text.push_str(text);
                                }
                                DeltaInfo::ThinkingDelta { thinking } => {
                                    block.text.push_str(thinking);
                                }
                                DeltaInfo::InputJsonDelta { partial_json } => {
                                    block.text.push_str(partial_json);
                                }
                                DeltaInfo::SignatureDelta { signature } => {
                                    block.signature = Some(signature.clone());
                                }
                            }
                        }
                    }
                    StreamEvent::ContentBlockStop { index } => {
                        if let Some(block) = current_blocks.get(*index) {
                            let content_block = match block.block_type {
                                BlockType::Text => ContentBlock::Text {
                                    text: block.text.clone(),
                                },
                                BlockType::Thinking => ContentBlock::Thinking {
                                    thinking: block.text.clone(),
                                    signature: block.signature.clone().unwrap_or_default(),
                                },
                                BlockType::ToolUse => ContentBlock::ToolUse {
                                    id: block.tool_id.clone().unwrap_or_default(),
                                    name: block.tool_name.clone().unwrap_or_default(),
                                    input: serde_json::from_str(&block.text)
                                        .unwrap_or(serde_json::Value::Object(Default::default())),
                                },
                            };
                            result.content.push(content_block);
                        }
                    }
                    StreamEvent::MessageDelta {
                        usage,
                        stop_reason,
                    } => {
                        if let Some(u) = usage {
                            result.usage.output_tokens = u.output_tokens;
                        }
                        result.stop_reason = stop_reason.clone();
                    }
                    _ => {}
                }
                on_event(event);
            }
        }

        Ok(result)
    }

    fn convert_messages(messages: &[Message]) -> Vec<ApiMessage> {
        messages
            .iter()
            .map(|m| {
                let content: Vec<serde_json::Value> = m
                    .content
                    .iter()
                    .map(|c| serde_json::to_value(c).unwrap())
                    .collect();
                ApiMessage {
                    role: match m.role {
                        crate::types::Role::User => "user".into(),
                        crate::types::Role::Assistant => "assistant".into(),
                    },
                    content: serde_json::Value::Array(content),
                }
            })
            .collect()
    }

    /// Parse next complete SSE event from buffer, consuming it
    fn parse_next_sse_event(buffer: &mut String) -> Option<StreamEvent> {
        // Normalize \r\n to \n for cross-platform SSE
        if buffer.contains("\r\n") {
            *buffer = buffer.replace("\r\n", "\n");
        }
        // SSE events are separated by double newlines
        let event_end = buffer.find("\n\n")?;
        let event_text = buffer[..event_end].to_string();
        *buffer = buffer[event_end + 2..].to_string();

        let mut event_type = String::new();
        let mut data = String::new();

        for line in event_text.lines() {
            if let Some(rest) = line.strip_prefix("event: ") {
                event_type = rest.to_string();
            } else if let Some(rest) = line.strip_prefix("data: ") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(rest);
            }
        }

        match event_type.as_str() {
            "ping" => Some(StreamEvent::Ping),
            "message_start" => {
                let v: serde_json::Value = serde_json::from_str(&data).ok()?;
                let usage = v
                    .get("message")
                    .and_then(|m| m.get("usage"))
                    .and_then(|u| serde_json::from_value(u.clone()).ok());
                Some(StreamEvent::MessageStart { usage })
            }
            "content_block_start" => {
                let v: serde_json::Value = serde_json::from_str(&data).ok()?;
                let index = v.get("index")?.as_u64()? as usize;
                let block: ContentBlockInfo =
                    serde_json::from_value(v.get("content_block")?.clone()).ok()?;
                Some(StreamEvent::ContentBlockStart { index, block })
            }
            "content_block_delta" => {
                let v: serde_json::Value = serde_json::from_str(&data).ok()?;
                let index = v.get("index")?.as_u64()? as usize;
                let delta: DeltaInfo = serde_json::from_value(v.get("delta")?.clone()).ok()?;
                Some(StreamEvent::ContentBlockDelta { index, delta })
            }
            "content_block_stop" => {
                let v: serde_json::Value = serde_json::from_str(&data).ok()?;
                let index = v.get("index")?.as_u64()? as usize;
                Some(StreamEvent::ContentBlockStop { index })
            }
            "message_delta" => {
                let v: serde_json::Value = serde_json::from_str(&data).ok()?;
                let delta = v.get("delta")?;
                let stop_reason = delta
                    .get("stop_reason")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string());
                let usage = v
                    .get("usage")
                    .and_then(|u| serde_json::from_value(u.clone()).ok());
                Some(StreamEvent::MessageDelta { usage, stop_reason })
            }
            "message_stop" => Some(StreamEvent::MessageStop),
            "error" => {
                let v: serde_json::Value = serde_json::from_str(&data).ok()?;
                let message = v
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown error")
                    .to_string();
                Some(StreamEvent::Error { message })
            }
            other => {
                tracing::debug!("Unknown SSE event type: {}", other);
                None
            }
        }
    }
}

#[derive(Debug, Default)]
struct InProgressBlock {
    block_type: BlockType,
    text: String,
    tool_id: Option<String>,
    tool_name: Option<String>,
    signature: Option<String>,
}

#[derive(Debug, Default)]
enum BlockType {
    #[default]
    Text,
    Thinking,
    ToolUse,
}
