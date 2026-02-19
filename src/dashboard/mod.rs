pub mod api;
pub mod ws;

use anyhow::Result;
use axum::{
    Router,
    routing::{get, post},
    response::{Html, IntoResponse},
};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use crate::config::Config;
use crate::cost::CostTracker;

/// Shared state for the dashboard
pub struct DashboardState {
    pub config: Config,
    pub cost_tracker: Arc<RwLock<CostTracker>>,
    pub start_time: chrono::DateTime<chrono::Utc>,
    /// Broadcast channel for log lines
    pub log_tx: broadcast::Sender<String>,
    /// Recent log lines buffer for new dashboard connections
    pub log_buffer: RwLock<Vec<String>>,
    /// Active/completed agent records for the agents table
    pub agents: RwLock<Vec<api::AgentInfo>>,
    /// Chat history directory root
    pub chats_dir: Option<std::path::PathBuf>,
}

pub type SharedState = Arc<DashboardState>;

/// Send a log line to all connected dashboard clients and buffer it
pub fn broadcast_log(state: &SharedState, msg: String) {
    let _ = state.log_tx.send(msg.clone());
    // Buffer for late joiners (fire-and-forget; don't block on lock)
    let state = state.clone();
    tokio::spawn(async move {
        let mut buf = state.log_buffer.write().await;
        buf.push(msg);
        // Keep last 500 lines
        if buf.len() > 500 {
            let drain = buf.len() - 500;
            buf.drain(..drain);
        }
    });
}

/// Send a log line directly via the broadcast sender (for use without SharedState)
pub fn broadcast_log_tx(tx: &broadcast::Sender<String>, msg: String) {
    let _ = tx.send(msg);
}

/// Start the dashboard HTTP server, returning the shared state for external use
pub async fn start(
    config: Config,
    cost_tracker: Arc<RwLock<CostTracker>>,
    chats_dir: Option<std::path::PathBuf>,
) -> Result<SharedState> {
    let bind = format!("{}:{}", config.dashboard.bind, config.dashboard.port);

    let (log_tx, _) = broadcast::channel::<String>(256);

    let state = Arc::new(DashboardState {
        config: config.clone(),
        cost_tracker,
        start_time: chrono::Utc::now(),
        log_tx,
        log_buffer: RwLock::new(Vec::new()),
        agents: RwLock::new(Vec::new()),
        chats_dir,
    });

    let app = Router::new()
        // Dashboard SPA
        .route("/", get(index_handler))
        // API endpoints
        .route("/api/status", get(api::status))
        .route("/api/agents", get(api::agents_list))
        .route("/api/bots", get(api::bots_list))
        .route("/api/bots/{name}/history", get(api::chat_history))
        .route("/api/cost", get(api::cost_summary))
        .route("/api/config", get(api::config_get).post(api::config_update))
        .route("/api/logs", get(api::logs_buffer))
        .route("/api/tasks", get(api::tasks_list))
        .route("/api/tasks/{slug}/file", get(api::task_file_read))
        .route("/api/org", get(api::org_chart))
        .route("/api/docs", get(api::docs))
        .route("/api/tmp", get(api::tmp_status))
        .route("/api/tmp/clear", post(api::tmp_clear))
        // WebSocket for live streaming
        .route("/ws/chat", get(ws::chat_handler))
        .route("/ws/logs", get(ws::logs_handler))
        .with_state(state.clone());

    // Warn if dashboard is bound to a non-loopback address (WebSocket has no auth)
    if config.dashboard.bind != "127.0.0.1" && config.dashboard.bind != "localhost" {
        tracing::warn!(
            "Dashboard bound to {} — WebSocket endpoints have NO authentication! \
             Consider binding to 127.0.0.1 or adding an auth layer.",
            config.dashboard.bind
        );
    }

    tracing::info!("Dashboard starting on http://{bind}");

    let ret = state.clone();
    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(&bind).await.expect("bind dashboard");
        axum::serve(listener, app).await.expect("dashboard serve");
    });

    Ok(ret)
}

/// Serve the embedded SPA
async fn index_handler() -> impl IntoResponse {
    Html(DASHBOARD_HTML)
}

/// Embedded dashboard HTML — single-page app with tabs
const DASHBOARD_HTML: &str = include_str!("dashboard.html");
