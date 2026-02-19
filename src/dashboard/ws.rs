use axum::{
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;

use super::SharedState;
use crate::agent::AgentLoop;
use crate::auth::AuthStore;
use crate::client::AnthropicClient;
use crate::context::ContextManager;
use crate::memory::{MemoryManager, TaskStorage};
use crate::tools;
use crate::types::Thinking;

#[derive(Deserialize)]
pub struct ChatQuery {
    pub bot: Option<String>,
}

/// WebSocket handler for live chat
///
/// SECURITY: No authentication is performed on WebSocket connections.
/// This is safe only when the dashboard is bound to localhost (127.0.0.1).
/// If exposed on a public interface, an auth layer must be added.
pub async fn chat_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
    Query(query): Query<ChatQuery>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_chat(socket, state, query.bot))
}

/// Resolve bot name to (model, system_prompt, task_scope)
fn resolve_bot(state: &SharedState, bot_name: Option<&str>) -> (String, String, Vec<String>) {
    let default = (
        state.config.models.standard.clone(),
        "You are DevMan, a helpful coding assistant. Be concise and use tools proactively.".into(),
        vec!["*".into()],
    );

    let name = match bot_name {
        Some(n) if n != "manager" => n,
        _ => return default,
    };

    let empty = vec![];
    let bots = state.config.telegram.as_ref()
        .map(|t| &t.bots)
        .unwrap_or(&empty);

    for bot in bots {
        if bot.name == name {
            let model = match bot.default_model.as_str() {
                "quick" => state.config.models.quick.clone(),
                "complex" => state.config.models.complex.clone(),
                "manager" => state.config.models.manager.clone(),
                _ => state.config.models.standard.clone(),
            };

            let prompt = bot.system_prompt.clone().unwrap_or_else(|| {
                format!("You are a DevMan bot scoped to tasks: {:?}. Be helpful and concise.", bot.tasks)
            });

            return (model, prompt, bot.tasks.clone());
        }
    }

    default
}

async fn handle_chat(mut socket: WebSocket, state: SharedState, bot_name: Option<String>) {
    let (model, system_prompt, task_scope) = resolve_bot(&state, bot_name.as_deref());
    let display_name = bot_name.as_deref().unwrap_or("manager");

    // Send welcome
    let welcome = serde_json::json!({
        "role": "assistant",
        "text": format!("Connected to {} bot. Type a message to chat.", display_name)
    });
    let _ = socket
        .send(Message::Text(serde_json::to_string(&welcome).unwrap().into()))
        .await;

    // Try to create an agent for this session
    let auth = match AuthStore::load() {
        Ok(a) => a,
        Err(e) => {
            let _ = send_json(&mut socket, "assistant", &format!("Auth error: {e}")).await;
            return;
        }
    };

    let api_key = match auth.anthropic_api_key() {
        Ok(k) => k,
        Err(e) => {
            let _ = send_json(&mut socket, "assistant", &format!("No API key: {e}")).await;
            return;
        }
    };

    let brave_key = auth.brave_api_key();
    let github_token = auth.github_token();
    let tool_defs = tools::builtin_tool_definitions(state.config.tools.web_enabled, state.config.github.is_some());

    let context = ContextManager::new();
    let mut agent = AgentLoop::new(
        AnthropicClient::new(api_key),
        context,
        model.clone(),
        system_prompt,
        tool_defs,
        state.config.agents.max_turns,
        state.config.agents.max_tokens,
        Thinking::Off,
        brave_key,
        github_token,
    );

    // Attach scoped storage if this is a scoped bot with a single task
    if task_scope.len() == 1 && task_scope[0] != "*" {
        let mm = MemoryManager::new(MemoryManager::default_root());
        agent = agent.with_storage(mm.task_storage(&task_scope[0]));
    }

    // Chat loop
    while let Some(Ok(msg)) = socket.next().await {
        match msg {
            Message::Text(text) => {
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(user_text) = data["text"].as_str() {
                        let _ = state.log_tx.send(format!("[chat:{}] User: {}", display_name, user_text));

                        match agent.run_turn(user_text).await {
                            Ok(result) => {
                                let _ = send_json(&mut socket, "assistant", &result.text).await;

                                let mut ct = state.cost_tracker.write().await;
                                ct.record(
                                    &model,
                                    Some(display_name),
                                    result.usage.input_tokens,
                                    result.usage.output_tokens,
                                    0,
                                    0,
                                );

                                let _ = state.log_tx.send(format!(
                                    "[chat:{}] Reply ({} in / {} out tokens)",
                                    display_name, result.usage.input_tokens, result.usage.output_tokens
                                ));
                            }
                            Err(e) => {
                                let _ = send_json(&mut socket, "assistant", &format!("Error: {e}")).await;
                            }
                        }
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}

async fn send_json(socket: &mut WebSocket, role: &str, text: &str) -> Result<(), axum::Error> {
    let msg = serde_json::json!({ "role": role, "text": text });
    socket
        .send(Message::Text(serde_json::to_string(&msg).unwrap().into()))
        .await
        .map_err(|e| axum::Error::new(e))
}

/// WebSocket handler for live log streaming
///
/// SECURITY: No authentication — same assumptions as chat_handler above.
pub async fn logs_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_logs(socket, state))
}

async fn handle_logs(mut socket: WebSocket, state: SharedState) {
    let mut log_rx = state.log_tx.subscribe();

    loop {
        tokio::select! {
            result = log_rx.recv() => {
                match result {
                    Ok(line) => {
                        if socket.send(Message::Text(line.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        let msg = format!("[...skipped {} log lines...]", n);
                        let _ = socket.send(Message::Text(msg.into())).await;
                    }
                    Err(_) => break,
                }
            }
            msg = socket.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}
