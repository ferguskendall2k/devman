pub mod api;
pub mod ws;

use anyhow::Result;
use axum::{
    Router,
    routing::get,
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
}

pub type SharedState = Arc<DashboardState>;

/// Send a log line to all connected dashboard clients
pub fn broadcast_log(state: &SharedState, msg: String) {
    let _ = state.log_tx.send(msg);
}

/// Start the dashboard HTTP server
pub async fn start(
    config: Config,
    cost_tracker: Arc<RwLock<CostTracker>>,
) -> Result<()> {
    let bind = format!("{}:{}", config.dashboard.bind, config.dashboard.port);

    let (log_tx, _) = broadcast::channel::<String>(256);

    let state = Arc::new(DashboardState {
        config: config.clone(),
        cost_tracker,
        start_time: chrono::Utc::now(),
        log_tx,
    });

    let app = Router::new()
        // Dashboard SPA
        .route("/", get(index_handler))
        // API endpoints
        .route("/api/status", get(api::status))
        .route("/api/agents", get(api::agents_list))
        .route("/api/cost", get(api::cost_summary))
        .route("/api/config", get(api::config_get))
        // WebSocket for live streaming
        .route("/ws/chat", get(ws::chat_handler))
        .route("/ws/logs", get(ws::logs_handler))
        .with_state(state);

    tracing::info!("Dashboard starting on http://{bind}");

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Serve the embedded SPA
async fn index_handler() -> impl IntoResponse {
    Html(DASHBOARD_HTML)
}

/// Embedded dashboard HTML — single-page app
const DASHBOARD_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>DevMan Dashboard</title>
<style>
  :root {
    --bg: #0f1117;
    --surface: #1a1d27;
    --border: #2a2d3a;
    --text: #e4e4e7;
    --muted: #71717a;
    --accent: #3b82f6;
    --green: #22c55e;
    --red: #ef4444;
    --yellow: #eab308;
  }
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body { font-family: -apple-system, 'Segoe UI', sans-serif; background: var(--bg); color: var(--text); }
  .container { max-width: 1200px; margin: 0 auto; padding: 20px; }
  header { display: flex; align-items: center; justify-content: space-between; padding: 16px 0; border-bottom: 1px solid var(--border); margin-bottom: 24px; }
  header h1 { font-size: 1.5rem; }
  header h1 span { opacity: 0.5; }
  .status-badge { padding: 4px 12px; border-radius: 12px; font-size: 0.8rem; font-weight: 600; }
  .status-online { background: rgba(34,197,94,0.15); color: var(--green); }
  .status-offline { background: rgba(239,68,68,0.15); color: var(--red); }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(280px, 1fr)); gap: 16px; margin-bottom: 24px; }
  .card { background: var(--surface); border: 1px solid var(--border); border-radius: 12px; padding: 20px; }
  .card h3 { font-size: 0.85rem; color: var(--muted); text-transform: uppercase; letter-spacing: 0.05em; margin-bottom: 8px; }
  .card .value { font-size: 1.8rem; font-weight: 700; }
  .card .sub { font-size: 0.85rem; color: var(--muted); margin-top: 4px; }
  .section { background: var(--surface); border: 1px solid var(--border); border-radius: 12px; padding: 20px; margin-bottom: 16px; }
  .section h2 { font-size: 1.1rem; margin-bottom: 16px; }
  table { width: 100%; border-collapse: collapse; }
  th { text-align: left; padding: 8px 12px; color: var(--muted); font-size: 0.8rem; text-transform: uppercase; border-bottom: 1px solid var(--border); }
  td { padding: 10px 12px; border-bottom: 1px solid var(--border); font-size: 0.9rem; }
  .tag { display: inline-block; padding: 2px 8px; border-radius: 4px; font-size: 0.75rem; font-weight: 600; }
  .tag-running { background: rgba(59,130,246,0.15); color: var(--accent); }
  .tag-complete { background: rgba(34,197,94,0.15); color: var(--green); }
  .tag-failed { background: rgba(239,68,68,0.15); color: var(--red); }
  #chat { display: flex; flex-direction: column; height: 400px; }
  #chat-messages { flex: 1; overflow-y: auto; padding: 12px; font-family: 'SF Mono', 'Fira Code', monospace; font-size: 0.85rem; line-height: 1.6; }
  #chat-input-row { display: flex; gap: 8px; padding: 12px; border-top: 1px solid var(--border); }
  #chat-input { flex: 1; background: var(--bg); border: 1px solid var(--border); border-radius: 8px; padding: 10px 14px; color: var(--text); font-size: 0.9rem; outline: none; }
  #chat-input:focus { border-color: var(--accent); }
  #chat-send { background: var(--accent); color: white; border: none; border-radius: 8px; padding: 10px 20px; cursor: pointer; font-weight: 600; }
  #chat-send:hover { opacity: 0.9; }
  .msg-user { color: var(--green); }
  .msg-assistant { color: var(--text); }
  .msg-tool { color: var(--yellow); opacity: 0.7; }
  #logs { height: 300px; overflow-y: auto; font-family: monospace; font-size: 0.8rem; padding: 12px; background: var(--bg); border-radius: 8px; }
  .log-line { padding: 2px 0; }
  .log-info { color: var(--muted); }
  .log-warn { color: var(--yellow); }
  .log-error { color: var(--red); }
