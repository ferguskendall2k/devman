use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};

use super::SharedState;
use crate::agent::AgentLoop;
use crate::auth::AuthStore;
use crate::client::AnthropicClient;
use crate::context::ContextManager;
use crate::tools;
use crate::types::Thinking;

/// WebSocket handler for live chat
pub async fn chat_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_chat(socket, state))
}

async fn handle_chat(mut socket: WebSocket, state: SharedState) {
    // Send welcome
    let welcome = serde_json::json!({
        "role": "assistant",
        "text": "Connected to DevMan. Type a message to chat."
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
        state.config.models.standard.clone(),
        "You are DevMan, a helpful coding assistant. Be concise and use tools proactively.".into(),
        tool_defs,
        state.config.agents.max_turns,
        state.config.agents.max_tokens,
        Thinking::Off,
        brave_key,
        github_token,
    );

    // Chat loop
    while let Some(Ok(msg)) = socket.next().await {
        match msg {
            Message::Text(text) => {
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(user_text) = data["text"].as_str() {
                        // Log it
                        let _ = state.log_tx.send(format!("[chat] User: {}", user_text));

                        match agent.run_turn(user_text).await {
                            Ok(result) => {
                                let _ = send_json(&mut socket, "assistant", &result.text).await;

                                // Track cost
                                let mut ct = state.cost_tracker.write().await;
                                ct.record(
                                    &state.config.models.standard,
                                    None,
                                    result.usage.input_tokens,
                                    result.usage.output_tokens,
                                );

                                let _ = state.log_tx.send(format!(
                                    "[chat] Agent replied ({} in / {} out tokens)",
                                    result.usage.input_tokens, result.usage.output_tokens
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
            // Forward broadcast log messages to the client
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
            // Handle client messages (close)
            msg = socket.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}
