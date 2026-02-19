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
                                    ct.record(m, Some(&job.name), result.usage.input_tokens, result.usage.output_tokens, 0, 0);
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

                    let chat_id = message.chat.id;
                    let user_name = user.username.clone().unwrap_or_else(|| user.first_name.clone());

                    // Download directory for this chat's files
                    let download_dir = chats_dir.join(format!("{chat_id}_files"));

                    // Build message text from text/caption + any attachments
                    let mut text_parts: Vec<String> = Vec::new();

                    // Text or caption
                    if let Some(ref t) = message.text {
                        text_parts.push(t.clone());
                    } else if let Some(ref c) = message.caption {
                        text_parts.push(c.clone());
                    }

                    // Photo — download largest size
                    if let Some(ref photos) = message.photo {
                        if let Some(largest) = photos.last() {
                            match bot.download_by_id(&largest.file_id, &download_dir, "photo").await {
                                Ok(path) => {
                                    text_parts.push(format!("[Image downloaded: {}  ({}×{})]", path.display(), largest.width, largest.height));
                                }
                                Err(e) => {
                                    text_parts.push(format!("[Failed to download image: {e}]"));
                                }
                            }
                        }
                    }

                    // Document
                    if let Some(ref doc) = message.document {
                        let name = doc.file_name.as_deref().unwrap_or("document");
                        match bot.download_by_id(&doc.file_id, &download_dir, name).await {
                            Ok(path) => {
                                let mime = doc.mime_type.as_deref().unwrap_or("unknown");
                                let size = doc.file_size.unwrap_or(0);
                                text_parts.push(format!("[File downloaded: {} ({mime}, {size} bytes)]", path.display()));
                            }
                            Err(e) => {
                                text_parts.push(format!("[Failed to download file: {e}]"));
                            }
                        }
                    }

                    // Voice message
                    if let Some(ref voice) = message.voice {
                        match bot.download_by_id(&voice.file_id, &download_dir, "voice").await {
                            Ok(path) => {
                                text_parts.push(format!("[Voice message downloaded: {} ({}s)]", path.display(), voice.duration));
                            }
                            Err(e) => {
                                text_parts.push(format!("[Failed to download voice: {e}]"));
                            }
                        }
                    }

                    // Audio
                    if let Some(ref audio) = message.audio {
                        let name = audio.file_name.as_deref().unwrap_or("audio");
                        match bot.download_by_id(&audio.file_id, &download_dir, name).await {
                            Ok(path) => {
                                text_parts.push(format!("[Audio downloaded: {} ({}s)]", path.display(), audio.duration));
                            }
                            Err(e) => {
                                text_parts.push(format!("[Failed to download audio: {e}]"));
                            }
                        }
                    }

                    // Video
                    if let Some(ref video) = message.video {
                        let name = video.file_name.as_deref().unwrap_or("video");
                        match bot.download_by_id(&video.file_id, &download_dir, name).await {
                            Ok(path) => {
                                text_parts.push(format!("[Video downloaded: {} ({}s, {}×{})]", path.display(), video.duration, video.width, video.height));
                            }
                            Err(e) => {
                                text_parts.push(format!("[Failed to download video: {e}]"));
                            }
                        }
                    }

                    // Sticker
                    if let Some(ref sticker) = message.sticker {
                        let emoji = sticker.emoji.as_deref().unwrap_or("");
                        text_parts.push(format!("[Sticker: {emoji}]"));
                    }

                    // Skip if nothing useful
                    let text = text_parts.join("\n");
                    if text.is_empty() {
                        continue;
                    }

                    eprintln!("{} {} {}", "📩".dimmed(), user_name.cyan(), text.lines().next().unwrap_or("").dimmed());

                    let _ = bot.send_typing(chat_id).await;

                    // Get or create per-chat context
                    // Note: safe because updates are processed sequentially within select! arm
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
                            ct.record(&model, None, result.usage.input_tokens, result.usage.output_tokens, 0, 0);
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
