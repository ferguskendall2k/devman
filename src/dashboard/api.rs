use axum::{
    extract::{State, Path, Query},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::SharedState;

// ── Status ──────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct StatusResponse {
    pub status: String,
    pub uptime: String,
    pub cost_usd: f64,
    pub total_tokens: u64,
    pub version: String,
}

pub async fn status(State(state): State<SharedState>) -> Json<StatusResponse> {
    let cost = state.cost_tracker.read().await;
    let uptime = chrono::Utc::now() - state.start_time;
    let hours = uptime.num_hours();
    let mins = uptime.num_minutes() % 60;

    Json(StatusResponse {
        status: "online".into(),
        uptime: format!("{hours}h {mins}m"),
        cost_usd: cost.session_total.estimated_cost_usd,
        total_tokens: cost.session_total.input_tokens + cost.session_total.output_tokens,
        version: env!("CARGO_PKG_VERSION").into(),
    })
}

// ── Agents ──────────────────────────────────────────────────────────

#[derive(Clone, Serialize)]
pub struct AgentInfo {
    pub run_id: String,
    pub task_id: String,
    pub model: String,
    pub status: String,
    pub cost_usd: f64,
}

pub async fn agents_list(State(state): State<SharedState>) -> Json<Vec<AgentInfo>> {
    let agents = state.agents.read().await;
    Json(agents.clone())
}

// ── Bots ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct BotInfo {
    pub name: String,
    pub model: String,
    pub tasks: Vec<String>,
}

pub async fn bots_list(State(state): State<SharedState>) -> Json<Vec<BotInfo>> {
    let mut bots = vec![BotInfo {
        name: "manager".into(),
        model: state.config.models.standard.clone(),
        tasks: vec!["*".into()],
    }];

    if let Some(ref tg) = state.config.telegram {
        for bot in &tg.bots {
            bots.push(BotInfo {
                name: bot.name.clone(),
                model: bot.default_model.clone(),
                tasks: bot.tasks.clone(),
            });
        }
    }

    Json(bots)
}

// ── Chat History ────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub text: String,
}

