use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level configuration (from config.toml + CLI args)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub models: ModelConfig,
    pub tools: ToolsConfig,
    pub agents: AgentPoolConfig,
    pub telegram: Option<TelegramConfig>,
    pub brave: Option<BraveConfig>,
    pub elevenlabs: Option<ElevenLabsConfig>,
    pub github: Option<GitHubConfig>,
    pub secrets: SecretsConfig,
    pub vault: VaultConfig,
    pub logging: LoggingConfig,
    pub dashboard: DashboardConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            models: ModelConfig::default(),
            tools: ToolsConfig::default(),
            agents: AgentPoolConfig::default(),
            telegram: None,
            brave: None,
            elevenlabs: None,
            github: None,
            secrets: SecretsConfig::default(),
            vault: VaultConfig::default(),
            logging: LoggingConfig::default(),
            dashboard: DashboardConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelConfig {
    pub manager: String,
    pub quick: String,
    pub standard: String,
    pub complex: String,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            manager: "claude-haiku-4-5-20250512".into(),
            quick: "claude-haiku-4-5-20250512".into(),
            standard: "claude-sonnet-4-20250514".into(),
            complex: "claude-opus-4-20250414".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolsConfig {
    pub shell_confirm: bool,
    pub web_enabled: bool,
    pub custom: Vec<CustomToolConfig>,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            shell_confirm: false,
            web_enabled: true,
            custom: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomToolConfig {
    pub name: String,
    pub description: String,
    pub command: Vec<String>,
    pub input_schema: String,
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentPoolConfig {
    pub max_concurrent: u32,
    pub max_turns: u32,
    pub max_tokens: u32,
    pub recovery: String,
    pub checkpoint_interval: u32,
}

impl Default for AgentPoolConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 5,
            max_turns: 50,
            max_tokens: 16384,
            recovery: "report".into(),
            checkpoint_interval: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub bot_token: Option<String>,
    pub allowed_users: Vec<i64>,
    /// Scoped bots — each bound to specific tasks
    #[serde(default)]
    pub bots: Vec<ScopedBotConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopedBotConfig {
    pub name: String,
    pub bot_token: String,
    pub allowed_users: Vec<i64>,
    /// Task slugs this bot can access. ["*"] = all tasks.
    pub tasks: Vec<String>,
    /// Optional system prompt override
    pub system_prompt: Option<String>,
    /// Optional system prompt file path
    pub system_prompt_file: Option<String>,
    /// Model tier: "quick", "standard", "complex" (default: "standard")
    #[serde(default = "default_model_tier")]
    pub default_model: String,
    /// "scoped" (default) or "full"
    #[serde(default = "default_memory_access")]
    pub memory_access: String,
    /// Max output tokens for this bot (default: 4096 — good for Telegram)
    #[serde(default = "default_bot_max_tokens")]
    pub max_tokens: u32,
    /// Max conversation turns before auto-compaction (default: 20)
    #[serde(default = "default_bot_max_turns")]
    pub max_turns: u32,
}

fn default_bot_max_tokens() -> u32 {
    4096
}

fn default_bot_max_turns() -> u32 {
    20
}

fn default_model_tier() -> String {
    "standard".into()
}

fn default_memory_access() -> String {
    "scoped".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BraveConfig {
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElevenLabsConfig {
    pub api_key: String,
    pub voice_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubConfig {
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SecretsConfig {
    pub backend: String,
}

impl Default for SecretsConfig {
    fn default() -> Self {
        Self {
            backend: "auto".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VaultConfig {
    pub enabled: bool,
    pub telegram_auto_delete_seconds: u32,
    pub telegram_spoiler: bool,
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            telegram_auto_delete_seconds: 60,
            telegram_spoiler: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    pub level: String,
    pub file: Option<PathBuf>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".into(),
            file: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DashboardConfig {
    pub enabled: bool,
    pub port: u16,
    pub bind: String,
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 18790,
            bind: "127.0.0.1".into(),
        }
    }
}

impl Config {
    /// Load config from default path (~/.config/devman/config.toml)
    pub fn load() -> Result<Self> {
        let config_path = Self::default_path();
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .with_context(|| format!("reading config from {}", config_path.display()))?;
            toml::from_str(&content).with_context(|| "parsing config.toml")
        } else {
            Ok(Self::default())
        }
    }

    /// Load config from a specific path
    pub fn load_from(path: &PathBuf) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("reading config from {}", path.display()))?;
        toml::from_str(&content).with_context(|| "parsing config file")
    }

    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("devman")
            .join("config.toml")
    }

    /// Save config to default path
    pub fn save(&self) -> Result<()> {
        let path = Self::default_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }
}
