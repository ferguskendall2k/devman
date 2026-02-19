use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::RwLock;

use crate::agent::AgentLoop;
use crate::auth::AuthStore;
use crate::client::AnthropicClient;
use crate::config::Config;
use crate::context::ContextManager;
use crate::cost::CostTracker;
use crate::cron::CronScheduler;
use crate::telegram::api::TelegramBot;
use crate::tools;
use crate::types::Thinking;

/// Per-chat conversation state
struct ChatState {
    context: ContextManager,
}

pub async fn run(config: &Config) -> Result<()> {
    let auth = AuthStore::load().context("loading credentials")?;
    let api_key = auth.anthropic_api_key()?;
    let brave_api_key = auth.brave_api_key();
    let github_token = auth.github_token();

    // Telegram bot
    let bot_token = auth.telegram_bot_token().context(
        "Telegram bot token not configured. Set TELEGRAM_BOT_TOKEN or add to credentials.toml",
    )?;
    let allowed_users = config
        .telegram
        .as_ref()
        .map(|t| t.allowed_users.clone())
        .unwrap_or_default();
    let bot = TelegramBot::new(bot_token, allowed_users);

    // Shared cost tracker
    let cost_tracker = Arc::new(RwLock::new(CostTracker::new()));

    // Cron scheduler
    let state_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("devman");
    std::fs::create_dir_all(&state_dir)?;
    let mut cron = CronScheduler::new(state_dir.join("cron-jobs.json"));

    // Per-chat conversation persistence
    let chats_dir = state_dir.join("chats");
    std::fs::create_dir_all(&chats_dir)?;
    let mut chat_states: HashMap<i64, ChatState> = HashMap::new();

    // Dashboard (spawn in background if enabled)
    if config.dashboard.enabled {
        let dash_config = config.clone();
        let dash_cost = cost_tracker.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::dashboard::start(dash_config, dash_cost).await {
                tracing::error!("Dashboard error: {e}");
            }
        });
        eprintln!(
            "{} Dashboard at {}",
            "🌐".dimmed(),
            format!("http://{}:{}", config.dashboard.bind, config.dashboard.port)
                .cyan()
                .bold()
        );
    }

    let model = config.models.standard.clone();
    let system_prompt =
        "You are DevMan, a helpful coding assistant. Be concise and use tools proactively."
            .to_string();
    let tool_defs = tools::builtin_tool_definitions(config.tools.web_enabled, config.github.is_some());

    eprintln!(
        "{} {} {}",
        "🤖".bold(),
        "DevMan serving".green().bold(),
        format!("(model: {})", model).dimmed()
    );
    eprintln!("{}", "Press Ctrl+C to stop".dimmed());

    let mut offset: i64 = 0;
    let mut cron_tick = tokio::time::interval(std::time::Duration::from_secs(30));

    loop {
        tokio::select! {
            _ = signal::ctrl_c() => {
                eprintln!("\n{}", "Shutting down...".yellow());
                cron.save()?;
                break;
            }

            // Cron tick
            _ = cron_tick.tick() => {
                let due_jobs = cron.tick();
                for job in due_jobs {
                    eprintln!("{} Cron fired: {}", "⏰".dimmed(), job.name);
                    match &job.action {
                        crate::cron::CronAction::SystemEvent { text } => {
                            eprintln!("  {}", text.dimmed());
                            // TODO: inject into relevant chat or log
                        }
                        crate::cron::CronAction::AgentTask { message, model: task_model } => {
                            // Run as a standalone agent turn
                            let m = task_model.as_deref().unwrap_or(&model);
                            let client = AnthropicClient::new(api_key.clone());
                            let context = ContextManager::new();
                            let mut agent = AgentLoop::new(
                                client,
                                context,
                                m.to_string(),
                                system_prompt.clone(),
                                tool_defs.clone(),
                                config.agents.max_turns,
                                config.agents.max_tokens,
                                Thinking::Off,
                                brave_api_key.clone(),
                github_token.clone(),
                            );
                            match agent.run_turn(message).await {
                                Ok(result) => {
                                    eprintln!("  Cron result: {}", &result.text[..result.text.len().min(200)]);
                                    let mut ct = cost_tracker.write().await;
                                    ct.record(m, Some(&job.name), result.usage.input_tokens, result.usage.output_tokens);
                                }
                                Err(e) => eprintln!("  Cron error: {e}"),
                            }
                        }
                    }
                }
                cron.save()?;
            }

            // Telegram polling
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
                        continue;
                    }

                    let text = match message.text {
                        Some(ref t) => t.clone(),
                        None => continue,
                    };

                    let chat_id = message.chat.id;
                    let user_name = user.username.clone().unwrap_or_else(|| user.first_name.clone());

                    eprintln!("{} {} {}", "📩".dimmed(), user_name.cyan(), text.dimmed());

                    let _ = bot.send_typing(chat_id).await;

                    // Get or create per-chat context
                    let chat = chat_states.entry(chat_id).or_insert_with(|| {
                        ChatState {
                            context: ContextManager::with_persistence(
                                chats_dir.join(format!("{chat_id}.json")),
                            ),
                        }
                    });

                    // Build agent with this chat's context
                    let context = std::mem::replace(
                        &mut chat.context,
                        ContextManager::new(),
                    );

                    let mut agent = AgentLoop::new(
                        AnthropicClient::new(api_key.clone()),
                        context,
                        model.clone(),
                        system_prompt.clone(),
                        tool_defs.clone(),
                        config.agents.max_turns,
                        config.agents.max_tokens,
                        Thinking::Off,
                        brave_api_key.clone(),
                github_token.clone(),
                    );

                    match agent.run_turn(&text).await {
                        Ok(result) => {
                            let reply = if result.text.is_empty() {
                                "[No response]".to_string()
                            } else {
                                result.text
                            };

                            // Truncate for Telegram's 4096 char limit
                            let reply = if reply.len() > 4000 {
                                format!("{}...\n\n_(truncated)_", &reply[..4000])
                            } else {
                                reply
                            };

                            if let Err(e) = bot.send_message(chat_id, &reply).await {
                                tracing::error!("Failed to send reply: {e}");
                            }

                            // Track cost
                            let mut ct = cost_tracker.write().await;
                            ct.record(&model, None, result.usage.input_tokens, result.usage.output_tokens);
                        }
                        Err(e) => {
                            tracing::error!("Agent error: {e}");
                            let _ = bot.send_message(chat_id, &format!("❌ Error: {e}")).await;
                        }
                    }

                    // Put context back
                    chat.context = agent.context;
                }
            }
        }
    }

    Ok(())
}
