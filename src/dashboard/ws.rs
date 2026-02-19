use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};

use super::SharedState;

/// WebSocket handler for live chat
pub async fn chat_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_chat(socket, state))
}

async fn handle_chat(mut socket: WebSocket, _state: SharedState) {
    // Send welcome message
    let welcome = serde_json::json!({
        "role": "assistant",
        "text": "Connected to DevMan. Type a message to chat."
    });
    let _ = socket
        .send(Message::Text(serde_json::to_string(&welcome).unwrap().into()))
        .await;

    // Handle incoming messages
    while let Some(Ok(msg)) = socket.next().await {
        match msg {
            Message::Text(text) => {
                // Parse incoming message
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(user_text) = data["text"].as_str() {
                        // TODO: Route to agent loop and stream response back
                        // For now, echo acknowledgment
                        let response = serde_json::json!({
                            "role": "assistant",
                            "text": format!("Received: {}. (Agent routing not yet connected)", user_text)
                        });
                        let _ = socket
                            .send(Message::Text(
                                serde_json::to_string(&response).unwrap().into(),
                            ))
                            .await;
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}

/// WebSocket handler for live log streaming
pub async fn logs_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_logs(socket, state))
}

async fn handle_logs(mut socket: WebSocket, _state: SharedState) {
    // TODO: Connect to tracing subscriber and stream logs
    // For now, send periodic heartbeat
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let msg = format!("[{}] heartbeat", chrono::Utc::now().format("%H:%M:%S"));
                if socket.send(Message::Text(msg.into())).await.is_err() {
                    break;
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
