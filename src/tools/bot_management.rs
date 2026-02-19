use anyhow::{Context, Result};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::config::{Config, ScopedBotConfig};
use crate::memory::MemoryManager;

/// Global restart flag — set by tools that need a serve loop restart
pub static RESTART_REQUESTED: AtomicBool = AtomicBool::new(false);

pub async fn assign_bot_execute(input: &Value) -> Result<String> {
    let name = input["name"].as_str().unwrap_or("").to_string();
    let bot_token = input["bot_token"].as_str().unwrap_or("").to_string();
    let tasks: Vec<String> = input["tasks"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    if name.is_empty() || bot_token.is_empty() || tasks.is_empty() {
        anyhow::bail!("name, bot_token, and tasks are all required");
    }

    let allowed_users: Vec<i64> = input["allowed_users"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_i64()).collect())
        .unwrap_or_default();

    let default_model = input["default_model"]
        .as_str()
        .unwrap_or("standard")
        .to_string();

    let memory_access = input["memory_access"]
        .as_str()
        .unwrap_or("scoped")
        .to_string();

    let system_prompt = input["system_prompt"].as_str().map(String::from);

    // --- 1. Create task in memory if it doesn't exist ---
    let mm = MemoryManager::new(MemoryManager::default_root());
    let mut tasks_created = Vec::new();
    for task in &tasks {
        if task == "*" {
            continue;
        }
        // Check if task already exists
        if mm.load_task(task).is_err() {
            // Create the task
            match mm.create_task(task, "general") {
                Ok(msg) => tasks_created.push(msg),
                Err(e) => tracing::warn!("Failed to create task '{}': {}", task, e),
            }
        }
    }

    // --- 2. Update config ---
    let mut config = Config::load().context("loading config")?;

    let tg = config.telegram.get_or_insert_with(|| crate::config::TelegramConfig {
        bot_token: None,
        allowed_users: vec![],
        bots: vec![],
    });

    // Check for duplicate name
    if tg.bots.iter().any(|b| b.name == name) {
        anyhow::bail!("Bot '{}' already exists. Remove it first or use a different name.", name);
    }

    // If no allowed_users specified, inherit from manager
    let users = if allowed_users.is_empty() {
        tg.allowed_users.clone()
    } else {
        allowed_users
    };

    // Build system prompt if not provided
    let prompt = system_prompt.unwrap_or_else(|| {
        format!(
            "You are a DevMan scoped bot for tasks: {:?}. \
             Use storage tools to save reference files and research. \
             Use memory tools to track progress and decisions. \
             Be helpful, concise, and proactive about organizing information.",
            tasks
        )
    });

    let new_bot = ScopedBotConfig {
        name: name.clone(),
        bot_token,
        allowed_users: users,
        tasks: tasks.clone(),
        system_prompt: Some(prompt),
        system_prompt_file: None,
        default_model,
        memory_access,
    };

    tg.bots.push(new_bot);
    config.save().context("saving config")?;

    // --- 3. Signal restart ---
    RESTART_REQUESTED.store(true, Ordering::SeqCst);

    let mut result = format!("✅ Bot '{}' assigned to tasks: {:?}", name, tasks);
    for tc in &tasks_created {
        result.push_str(&format!("\n📋 {tc}"));
    }
    result.push_str("\n🔄 Restarting to activate...");
    Ok(result)
}

pub async fn list_bots_execute(_input: &Value) -> Result<String> {
    let config = Config::load().context("loading config")?;

    let bots = config.telegram
        .as_ref()
        .map(|t| &t.bots)
        .filter(|b| !b.is_empty());

    match bots {
        Some(bots) => {
            let mut out = format!("{} scoped bot(s):\n", bots.len());
            for b in bots {
                out.push_str(&format!(
                    "\n📱 **{}** → tasks: {:?} | model: {} | access: {}",
                    b.name, b.tasks, b.default_model, b.memory_access
                ));
                if !b.allowed_users.is_empty() {
                    out.push_str(&format!(" | users: {:?}", b.allowed_users));
                }
            }
            Ok(out)
        }
        None => Ok("No scoped bots configured.".to_string()),
    }
}

pub async fn remove_bot_execute(input: &Value) -> Result<String> {
    let name = input["name"].as_str().unwrap_or("");
    if name.is_empty() {
        anyhow::bail!("name is required");
    }

    let mut config = Config::load().context("loading config")?;

    let tg = config.telegram.as_mut()
        .ok_or_else(|| anyhow::anyhow!("No telegram config found"))?;

    let before = tg.bots.len();
    tg.bots.retain(|b| b.name != name);

    if tg.bots.len() == before {
        anyhow::bail!("Bot '{}' not found", name);
    }

    config.save().context("saving config")?;
    RESTART_REQUESTED.store(true, Ordering::SeqCst);
    Ok(format!("✅ Bot '{}' removed. 🔄 Restarting to apply...", name))
}
