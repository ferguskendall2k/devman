use anyhow::{Context, Result};
use colored::Colorize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::RwLock;

use crate::agent::AgentLoop;
use crate::auth::AuthStore;
use crate::client::AnthropicClient;
use crate::config::{Config, ScopedBotConfig};
use crate::context::ContextManager;
use crate::cost::CostTracker;
use crate::cron::CronScheduler;
use crate::memory::{MemoryManager, TaskStorage};
use crate::telegram::api::TelegramBot;
use crate::telegram::types::TgMessage;
use crate::tools;
use crate::tools::bot_management::RESTART_REQUESTED;
use crate::types::Thinking;
use std::sync::atomic::Ordering;

/// Per-chat conversation state
struct ChatState {
    context: ContextManager,
}

/// A running bot instance (manager or scoped)
struct BotInstance {
    name: String,
    bot: TelegramBot,
    offset: i64,
    chat_states: HashMap<i64, ChatState>,
    chats_dir: PathBuf,
    model: String,
    system_prompt: String,
    /// Task slugs this bot can access. Empty = all (manager).
    task_scope: Vec<String>,
    /// "scoped" or "full"
    memory_access: String,
}

impl BotInstance {
    /// Get appropriate TaskStorage for this bot
    fn task_storage(&self) -> Option<TaskStorage> {
        let mm = MemoryManager::new(MemoryManager::default_root());
        if self.memory_access == "full" || self.task_scope.is_empty() || self.task_scope == ["*"] {
            Some(mm.global_storage())
        } else if self.task_scope.len() == 1 {
            // Single task scope — give scoped storage for that task
            Some(mm.task_storage(&self.task_scope[0]))
        } else {
            // Multiple tasks but scoped — give global (tools enforce scope via system prompt)
            // TODO: implement multi-task scoped storage
            Some(mm.global_storage())
        }
    }
}

/// Extract text + download attachments from a Telegram message
async fn extract_message_content(bot: &TelegramBot, msg: &TgMessage, download_dir: &PathBuf) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(ref t) = msg.text {
        parts.push(t.clone());
    } else if let Some(ref c) = msg.caption {
        parts.push(c.clone());
    }

    if let Some(ref photos) = msg.photo {
        if let Some(largest) = photos.last() {
            match bot.download_by_id(&largest.file_id, download_dir, "photo").await {
                Ok(path) => parts.push(format!("[Image downloaded: {} ({}×{})]", path.display(), largest.width, largest.height)),
                Err(e) => parts.push(format!("[Failed to download image: {e}]")),
            }
        }
    }

    if let Some(ref doc) = msg.document {
        let name = doc.file_name.as_deref().unwrap_or("document");
        match bot.download_by_id(&doc.file_id, download_dir, name).await {
            Ok(path) => {
                let mime = doc.mime_type.as_deref().unwrap_or("unknown");
                let size = doc.file_size.unwrap_or(0);
                parts.push(format!("[File downloaded: {} ({mime}, {size} bytes)]", path.display()));
            }
            Err(e) => parts.push(format!("[Failed to download file: {e}]")),
        }
    }

    if let Some(ref voice) = msg.voice {
        match bot.download_by_id(&voice.file_id, download_dir, "voice").await {
            Ok(path) => parts.push(format!("[Voice message downloaded: {} ({}s)]", path.display(), voice.duration)),
            Err(e) => parts.push(format!("[Failed to download voice: {e}]")),
        }
    }

    if let Some(ref audio) = msg.audio {
        let name = audio.file_name.as_deref().unwrap_or("audio");
        match bot.download_by_id(&audio.file_id, download_dir, name).await {
            Ok(path) => parts.push(format!("[Audio downloaded: {} ({}s)]", path.display(), audio.duration)),
            Err(e) => parts.push(format!("[Failed to download audio: {e}]")),
        }
    }

    if let Some(ref video) = msg.video {
        let name = video.file_name.as_deref().unwrap_or("video");
        match bot.download_by_id(&video.file_id, download_dir, name).await {
            Ok(path) => parts.push(format!("[Video downloaded: {} ({}s, {}×{})]", path.display(), video.duration, video.width, video.height)),
            Err(e) => parts.push(format!("[Failed to download video: {e}]")),
        }
    }

    if let Some(ref sticker) = msg.sticker {
        let emoji = sticker.emoji.as_deref().unwrap_or("");
        parts.push(format!("[Sticker: {emoji}]"));
    }

    parts.join("\n")
}

