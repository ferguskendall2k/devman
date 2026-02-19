use crate::config::Config;
use crate::types::ToolDefinition;
use anyhow::{Context, Result};
use serde_json;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// A custom user-defined tool loaded from config
#[derive(Debug, Clone)]
pub struct CustomTool {
    pub name: String,
    pub description: String,
    pub command: Vec<String>,
    pub input_schema: serde_json::Value,
    pub timeout_secs: u64,
}

impl CustomTool {
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name.clone(),
            description: self.description.clone(),
            input_schema: self.input_schema.clone(),
        }
    }

    pub async fn execute(&self, input: &serde_json::Value) -> Result<String> {
        let input_json = serde_json::to_string(input)?;

        let program = self
            .command
            .first()
            .context("custom tool command is empty")?;
        let args = &self.command[1..];

        let mut child = Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .with_context(|| format!("spawning custom tool '{}'", self.name))?;

        // Write input JSON to stdin
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input_json.as_bytes()).await?;
            stdin.shutdown().await?;
        }

        // Apply timeout
        let output = tokio::time::timeout(
            Duration::from_secs(self.timeout_secs),
            child.wait_with_output(),
        )
        .await
        .with_context(|| format!("custom tool '{}' timed out after {}s", self.name, self.timeout_secs))?
        .with_context(|| format!("running custom tool '{}'", self.name))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "custom tool '{}' exited with {}: {}",
                self.name,
                output.status,
                if stderr.is_empty() { &stdout } else { stderr.as_ref() }
            );
        }

        // Try to parse as JSON with content/is_error fields
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&stdout) {
            if let Some(content) = parsed.get("content").and_then(|c| c.as_str()) {
                let is_error = parsed
                    .get("is_error")
                    .and_then(|e| e.as_bool())
                    .unwrap_or(false);
                if is_error {
                    anyhow::bail!("{}", content);
                }
                return Ok(content.to_string());
            }
        }

        // Return raw stdout
        Ok(stdout)
    }
}

/// Load custom tools from config
pub fn load_custom_tools(config: &Config) -> Vec<CustomTool> {
    config
        .tools
        .custom
        .iter()
        .filter_map(|ct| {
            let input_schema = serde_json::from_str(&ct.input_schema)
                .map_err(|e| {
                    eprintln!(
                        "Warning: invalid input_schema for custom tool '{}': {}",
                        ct.name, e
                    );
                    e
                })
                .ok()?;
            Some(CustomTool {
                name: ct.name.clone(),
                description: ct.description.clone(),
                command: ct.command.clone(),
                input_schema,
                timeout_secs: ct.timeout.unwrap_or(30),
            })
        })
        .collect()
}
