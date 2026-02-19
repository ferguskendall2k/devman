use anyhow::Result;
use colored::Colorize;

use crate::client::{AnthropicClient, StreamEvent};
use crate::context::ContextManager;
use crate::tools;
use crate::types::{ContentBlock, Thinking, ToolDefinition, Usage};

/// The core agent loop — prompt → tool → result → repeat
pub struct AgentLoop {
    client: AnthropicClient,
    pub context: ContextManager,
    model: String,
    system_prompt: String,
    tools: Vec<ToolDefinition>,
    max_turns: u32,
    max_tokens: u32,
    thinking: Thinking,
    brave_api_key: Option<String>,
    github_token: Option<String>,
}

impl AgentLoop {
    pub fn new(
        client: AnthropicClient,
        context: ContextManager,
        model: String,
        system_prompt: String,
        tools: Vec<ToolDefinition>,
        max_turns: u32,
        max_tokens: u32,
        thinking: Thinking,
        brave_api_key: Option<String>,
        github_token: Option<String>,
    ) -> Self {
        Self {
            client,
            context,
            model,
            system_prompt,
            tools,
            max_turns,
            max_tokens,
            thinking,
            brave_api_key,
            github_token,
        }
    }

    /// Run a single user turn — may result in multiple API calls if tools are used
    pub async fn run_turn(&mut self, user_message: &str) -> Result<TurnResult> {
        self.context.add_user_message(user_message);

        let mut total_usage = Usage::default();
        let mut turns = 0;

        loop {
            turns += 1;
            if turns > self.max_turns {
                return Ok(TurnResult {
                    text: "[Turn limit reached]".into(),
                    usage: total_usage,
                });
            }

            // Check if we should compact
            if self.context.estimated_tokens() > 150_000 {
                eprintln!("{}", "⚡ Compacting conversation...".dimmed());
                self.context.compact(10);
            }

            let response = self
                .client
                .send_message(
                    &self.model,
                    &self.system_prompt,
                    &self.context.messages,
                    &self.tools,
                    self.max_tokens,
                    self.thinking,
                    &mut |event| {
                        // Stream text to terminal as it arrives
                        match event {
                            StreamEvent::ContentBlockDelta { delta, .. } => match delta {
                                crate::client::DeltaInfo::TextDelta { text } => {
                                    eprint!("{text}");
                                }
                                crate::client::DeltaInfo::ThinkingDelta { thinking } => {
                                    eprint!("{}", thinking.dimmed());
                                }
                                _ => {}
                            },
                            _ => {}
                        }
                    },
                )
                .await?;

            // Accumulate usage
            total_usage.input_tokens += response.usage.input_tokens;
            total_usage.output_tokens += response.usage.output_tokens;
            self.context.total_input_tokens += response.usage.input_tokens;
            self.context.total_output_tokens += response.usage.output_tokens;

            // Add assistant response to context
            self.context.add_assistant_message(response.content.clone());

            // Check for tool use
            let tool_calls: Vec<_> = response
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::ToolUse { id, name, input } => {
                        Some((id.clone(), name.clone(), input.clone()))
                    }
                    _ => None,
                })
                .collect();

            if tool_calls.is_empty() {
                // No tools — extract text and return
                let text = response
                    .content
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");

                eprintln!(); // newline after streamed text
                self.context.save()?;

                return Ok(TurnResult {
                    text,
                    usage: total_usage,
                });
            }

            // Execute tools
            for (id, name, input) in tool_calls {
                eprintln!("\n{} {}", "🔧".dimmed(), name.cyan());

                let result =
                    tools::execute_tool(&name, &input, self.brave_api_key.as_deref(), None, self.github_token.as_deref()).await;

                let (content, is_error) = match result {
                    Ok(output) => (output, false),
                    Err(e) => (format!("Error: {e}"), true),
                };

                // Truncate tool output for display
                let display = if content.len() > 200 {
                    format!("{}...", &content[..200])
                } else {
                    content.clone()
                };
                eprintln!("{}", display.dimmed());

                self.context.add_tool_result(&id, &content, is_error);
            }

            self.context.save()?;
            // Loop back to get the next response
        }
    }
}

#[derive(Debug)]
pub struct TurnResult {
    pub text: String,
    pub usage: Usage,
}
