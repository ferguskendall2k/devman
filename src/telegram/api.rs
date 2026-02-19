use anyhow::{Context, Result};
use reqwest::Client;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::types::{ApiResponse, RateLimitError, TgFile, TgMessage, Update};

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
        // Try with Markdown first, then plain text, with rate limit retry
        for attempt in 0..3 {
            let parse_mode = if attempt == 0 { Some("Markdown") } else { None };

            let mut body = serde_json::json!({
                "chat_id": chat_id,
                "text": text,
            });
            if let Some(pm) = parse_mode {
                body["parse_mode"] = serde_json::Value::String(pm.into());
            }

            let resp: ApiResponse<TgMessage> = self
                .client
                .post(format!("{}sendMessage", self.base_url))
                .json(&body)
                .send()
                .await
                .context("sending Telegram message")?
                .json()
                .await
                .context("parsing sendMessage response")?;

            if resp.ok {
                return resp.result.context("no message in response");
            }

            let desc = resp.description.as_deref().unwrap_or("");

            // Rate limited — wait and retry
            if desc.contains("Too Many Requests") || desc.contains("retry after") {
                let wait = resp.parameters
                    .and_then(|p| p.retry_after)
                    .unwrap_or(5);
                tracing::warn!("Telegram rate limited — waiting {wait}s");
                tokio::time::sleep(Duration::from_secs(wait)).await;
                continue;
            }

            // Markdown parse error — retry without parse_mode
            if parse_mode.is_some() && (desc.contains("parse") || desc.contains("format") || desc.contains("entities")) {
                tracing::debug!("Markdown send failed ({desc}), retrying as plain text");
                continue;
            }

            anyhow::bail!("sendMessage failed: {desc}");
        }

        anyhow::bail!("sendMessage failed after 3 attempts")
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

    /// Get file info (needed to get file_path for download)
    pub async fn get_file(&self, file_id: &str) -> Result<TgFile> {
        let resp: ApiResponse<TgFile> = self
            .client
            .get(format!("{}getFile", self.base_url))
            .query(&[("file_id", file_id)])
            .send()
            .await
            .context("getting file info")?
            .json()
            .await
            .context("parsing getFile response")?;

        if !resp.ok {
            anyhow::bail!("getFile failed: {}", resp.description.unwrap_or_default());
        }
        resp.result.context("no file in response")
    }

    /// Download a file to a local path. Returns the local path.
    pub async fn download_file(&self, file_path: &str, local_dir: &Path, filename: &str) -> Result<PathBuf> {
        // Extract token from base_url: https://api.telegram.org/bot<TOKEN>/
        let token = self.base_url
            .strip_prefix("https://api.telegram.org/bot")
            .and_then(|s| s.strip_suffix('/'))
            .unwrap_or("");

        let url = format!("https://api.telegram.org/file/bot{token}/{file_path}");

        let bytes = self
            .client
            .get(&url)
            .send()
            .await
            .context("downloading file")?
            .bytes()
            .await
            .context("reading file bytes")?;

        std::fs::create_dir_all(local_dir)?;
        let local_path = local_dir.join(filename);
        std::fs::write(&local_path, &bytes)?;

        Ok(local_path)
    }

    /// Download a Telegram file by file_id, returns (local_path, original_filename)
    pub async fn download_by_id(&self, file_id: &str, local_dir: &Path, fallback_name: &str) -> Result<PathBuf> {
        let file_info = self.get_file(file_id).await?;
        let tg_path = file_info.file_path.context("no file_path in response")?;

        // Use the extension from Telegram's path if available
        let ext = Path::new(&tg_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let filename = if fallback_name.contains('.') {
            fallback_name.to_string()
        } else if !ext.is_empty() {
            format!("{fallback_name}.{ext}")
        } else {
            fallback_name.to_string()
        };

        self.download_file(&tg_path, local_dir, &filename).await
    }

    pub fn is_allowed(&self, user_id: i64) -> bool {
        self.allowed_users.is_empty() || self.allowed_users.contains(&user_id)
    }
}
