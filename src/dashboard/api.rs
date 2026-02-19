use axum::{extract::State, Json};
use serde::Serialize;

use super::SharedState;

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

#[derive(Serialize)]
pub struct AgentInfo {
    pub run_id: String,
    pub task_id: String,
    pub model: String,
    pub status: String,
    pub cost_usd: f64,
}

pub async fn agents_list(State(_state): State<SharedState>) -> Json<Vec<AgentInfo>> {
    // TODO: Wire up to orchestrator's agent list
    Json(vec![])
}

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

pub async fn config_get(State(state): State<SharedState>) -> Json<serde_json::Value> {
    // Return safe config (no secrets)
    Json(serde_json::json!({
        "models": state.config.models,
        "tools": state.config.tools,
        "agents": state.config.agents,
        "dashboard": state.config.dashboard,
    }))
}