/// Process a single message for a bot instance
async fn handle_message(
    instance: &mut BotInstance,
    msg: TgMessage,
    api_key: &str,
    tool_defs: &[crate::types::ToolDefinition],
    brave_api_key: &Option<String>,
    github_token: &Option<String>,
    cost_tracker: &Arc<RwLock<CostTracker>>,
    config: &Config,
) {
    let user = match msg.from {
        Some(ref u) => u,
        None => return,
    };

    if !instance.bot.is_allowed(user.id) {
        return;
    }

    let chat_id = msg.chat.id;
    let user_name = user.username.clone().unwrap_or_else(|| user.first_name.clone());
    let download_dir = instance.chats_dir.join(format!("{chat_id}_files"));

    let text = extract_message_content(&instance.bot, &msg, &download_dir).await;
    if text.is_empty() {
        return;
    }

    eprintln!("{} [{}] {} {}", "📩".dimmed(), instance.name.yellow(), user_name.cyan(), text.lines().next().unwrap_or("").dimmed());

    let _ = instance.bot.send_typing(chat_id).await;

    let storage = instance.task_storage();
    let chats_dir = instance.chats_dir.clone();

    let chat = instance.chat_states.entry(chat_id).or_insert_with(|| {
        ChatState {
            context: ContextManager::with_persistence(
                chats_dir.join(format!("{chat_id}.json")),
            ),
        }
    });

    let context = std::mem::replace(&mut chat.context, ContextManager::new());

    let mut agent = AgentLoop::new(
        AnthropicClient::new(api_key.to_string()),
        context,
        instance.model.clone(),
        instance.system_prompt.clone(),
        tool_defs.to_vec(),
        config.agents.max_turns,
        config.agents.max_tokens,
        Thinking::Off,
        brave_api_key.clone(),
        github_token.clone(),
    );

    if let Some(s) = storage {
        agent = agent.with_storage(s);
    }

    match agent.run_turn(&text).await {
        Ok(result) => {
            let reply = if result.text.is_empty() {
                "[No response]".to_string()
            } else {
                result.text
            };

            let reply = if reply.len() > 4000 {
                format!("{}...\n\n_(truncated)_", &reply[..4000])
            } else {
                reply
            };

            if let Err(e) = instance.bot.send_message(chat_id, &reply).await {
                tracing::error!("[{}] Failed to send reply: {e}", instance.name);
            }

            let mut ct = cost_tracker.write().await;
            ct.record(&instance.model, Some(&instance.name), result.usage.input_tokens, result.usage.output_tokens, 0, 0);
        }
        Err(e) => {
            tracing::error!("[{}] Agent error: {e}", instance.name);
            let _ = instance.bot.send_message(chat_id, &format!("❌ Error: {e}")).await;
        }
    }

    chat.context = agent.context;
}

/// Re-exec the current process with new args (Unix exec, replaces process)
fn exec_process(exe: &std::path::Path, args: &[&str]) -> std::io::Error {
    use std::os::unix::process::CommandExt;
    // This only returns if exec fails
    std::process::Command::new(exe).args(args).exec()
}