pub async fn chat_history(
    State(state): State<SharedState>,
    Path(bot_name): Path<String>,
) -> Json<Vec<ChatMessage>> {
    let chats_dir = match &state.chats_dir {
        Some(d) => d.clone(),
        None => return Json(vec![]),
    };

    // Bot conversations are stored per chat_id. Find all conversation files for this bot.
    let bot_dir = if bot_name == "manager" {
        // Manager history could be in chats/manager/ or directly in chats/ (legacy)
        let manager_dir = chats_dir.join("manager");
        if manager_dir.is_dir() { manager_dir } else { chats_dir.clone() }
    } else {
        chats_dir.join(&bot_name)
    };

    let mut messages = Vec::new();

    // Read all .json conversation files in the bot dir
    if let Ok(entries) = std::fs::read_dir(&bot_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(conv) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(msgs) = conv["messages"].as_array() {
                            for msg in msgs {
                                let role = msg["role"].as_str().unwrap_or("unknown");
                                // Skip tool_use and tool_result, only show text
                                if role != "user" && role != "assistant" { continue; }

                                // Extract text from content blocks
                                if let Some(content) = msg["content"].as_array() {
                                    for block in content {
                                        if block["type"].as_str() == Some("text") {
                                            if let Some(text) = block["text"].as_str() {
                                                messages.push(ChatMessage {
                                                    role: role.to_string(),
                                                    text: text.to_string(),
                                                });
                                            }
                                        }
                                    }
                                } else if let Some(text) = msg["content"].as_str() {
                                    messages.push(ChatMessage {
                                        role: role.to_string(),
                                        text: text.to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Also check legacy path (chats/<chat_id>.json without bot subfolder) for manager
    if bot_name == "manager" {
        if let Ok(entries) = std::fs::read_dir(&chats_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().map(|e| e == "json").unwrap_or(false) {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(conv) = serde_json::from_str::<serde_json::Value>(&content) {
                            if let Some(msgs) = conv["messages"].as_array() {
                                for msg in msgs {
                                    let role = msg["role"].as_str().unwrap_or("unknown");
                                    if role != "user" && role != "assistant" { continue; }
                                    if let Some(content) = msg["content"].as_array() {
                                        for block in content {
                                            if block["type"].as_str() == Some("text") {
                                                if let Some(text) = block["text"].as_str() {
                                                    messages.push(ChatMessage {
                                                        role: role.to_string(),
                                                        text: text.to_string(),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Json(messages)
}

// ── Cost ────────────────────────────────────────────────────────────

pub async fn cost_summary(State(state): State<SharedState>) -> Json<serde_json::Value> {
    let cost = state.cost_tracker.read().await;
    Json(serde_json::json!({
        "session": {
            "input_tokens": cost.session_total.input_tokens,
            "output_tokens": cost.session_total.output_tokens,
            "estimated_cost_usd": cost.session_total.estimated_cost_usd,
        },
        "by_model": cost.by_model,
        "by_task": cost.by_task,
    }))
}

// ── Config ──────────────────────────────────────────────────────────

pub async fn config_get(State(state): State<SharedState>) -> Json<serde_json::Value> {
    // Return safe config (no secrets)
    Json(serde_json::json!({
        "models": state.config.models,
        "tools": state.config.tools,
        "agents": state.config.agents,
        "dashboard": state.config.dashboard,
    }))
}

#[derive(Deserialize)]
pub struct ConfigUpdate {
    pub models: Option<ModelsUpdate>,
    pub tools: Option<ToolsUpdate>,
    pub agents: Option<AgentsUpdate>,
}

#[derive(Deserialize)]
pub struct ModelsUpdate {
    pub manager: Option<String>,
    pub quick: Option<String>,
    pub standard: Option<String>,
    pub complex: Option<String>,
}

#[derive(Deserialize)]
pub struct ToolsUpdate {
    pub shell_confirm: Option<bool>,
    pub web_enabled: Option<bool>,
}

#[derive(Deserialize)]
pub struct AgentsUpdate {
    pub max_concurrent: Option<u32>,
    pub max_turns: Option<u32>,
    pub max_tokens: Option<u32>,
}

pub async fn config_update(
    State(state): State<SharedState>,
    Json(update): Json<ConfigUpdate>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Read current config file
    let config_path = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("devman")
        .join("config.toml");

    let content = std::fs::read_to_string(&config_path)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut doc: toml_edit::DocumentMut = content.parse()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Apply model updates
    if let Some(models) = &update.models {
        if let Some(ref v) = models.manager { doc["models"]["manager"] = toml_edit::value(v.as_str()); }
        if let Some(ref v) = models.quick { doc["models"]["quick"] = toml_edit::value(v.as_str()); }
        if let Some(ref v) = models.standard { doc["models"]["standard"] = toml_edit::value(v.as_str()); }
        if let Some(ref v) = models.complex { doc["models"]["complex"] = toml_edit::value(v.as_str()); }
    }

    // Apply tool updates
    if let Some(tools) = &update.tools {
        if let Some(v) = tools.shell_confirm { doc["tools"]["shell_confirm"] = toml_edit::value(v); }
        if let Some(v) = tools.web_enabled { doc["tools"]["web_enabled"] = toml_edit::value(v); }
    }

    // Apply agent updates
    if let Some(agents) = &update.agents {
        if let Some(v) = agents.max_concurrent { doc["agents"]["max_concurrent"] = toml_edit::value(v as i64); }
        if let Some(v) = agents.max_turns { doc["agents"]["max_turns"] = toml_edit::value(v as i64); }
        if let Some(v) = agents.max_tokens { doc["agents"]["max_tokens"] = toml_edit::value(v as i64); }
    }

    // Write back
    std::fs::write(&config_path, doc.to_string())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Log
    let _ = state.log_tx.send("⚙️ Config updated via dashboard".into());

    Ok(Json(serde_json::json!({ "ok": true, "note": "Config saved. Restart DevMan to apply changes." })))
}

// ── Logs ────────────────────────────────────────────────────────────

pub async fn logs_buffer(State(state): State<SharedState>) -> Json<Vec<String>> {
    let buf = state.log_buffer.read().await;
    Json(buf.clone())
}

// ── Tasks ───────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct TaskInfo {
    pub slug: String,
    pub files: Vec<TaskFile>,
    pub file_count: usize,
    pub total_bytes: u64,
}

#[derive(Serialize)]
pub struct TaskFile {
    pub path: String,       // relative to task storage root
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
}

pub async fn tasks_list(State(state): State<SharedState>) -> Json<Vec<TaskInfo>> {
    let root = crate::memory::MemoryManager::default_root().join("tasks");
    let mut tasks = Vec::new();

    // Manager-level files (from chat files directory)
    if let Some(ref chats_dir) = state.chats_dir {
        // Find *_files directories at the chats root level (manager's files)
        if let Ok(entries) = std::fs::read_dir(chats_dir) {
            let mut mgr_files = Vec::new();
            let mut mgr_bytes = 0u64;
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if entry.path().is_dir() && name.ends_with("_files") {
                    collect_files(&entry.path(), &entry.path(), &mut mgr_files, &mut mgr_bytes);
                }
            }
            if !mgr_files.is_empty() {
                let count = mgr_files.iter().filter(|f| !f.is_dir).count();
                tasks.push(TaskInfo {
                    slug: "manager".into(),
                    files: mgr_files,
                    file_count: count,
                    total_bytes: mgr_bytes,
                });
            }
        }

        // Scoped bot files (from chats/<bot_name>/<chat_id>_files/)
        if let Some(ref tg) = state.config.telegram {
            for bot in &tg.bots {
                let bot_dir = chats_dir.join(&bot.name);
                if !bot_dir.is_dir() { continue; }
                let mut bot_files = Vec::new();
                let mut bot_bytes = 0u64;
                if let Ok(entries) = std::fs::read_dir(&bot_dir) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if entry.path().is_dir() && name.ends_with("_files") {
                            collect_files(&entry.path(), &entry.path(), &mut bot_files, &mut bot_bytes);
                        }
                    }
                }
                // Also include task storage files
                for task in &bot.tasks {
                    let storage_dir = root.join(task).join("storage");
                    if storage_dir.is_dir() {
                        collect_files(&storage_dir, &storage_dir, &mut bot_files, &mut bot_bytes);
                    }
                }
                if !bot_files.is_empty() {
                    let count = bot_files.iter().filter(|f| !f.is_dir).count();
                    tasks.push(TaskInfo {
                        slug: bot.name.clone(),
                        files: bot_files,
                        file_count: count,
                        total_bytes: bot_bytes,
                    });
                }
            }
        }
    }

    // Also include any task storage dirs that aren't already covered by a bot
    let covered: Vec<String> = state.config.telegram.as_ref()
        .map(|t| t.bots.iter().flat_map(|b| b.tasks.clone()).collect())
        .unwrap_or_default();

    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten() {
            if !entry.path().is_dir() { continue; }
            let slug = entry.file_name().to_string_lossy().to_string();
            if covered.contains(&slug) { continue; }
            let storage_dir = entry.path().join("storage");
            let mut files = Vec::new();
            let mut total_bytes = 0u64;
            if storage_dir.is_dir() {
                collect_files(&storage_dir, &storage_dir, &mut files, &mut total_bytes);
            }
            if !files.is_empty() {
                let count = files.iter().filter(|f| !f.is_dir).count();
                tasks.push(TaskInfo {
                    slug,
                    files,
                    file_count: count,
                    total_bytes: total_bytes,
                });
            }
        }
    }

    Json(tasks)
}

fn collect_files(base: &std::path::Path, dir: &std::path::Path, files: &mut Vec<TaskFile>, total: &mut u64) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let rel = path.strip_prefix(base).unwrap_or(&path).to_string_lossy().to_string();
            let name = entry.file_name().to_string_lossy().to_string();
            let meta = entry.metadata().ok();
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);

            if is_dir {
                files.push(TaskFile { path: rel, name, size: 0, is_dir: true });
                collect_files(base, &path, files, total);
            } else {
                *total += size;
                files.push(TaskFile { path: rel, name, size, is_dir: false });
            }
        }
    }
}

#[derive(Deserialize)]
pub struct FileQuery {
    pub path: Option<String>,
}

pub async fn task_file_read(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
    Query(q): Query<FileQuery>,
) -> Result<String, StatusCode> {
    let file_path = q.path.unwrap_or_default();

    // Prevent directory traversal
    if file_path.contains("..") {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Try task storage first
    let storage_path = crate::memory::MemoryManager::default_root()
        .join("tasks")
        .join(&slug)
        .join("storage")
        .join(&file_path);
    if let Ok(content) = std::fs::read_to_string(&storage_path) {
        return Ok(content);
    }

    // Try chat files directory (for manager and bot files)
    if let Some(ref chats_dir) = state.chats_dir {
        // For "manager", files are in chats/*_files/
        if slug == "manager" {
            if let Ok(entries) = std::fs::read_dir(chats_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if entry.path().is_dir() && name.ends_with("_files") {
                        let full = entry.path().join(&file_path);
                        if let Ok(content) = std::fs::read_to_string(&full) {
                            return Ok(content);
                        }
                    }
                }
            }
        } else {
            // Scoped bot: chats/<bot>/*_files/
            let bot_dir = chats_dir.join(&slug);
            if let Ok(entries) = std::fs::read_dir(&bot_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if entry.path().is_dir() && name.ends_with("_files") {
                        let full = entry.path().join(&file_path);
                        if let Ok(content) = std::fs::read_to_string(&full) {
                            return Ok(content);
                        }
                    }
                }
            }
        }
    }

    Err(StatusCode::NOT_FOUND)
}

// ── Temp Files ──────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct TmpStatus {
    pub file_count: usize,
    pub total_bytes: u64,
    pub files: Vec<TaskFile>,
}

pub async fn tmp_status() -> Json<TmpStatus> {
    let tmp_dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("devman")
        .join("tmp");
    let mut files = Vec::new();
    let mut total = 0u64;
    if tmp_dir.is_dir() {
        collect_files(&tmp_dir, &tmp_dir, &mut files, &mut total);
    }
    let count = files.iter().filter(|f| !f.is_dir).count();
    Json(TmpStatus { file_count: count, total_bytes: total, files })
}

pub async fn tmp_clear(State(state): State<SharedState>) -> Json<serde_json::Value> {
    let tmp_dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("devman")
        .join("tmp");
    let mut cleared = 0usize;
    if let Ok(entries) = std::fs::read_dir(&tmp_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let _ = std::fs::remove_file(&path);
                cleared += 1;
            } else if path.is_dir() {
                let _ = std::fs::remove_dir_all(&path);
                cleared += 1;
            }
        }
    }
    let _ = state.log_tx.send(format!("🧹 Cleared {} items from tmp", cleared));
    Json(serde_json::json!({ "ok": true, "cleared": cleared }))
}

// ── Docs ────────────────────────────────────────────────────────────

pub async fn docs() -> String {
    include_str!("../../README.md").to_string()
}

// ── Org Chart ───────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct OrgNode {
    pub name: String,
    pub role: String,          // "manager" | "scoped" | "sub-agent"
    pub model: String,
    pub tasks: Vec<String>,
    pub status: String,        // "online" | "idle"
    pub children: Vec<OrgNode>,
}

pub async fn org_chart(State(state): State<SharedState>) -> Json<OrgNode> {
    let config = &state.config;

    // Build scoped bot children
    let scoped_bots: Vec<OrgNode> = config.telegram.as_ref()
        .map(|t| &t.bots)
        .unwrap_or(&vec![])
        .iter()
        .map(|bot| OrgNode {
            name: bot.name.clone(),
            role: "scoped".into(),
            model: bot.default_model.clone(),
            tasks: bot.tasks.clone(),
            status: "online".into(),
            children: vec![],
        })
        .collect();

    // Sub-agents from the agents list
    let agents = state.agents.read().await;
    let sub_agents: Vec<OrgNode> = agents.iter().map(|a| OrgNode {
        name: a.run_id.clone(),
        role: "sub-agent".into(),
        model: a.model.clone(),
        tasks: vec![a.task_id.clone()],
        status: a.status.clone(),
        children: vec![],
    }).collect();

    let mut children = scoped_bots;
    children.extend(sub_agents);

    Json(OrgNode {
        name: "Manager".into(),
        role: "manager".into(),
        model: config.models.standard.clone(),
        tasks: vec!["*".into()],
        status: "online".into(),
        children,
    })
}
