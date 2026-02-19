use anyhow::{Context, Result};
use reqwest::Client;
use std::time::Duration;

use super::types::{ApiResponse, TgMessage, Update};

pub struct TelegramBot {
    client: Client,
    base_url: String,
    allowed_users: Vec<i64>,
}

impl TelegramBot {
    pub fn new(token: String, allowed_users: Vec<i64>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("failed to build HTTP client");
        let base_url = format!("https://api.telegram.org/bot{token}/");
        Self {
            client,
            base_url,
            allowed_users,
        }
    }

    pub async fn get_updates(&self, offset: i64, timeout: u32) -> Result<Vec<Update>> {
        let resp: ApiResponse<Vec<Update>> = self
            .client
            .get(format!("{}getUpdates", self.base_url))
            .query(&[
                ("offset", offset.to_string()),
                ("timeout", timeout.to_string()),
            ])
            .send()
            .await
            .context("polling Telegram updates")?
            .json()
            .await
            .context("parsing Telegram updates")?;

        if !resp.ok {
            anyhow::bail!(
                "Telegram API error: {}",
                resp.description.unwrap_or_default()
            );
        }

        Ok(resp.result.unwrap_or_default())
    }

    pub async fn send_message(&self, chat_id: i64, text: &str) -> Result<TgMessage> {
        // Try with Markdown first; if Telegram rejects it (unbalanced formatting),
        // retry without parse_mode so the message still gets delivered.
        let resp: ApiResponse<TgMessage> = self
            .client
            .post(format!("{}sendMessage", self.base_url))
            .json(&serde_json::json!({
                "chat_id": chat_id,
                "text": text,
                "parse_mode": "Markdown",
            }))
            .send()
            .await
            .context("sending Telegram message")?
            .json()
            .await
            .context("parsing sendMessage response")?;

        if resp.ok {
            return resp.result.context("no message in response");
        }

        // Markdown failed — retry without parse_mode
        tracing::debug!(
            "Markdown send failed ({}), retrying as plain text",
            resp.description.as_deref().unwrap_or("unknown")
        );

        let resp2: ApiResponse<TgMessage> = self
            .client
            .post(format!("{}sendMessage", self.base_url))
            .json(&serde_json::json!({
                "chat_id": chat_id,
                "text": text,
            }))
            .send()
            .await
            .context("sending Telegram message (plain fallback)")?
            .json()
            .await
            .context("parsing sendMessage response (plain fallback)")?;

        if !resp2.ok {
            anyhow::bail!(
                "sendMessage failed: {}",
                resp2.description.unwrap_or_default()
            );
        }

        resp2.result.context("no message in response")
    }

    pub async fn send_typing(&self, chat_id: i64) -> Result<()> {
        let _resp: ApiResponse<bool> = self
            .client
            .post(format!("{}sendChatAction", self.base_url))
            .json(&serde_json::json!({
                "chat_id": chat_id,
                "action": "typing",
            }))
            .send()
            .await
            .context("sending typing action")?
            .json()
            .await
            .context("parsing sendChatAction response")?;

        Ok(())
    }

    pub fn is_allowed(&self, user_id: i64) -> bool {
        self.allowed_users.is_empty() || self.allowed_users.contains(&user_id)
    }
}