pub async fn run(config: &Config) -> Result<()> {
    let auth = AuthStore::load().context("loading credentials")?;
    let api_key = auth.anthropic_api_key()?;
    let brave_api_key = auth.brave_api_key();
    let github_token = auth.github_token();

    let cost_tracker = Arc::new(RwLock::new(CostTracker::new()));

    // State directory
    let state_dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("devman");
    std::fs::create_dir_all(&state_dir)?;

    // Cron
    let mut cron = CronScheduler::new(state_dir.join("cron-jobs.json"));

    // Tool definitions
    let tool_defs = tools::builtin_tool_definitions(config.tools.web_enabled, config.github.is_some());

    // Dashboard
    if config.dashboard.enabled {
        let dash_config = config.clone();
        let dash_cost = cost_tracker.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::dashboard::start(dash_config, dash_cost).await {
                tracing::error!("Dashboard error: {e}");
            }
        });
        eprintln!("{} Dashboard at {}", "🌐".dimmed(),
            format!("http://{}:{}", config.dashboard.bind, config.dashboard.port).cyan().bold());
    }

    // --- Manager bot ---
    let manager_token = auth.telegram_bot_token().context(
        "Telegram bot token not configured. Set TELEGRAM_BOT_TOKEN or add to credentials.toml",
    )?;
    let manager_users = config.telegram.as_ref()
        .map(|t| t.allowed_users.clone())
        .unwrap_or_default();

    let manager_chats_dir = state_dir.join("chats").join("manager");
    std::fs::create_dir_all(&manager_chats_dir)?;

    let mut manager = BotInstance {
        name: "manager".to_string(),
        bot: TelegramBot::new(manager_token, manager_users),
        offset: 0,
        chat_states: HashMap::new(),
        chats_dir: manager_chats_dir,
        model: config.models.standard.clone(),
        system_prompt: "You are DevMan, a helpful coding assistant. Be concise and use tools proactively.".to_string(),
        task_scope: vec!["*".to_string()],
        memory_access: "full".to_string(),
    };

    // --- Scoped bots ---
    let scoped_configs: Vec<ScopedBotConfig> = config.telegram.as_ref()
        .map(|t| t.bots.clone())
        .unwrap_or_default();

    let mut scoped_bots: Vec<BotInstance> = Vec::new();
    for sc in &scoped_configs {
        let bot_chats_dir = state_dir.join("chats").join(&sc.name);
        std::fs::create_dir_all(&bot_chats_dir)?;

        // Load system prompt from file or inline
        let sys_prompt = if let Some(ref path) = sc.system_prompt_file {
            std::fs::read_to_string(path).unwrap_or_else(|_| {
                format!("You are a DevMan bot scoped to tasks: {:?}. Be helpful and concise.", sc.tasks)
            })
        } else if let Some(ref prompt) = sc.system_prompt {
            prompt.clone()
        } else {
            format!("You are a DevMan bot scoped to tasks: {:?}. Be helpful and concise. Only use storage and memory tools for your assigned tasks.", sc.tasks)
        };

        // Resolve model tier to actual model name
        let model = match sc.default_model.as_str() {
            "quick" => config.models.quick.clone(),
            "complex" => config.models.complex.clone(),
            "manager" => config.models.manager.clone(),
            "standard" | _ => config.models.standard.clone(),
        };

        scoped_bots.push(BotInstance {
            name: sc.name.clone(),
            bot: TelegramBot::new(sc.bot_token.clone(), sc.allowed_users.clone()),
            offset: 0,
            chat_states: HashMap::new(),
            chats_dir: bot_chats_dir,
            model,
            system_prompt: sys_prompt,
            task_scope: sc.tasks.clone(),
            memory_access: sc.memory_access.clone(),
        });

        eprintln!("{} Scoped bot '{}' → tasks: {:?}", "🤖".dimmed(), sc.name.cyan(), sc.tasks);
    }

    eprintln!("{} {} {}", "🤖".bold(), "DevMan serving".green().bold(),
        format!("(manager + {} scoped bots)", scoped_bots.len()).dimmed());
    eprintln!("{}", "Press Ctrl+C to stop".dimmed());

    let mut cron_tick = tokio::time::interval(std::time::Duration::from_secs(30));
    let model = config.models.standard.clone();
    let system_prompt = "You are DevMan, a helpful coding assistant. Be concise and use tools proactively.".to_string();

    loop {
        // Check restart flag (set by assign_bot/remove_bot tools)
        if RESTART_REQUESTED.load(Ordering::SeqCst) {
            RESTART_REQUESTED.store(false, Ordering::SeqCst);
            eprintln!("\n{}", "🔄 Restart requested — re-execing...".yellow());
            cron.save()?;

            // Re-exec ourselves
            let exe = std::env::current_exe().context("finding current executable")?;
            let err = exec_process(&exe, &["serve"]);
            // exec_process only returns on error
            anyhow::bail!("Failed to re-exec: {err}");
        }

        let manager_poll = manager.bot.get_updates(manager.offset, 1);

        tokio::select! {
            _ = signal::ctrl_c() => {
                eprintln!("\n{}", "Shutting down...".yellow());
                cron.save()?;
                break;
            }

            _ = cron_tick.tick() => {
                let due_jobs = cron.tick();
                for job in due_jobs {
                    eprintln!("{} Cron fired: {}", "⏰".dimmed(), job.name);
                    match &job.action {
                        crate::cron::CronAction::SystemEvent { text } => {
                            eprintln!("  {}", text.dimmed());
                        }
                        crate::cron::CronAction::AgentTask { message, model: task_model } => {
                            let m = task_model.as_deref().unwrap_or(&model);
                            let client = AnthropicClient::new(api_key.clone());
                            let context = ContextManager::new();
                            let mut agent = AgentLoop::new(
                                client, context, m.to_string(), system_prompt.clone(),
                                tool_defs.clone(), config.agents.max_turns, config.agents.max_tokens,
                                Thinking::Off, brave_api_key.clone(), github_token.clone(),
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

            // Manager bot polling
            result = manager_poll => {
                if let Ok(updates) = result {
                    for update in updates {
                        manager.offset = update.update_id + 1;
                        if let Some(msg) = update.message {
                            handle_message(&mut manager, msg, &api_key, &tool_defs, &brave_api_key, &github_token, &cost_tracker, config).await;
                        }
                    }
                } else if let Err(e) = result {
                    tracing::error!("[manager] Poll error: {e}");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }

            // Scoped bots — poll all with a small sleep cycle
            _ = async {
                for (i, bot) in scoped_bots.iter_mut().enumerate() {
                    match bot.bot.get_updates(bot.offset, 0).await {
                        Ok(updates) => {
                            for update in updates {
                                bot.offset = update.update_id + 1;
                                if let Some(msg) = update.message {
                                    handle_message(bot, msg, &api_key, &tool_defs, &brave_api_key, &github_token, &cost_tracker, config).await;
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!("[{}] Poll error: {e}", bot.name);
                        }
                    }
                }
                // Only run this branch if there are scoped bots
                if scoped_bots.is_empty() {
                    // Never resolve if no scoped bots — let other branches win
                    std::future::pending::<()>().await;
                }
            } => {}
        }
    }

    Ok(())
}
