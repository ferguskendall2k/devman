use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Credential store — reads from multiple sources
#[derive(Debug)]
pub struct AuthStore {
    credentials: Credentials,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Credentials {
    anthropic: Option<AnthropicCreds>,
    telegram: Option<TelegramCreds>,
    brave: Option<BraveCreds>,
    elevenlabs: Option<ElevenLabsCreds>,
    github: Option<GitHubCreds>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AnthropicCreds {
    api_key: Option<String>,
    auth_mode: Option<String>, // "apikey" | "oauth"
}

#[derive(Debug, Serialize, Deserialize)]
struct TelegramCreds {
    bot_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct BraveCreds {
    api_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ElevenLabsCreds {
    api_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct GitHubCreds {
    token: String,
}

/// Claude Code credential file format (~/.claude/.credentials.json)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeCodeCredentials {
    claude_ai_oauth: Option<ClaudeAiOAuth>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeAiOAuth {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<u64>,
}

/// OpenClaw auth-profiles.json format
#[derive(Debug, Deserialize)]
struct OpenClawAuthProfiles {
    profiles: Option<std::collections::HashMap<String, OpenClawProfile>>,
}

#[derive(Debug, Deserialize)]
struct OpenClawProfile {
    #[serde(rename = "type")]
    profile_type: Option<String>,
    provider: Option<String>,
    token: Option<String>,      // type: "token"
    access: Option<String>,     // type: "oauth"
    refresh: Option<String>,    // type: "oauth"
    expires: Option<u64>,
}

impl AuthStore {
    /// Load credentials with resolution order:
    /// 1. Environment variables
    /// 2. Claude Code CLI OAuth (~/.claude/.credentials.json)
    /// 3. OpenClaw auth-profiles (if installed)
    /// 4. DevMan credentials.toml
    pub fn load() -> Result<Self> {
        let creds_path = Self::credentials_path();
        let mut credentials = if creds_path.exists() {
            let content = std::fs::read_to_string(&creds_path)
                .with_context(|| "reading credentials.toml")?;
            toml::from_str(&content).unwrap_or_default()
        } else {
            Credentials::default()
        };

        // Override with env vars
        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            credentials.anthropic = Some(AnthropicCreds {
                api_key: Some(key),
                auth_mode: Some("apikey".into()),
            });
        }

        Ok(Self { credentials })
    }

    /// Resolve the Anthropic API key/token
    /// Priority: env → Claude Code OAuth → OpenClaw auth-profiles → credentials.toml
    pub fn anthropic_api_key(&self) -> Result<String> {
        // 1. Already loaded from env or credentials.toml
        if let Some(ref creds) = self.credentials.anthropic {
            if let Some(ref key) = creds.api_key {
                return Ok(key.clone());
            }
        }

        // 2. Try Claude Code OAuth (~/.claude/.credentials.json)
        if let Some(token) = Self::read_claude_code_oauth()? {
            return Ok(token);
        }

        // 3. Try OpenClaw auth-profiles
        if let Some(token) = Self::read_openclaw_auth()? {
            return Ok(token);
        }

        anyhow::bail!(
            "No Anthropic API key found.\n\
             Options:\n\
             1. Login to Claude Code: `claude login`\n\
             2. Set ANTHROPIC_API_KEY environment variable\n\
             3. Run `devman init`\n\
             4. If OpenClaw is installed, its OAuth token will be used automatically"
        )
    }

    /// Read OAuth access token from Claude Code CLI
    fn read_claude_code_oauth() -> Result<Option<String>> {
        let home = dirs::home_dir().unwrap_or_default();
        let path = home.join(".claude").join(".credentials.json");
        if !path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&path)
            .with_context(|| "reading Claude Code credentials")?;
        let creds: ClaudeCodeCredentials = serde_json::from_str(&content)
            .with_context(|| "parsing Claude Code credentials")?;

        if let Some(oauth) = creds.claude_ai_oauth {
            // Check if expired
            if let Some(expires_at) = oauth.expires_at {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;
                if now_ms > expires_at {
                    tracing::warn!("Claude Code OAuth token expired — run `claude login` to refresh");
                    return Ok(None);
                }
            }
            tracing::info!("Using Anthropic OAuth token from Claude Code CLI");
            return Ok(Some(oauth.access_token));
        }

        Ok(None)
    }

    /// Read OAuth token from OpenClaw's auth-profiles.json
    fn read_openclaw_auth() -> Result<Option<String>> {
        // Try common OpenClaw auth-profiles locations
        let candidates = [
            dirs::home_dir()
                .unwrap_or_default()
                .join(".openclaw/agents/main/agent/auth-profiles.json"),
        ];

        for path in &candidates {
            if !path.exists() {
                continue;
            }

            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let profiles: OpenClawAuthProfiles = match serde_json::from_str(&content) {
                Ok(p) => p,
                Err(_) => continue,
            };

            if let Some(profiles_map) = profiles.profiles {
                // Look for anthropic:default profile
                if let Some(profile) = profiles_map.get("anthropic:default") {
                    // Check provider
                    if profile.provider.as_deref() != Some("anthropic") {
                        continue;
                    }

                    // type: "token" → use token field
                    if profile.profile_type.as_deref() == Some("token") {
                        if let Some(ref token) = profile.token {
                            // Check expiry if present
                            if let Some(expires) = profile.expires {
                                let now_ms = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap()
                                    .as_millis() as u64;
                                if now_ms > expires {
                                    tracing::warn!("OpenClaw OAuth token expired");
                                    continue;
                                }
                            }
                            tracing::info!("Using Anthropic OAuth token from OpenClaw");
                            return Ok(Some(token.clone()));
                        }
                    }

                    // type: "oauth" → use access field
                    if profile.profile_type.as_deref() == Some("oauth") {
                        if let Some(ref access) = profile.access {
                            if let Some(expires) = profile.expires {
                                let now_ms = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap()
                                    .as_millis() as u64;
                                if now_ms > expires {
                                    tracing::warn!("OpenClaw OAuth token expired");
                                    continue;
                                }
                            }
                            tracing::info!("Using Anthropic OAuth token from OpenClaw");
                            return Ok(Some(access.clone()));
                        }
                    }
                }
            }
        }

        Ok(None)
    }

    pub fn telegram_bot_token(&self) -> Option<String> {
        std::env::var("TELEGRAM_BOT_TOKEN")
            .ok()
            .or_else(|| {
                self.credentials
                    .telegram
                    .as_ref()
                    .map(|t| t.bot_token.clone())
            })
    }

    pub fn brave_api_key(&self) -> Option<String> {
        std::env::var("BRAVE_API_KEY")
            .ok()
            .or_else(|| {
                self.credentials.brave.as_ref().map(|b| b.api_key.clone())
            })
    }

    pub fn elevenlabs_api_key(&self) -> Option<String> {
        std::env::var("ELEVENLABS_API_KEY")
            .ok()
            .or_else(|| {
                self.credentials
                    .elevenlabs
                    .as_ref()
                    .map(|e| e.api_key.clone())
            })
    }

    pub fn github_token(&self) -> Option<String> {
        std::env::var("GITHUB_TOKEN")
            .ok()
            .or_else(|| {
                self.credentials.github.as_ref().map(|g| g.token.clone())
            })
    }

    fn credentials_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("devman")
            .join("credentials.toml")
    }
}
