use anyhow::Result;
use serde_json::json;
use std::time::Duration;
use tokio::process::Command;

use crate::types::ToolDefinition;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "shell".into(),
        description: "Execute a shell command. Returns stdout and stderr.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to execute"
                },
                "workdir": {
                    "type": "string",
                    "description": "Working directory (optional)"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in seconds (default: 120)"
                }
            },
            "required": ["command"]
        }),
    }
}

pub async fn execute(input: &serde_json::Value) -> Result<String> {
    let command = input["command"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'command' field"))?;

    let timeout_secs = input["timeout"].as_u64().unwrap_or(120);

    let mut cmd = Command::new("bash");
    cmd.arg("-c").arg(command);

    if let Some(workdir) = input["workdir"].as_str() {
        cmd.current_dir(workdir);
    }

    let output = tokio::time::timeout(Duration::from_secs(timeout_secs), cmd.output())
        .await
        .map_err(|_| anyhow::anyhow!("command timed out after {timeout_secs}s"))?
        .map_err(|e| anyhow::anyhow!("failed to execute command: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut result = String::new();
    if !stdout.is_empty() {
        result.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str("STDERR:\n");
        result.push_str(&stderr);
    }

    if !output.status.success() {
        result.push_str(&format!("\nExit code: {}", output.status.code().unwrap_or(-1)));
    }

    if result.is_empty() {
        result = "(no output)".into();
    }

    // Truncate very long output
    if result.len() > 50_000 {
        result.truncate(50_000);
        result.push_str("\n... (output truncated at 50KB)");
    }

    Ok(result)
}
