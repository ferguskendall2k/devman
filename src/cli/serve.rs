use anyhow::{Context, Result};
use colored::Colorize;
use tokio::signal;

use crate::agent::AgentLoop;
use crate::auth::AuthStore;
use crate::client::AnthropicClient;
use crate::config::Config;
use crate::context::ContextManager;
use crate::telegram::api::TelegramBot;
use crate::tools;
use crate::types::Thinking;

pub async fn run(config: &Config) -> Result<()> {
    let auth = AuthStore::load().context("loading credentials")?;

    // Anthropic client
    let api_key = auth.anthropic_api_key()?;
    let _client = AnthropicClient::new(api_key);

    // Telegram bot
    let bot_token = auth
        .telegram_bot_token()
        .context("Telegram bot token not configured. Set TELEGRAM_BOT_TOKEN or add to credentials.toml")?;

    let allowed_users = config
        .telegram
        .as_ref()
        .map(|t| t.allowed_users.clone())
        .unwrap_or_default();

    let bot = TelegramBot::new(bot_token, allowed_users);

    // Agent setup
    let model = config.models.standard.clone();
    let system_prompt = "You are DevMan, a helpful coding assistant. Be concise.".to_string();
    let tool_defs = tools::builtin_tool_definitions(config.tools.web_enabled);
    let brave_api_key = auth.brave_api_key();

    println!(
        "{} {} {}",
        "🤖".bold(),
        "DevMan Telegram bot started".green().bold(),
        format!("(model: {})", model).dimmed()
    );
    println!("{}", "Press Ctrl+C to stop".dimmed());

    let mut offset: i64 = 0;

    loop {
        tokio::select! {
            _ = signal::ctrl_c() => {
                println!("\n{}", "Shutting down...".yellow());
                break;
            }
            result = bot.get_updates(offset, 30) => {
                let updates = match result {
                    Ok(u) => u,
                    Err(e) => {
                        tracing::error!("Failed to get updates: {e}");
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        continue;
                    }
                };

                for update in updates {
                    offset = update.update_id + 1;

                    let message = match update.message {
                        Some(m) => m,
                        None => continue,
                    };

                    let user = match message.from {
                        Some(ref u) => u,
                        None => continue,
                    };

                    if !bot.is_allowed(user.id) {
                        tracing::warn!("Ignoring message from unauthorized user: {} ({})", user.first_name, user.id);
                        continue;
                    }

                    let text: String = match message.text {
                        Some(ref t) => t.clone(),
                        None => continue,
                    };

                    let chat_id = message.chat.id;
                    let user_name = user.username.clone().unwrap_or_else(|| user.first_name.clone());

                    println!(
                        "{} {} {}",
                        "📩".dimmed(),
                        user_name.cyan(),
                        text.dimmed()
                    );

                    // Send typing indicator
                    let _ = bot.send_typing(chat_id).await;

                    // Create a fresh agent loop per message (stateless for now)
                    let context = ContextManager::new();
                    let mut agent = AgentLoop::new(
                        AnthropicClient::new(auth.anthropic_api_key()?),
                        context,
                        model.clone(),
                        system_prompt.clone(),
                        tool_defs.clone(),
                        config.agents.max_turns,
                        config.agents.max_tokens,
                        Thinking::Off,
                        brave_api_key.clone(),
                    );

                    match agent.run_turn(&text).await {
                        Ok(result) => {
                            let reply = if result.text.is_empty() {
                                "[No response]".to_string()
                            } else {
                                result.text
                            };

                            if let Err(e) = bot.send_message(chat_id, &reply).await {
                                tracing::error!("Failed to send reply: {e}");
                            }
                        }
                        Err(e) => {
                            tracing::error!("Agent error: {e}");
                            let _ = bot
                                .send_message(chat_id, &format!("❌ Error: {e}"))
                                .await;
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
