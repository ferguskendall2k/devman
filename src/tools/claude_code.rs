use anyhow::Result;
use serde_json::json;
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;

use crate::types::ToolDefinition;

/// Tool definition for claude_code
pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "claude_code".into(),
        description: "Delegate a software development task to Claude Code (claude CLI). \
            Use this for complex coding tasks: writing features, debugging, refactoring, \
            code reviews, test writing, and multi-file changes. Claude Code has full \
            filesystem and shell access and will autonomously edit/test/iterate. \
            Returns the final output text and a summary of files changed."
            .into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "The development task to perform. Be specific about what to build, fix, or change. Include file paths and context."
                },
                "working_directory": {
                    "type": "string",
                    "description": "Directory to run in (the project root). Required."
                },
                "model": {
                    "type": "string",
                    "description": "Model to use (e.g. 'sonnet', 'opus', 'claude-sonnet-4-20250514'). Default: sonnet"
                },
                "max_budget_usd": {
                    "type": "number",
                    "description": "Maximum dollar spend for this task. Default: 1.00"
                },
                "allowed_tools": {
                    "type": "string",
                    "description": "Comma-separated tool names to allow (e.g. 'Bash,Edit,Read,Write'). Default: all tools"
                },
                "timeout_seconds": {
                    "type": "integer",
                    "description": "Maximum seconds to run. Default: 300 (5 minutes)"
                }
            },
            "required": ["task", "working_directory"]
        }),
    }
}

/// Execute a claude_code tool call
pub async fn execute(input: &serde_json::Value) -> Result<String> {
    let task = input["task"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'task' parameter"))?;

    let working_dir = input["working_directory"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'working_directory' parameter"))?;

    // Validate working directory exists
    if !Path::new(working_dir).is_dir() {
        anyhow::bail!("working_directory does not exist: {working_dir}");
    }

    let model = input["model"].as_str().unwrap_or("sonnet");
    let max_budget = input["max_budget_usd"].as_f64().unwrap_or(1.0);
    let timeout_secs = input["timeout_seconds"].as_u64().unwrap_or(300);
    let allowed_tools = input["allowed_tools"].as_str();

    // Find claude binary
    let claude_bin = find_claude_binary()?;

    // Build command
    let mut cmd = Command::new(&claude_bin);
    cmd.arg("--print")
        .arg("--output-format").arg("json")
        .arg("--model").arg(model)
        .arg("--max-budget-usd").arg(format!("{:.2}", max_budget))
        .arg("--dangerously-skip-permissions") // running in trusted context
        .arg("--no-session-persistence")       // don't clutter session list
        .current_dir(working_dir);

    if let Some(tools) = allowed_tools {
        cmd.arg("--allowed-tools").arg(tools);
    }

    cmd.arg(task);

    // Run with timeout
    let output = tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        cmd.output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!(
        "Claude Code timed out after {timeout_secs}s. The task may be too large — try breaking it down."
    ))??;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        return Ok(format!(
            "Claude Code exited with code {code}\n\nSTDERR:\n{stderr}\n\nSTDOUT:\n{stdout}"
        ));
    }

    // Parse JSON output for structured result
    let result = parse_claude_output(&stdout, &stderr);
    Ok(result)
}

/// Parse Claude Code JSON output into a readable summary
fn parse_claude_output(stdout: &str, stderr: &str) -> String {
    // Try to parse as JSON (--output-format json)
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(stdout) {
        let mut parts = Vec::new();

        // Extract the result text
        if let Some(result) = parsed["result"].as_str() {
            parts.push(format!("## Result\n{result}"));
        }

        // Extract cost info
        if let Some(cost) = parsed["cost_usd"].as_f64() {
            parts.push(format!("**Cost:** ${:.4}", cost));
        }

        // Extract token usage
        if let Some(usage) = parsed.get("usage") {
            let input = usage["input_tokens"].as_u64().unwrap_or(0);
            let output = usage["output_tokens"].as_u64().unwrap_or(0);
            if input > 0 || output > 0 {
                parts.push(format!("**Tokens:** {input} in / {output} out"));
            }
        }

        // Extract duration
        if let Some(duration) = parsed["duration_ms"].as_u64() {
            parts.push(format!("**Duration:** {:.1}s", duration as f64 / 1000.0));
        }

        // Extract num turns
        if let Some(turns) = parsed["num_turns"].as_u64() {
            parts.push(format!("**Turns:** {turns}"));
        }

        if !parts.is_empty() {
            return parts.join("\n");
        }
    }

    // Fallback: return raw output
    let mut result = stdout.to_string();
    if !stderr.is_empty() {
        result.push_str(&format!("\n\n--- stderr ---\n{stderr}"));
    }
    result
}

/// Find the claude binary, checking common locations
fn find_claude_binary() -> Result<String> {
    // Check common locations
    let candidates = [
        "claude",
        "/home/fergus/.local/bin/claude",
        "/usr/local/bin/claude",
    ];

    for candidate in &candidates {
        if let Ok(output) = std::process::Command::new(candidate)
            .arg("--version")
            .output()
        {
            if output.status.success() {
                return Ok(candidate.to_string());
            }
        }
    }

    anyhow::bail!(
        "Claude Code CLI not found. Install it with: npm install -g @anthropic-ai/claude-code"
    )
}

/// Run a development task via Claude Code and return structured output.
/// This is the high-level function used by the orchestrator for dev bot spawns.
pub async fn run_dev_task(
    task: &str,
    working_dir: &str,
    model: &str,
    max_budget_usd: f64,
    timeout_seconds: u64,
) -> Result<DevTaskResult> {
    let input = json!({
        "task": task,
        "working_directory": working_dir,
        "model": model,
        "max_budget_usd": max_budget_usd,
        "timeout_seconds": timeout_seconds,
    });

    let output = execute(&input).await?;

    Ok(DevTaskResult {
        output,
        model: model.to_string(),
    })
}

#[derive(Debug)]
pub struct DevTaskResult {
    pub output: String,
    pub model: String,
}