</style>
</head>
<body>
<div class="container">
  <header>
    <h1>🔧 DevMan <span>Dashboard</span></h1>
    <span id="status-badge" class="status-badge status-offline">Connecting...</span>
  </header>

  <div class="grid">
    <div class="card">
      <h3>Uptime</h3>
      <div class="value" id="uptime">--</div>
    </div>
    <div class="card">
      <h3>Session Cost</h3>
      <div class="value" id="cost">$0.00</div>
      <div class="sub" id="tokens">0 tokens</div>
    </div>
    <div class="card">
      <h3>Active Agents</h3>
      <div class="value" id="agents-count">0</div>
      <div class="sub" id="agents-model">—</div>
    </div>
  </div>

  <div class="section">
    <h2>💬 Chat</h2>
    <div id="chat">
      <div id="chat-messages"></div>
      <div id="chat-input-row">
        <input id="chat-input" type="text" placeholder="Send a message..." autocomplete="off">
        <button id="chat-send">Send</button>
      </div>
    </div>
  </div>

  <div class="section">
    <h2>🤖 Agents</h2>
    <table>
      <thead><tr><th>Run ID</th><th>Task</th><th>Model</th><th>Status</th><th>Cost</th></tr></thead>
      <tbody id="agents-table"></tbody>
    </table>
  </div>

  <div class="section">
    <h2>📋 Logs</h2>
    <div id="logs"></div>
  </div>
</div>

<script>
const API = window.location.origin;
const WS_BASE = API.replace('http', 'ws');

// Status polling
async function updateStatus() {
  try {
    const r = await fetch(API + '/api/status');
    const d = await r.json();
    document.getElementById('status-badge').textContent = 'Online';
    document.getElementById('status-badge').className = 'status-badge status-online';
    document.getElementById('uptime').textContent = d.uptime || '--';
    document.getElementById('cost').textContent = '$' + (d.cost_usd || 0).toFixed(4);
    document.getElementById('tokens').textContent = (d.total_tokens || 0).toLocaleString() + ' tokens';
  } catch(e) {
    document.getElementById('status-badge').textContent = 'Offline';
    document.getElementById('status-badge').className = 'status-badge status-offline';
  }
}

async function updateAgents() {
  try {
    const r = await fetch(API + '/api/agents');
    const agents = await r.json();
    const tbody = document.getElementById('agents-table');
    document.getElementById('agents-count').textContent = agents.filter(a => a.status === 'Running').length;
    tbody.innerHTML = agents.map(a => `
      <tr>
        <td style="font-family:monospace;font-size:0.8rem">${a.run_id.slice(0,24)}</td>
        <td>${a.task_id}</td>
        <td>${a.model}</td>
        <td><span class="tag tag-${a.status.toLowerCase()}">${a.status}</span></td>
        <td>$${(a.cost_usd || 0).toFixed(4)}</td>
      </tr>
    `).join('');
  } catch(e) {}
}

// Chat WebSocket
let chatWs = null;
function connectChat() {
  chatWs = new WebSocket(WS_BASE + '/ws/chat');
  chatWs.onmessage = (e) => {
    const msgs = document.getElementById('chat-messages');
    const div = document.createElement('div');
    const data = JSON.parse(e.data);
    div.className = 'msg-' + (data.role || 'assistant');
    div.textContent = (data.role === 'user' ? 'You: ' : 'Al: ') + data.text;
    msgs.appendChild(div);
    msgs.scrollTop = msgs.scrollHeight;
  };
  chatWs.onclose = () => setTimeout(connectChat, 3000);
}

function sendChat() {
  const input = document.getElementById('chat-input');
  const text = input.value.trim();
  if (!text || !chatWs) return;
  chatWs.send(JSON.stringify({ text }));
  const msgs = document.getElementById('chat-messages');
  const div = document.createElement('div');
  div.className = 'msg-user';
  div.textContent = 'You: ' + text;
  msgs.appendChild(div);
  msgs.scrollTop = msgs.scrollHeight;
  input.value = '';
}

document.getElementById('chat-send').addEventListener('click', sendChat);
document.getElementById('chat-input').addEventListener('keydown', (e) => {
  if (e.key === 'Enter') sendChat();
});

// Log WebSocket
let logWs = null;
function connectLogs() {
  logWs = new WebSocket(WS_BASE + '/ws/logs');
  logWs.onmessage = (e) => {
    const logs = document.getElementById('logs');
    const div = document.createElement('div');
    div.className = 'log-line log-info';
    div.textContent = e.data;
    logs.appendChild(div);
    if (logs.children.length > 500) logs.removeChild(logs.firstChild);
    logs.scrollTop = logs.scrollHeight;
  };
  logWs.onclose = () => setTimeout(connectLogs, 3000);
}

// Init
updateStatus();
updateAgents();
setInterval(updateStatus, 5000);
setInterval(updateAgents, 5000);
connectChat();
connectLogs();
</script>
</body>
</html>"#;
