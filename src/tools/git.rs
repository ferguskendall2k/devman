use anyhow::Result;
use serde_json::json;
use tokio::process::Command;

use crate::types::ToolDefinition;

async fn run_git(args: &[&str], workdir: Option<&str>) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(dir) = workdir {
        cmd.current_dir(dir);
    }
    let output = cmd.output().await
        .map_err(|e| anyhow::anyhow!("failed to run git: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut result = String::new();
    if !stdout.is_empty() {
        result.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !result.is_empty() { result.push('\n'); }
        result.push_str(&stderr);
    }
    if !output.status.success() {
        anyhow::bail!("git {} failed (exit {}):\n{}", args.join(" "), output.status.code().unwrap_or(-1), result);
    }
    if result.is_empty() {
        result = "(no output)".into();
    }
    Ok(result)
}

// --- git_status ---

pub fn git_status_definition() -> ToolDefinition {
    ToolDefinition {
        name: "git_status".into(),
        description: "Show git status (porcelain) and recent commits.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Repository path (default: cwd)" }
            }
        }),
    }
}

pub async fn git_status_execute(input: &serde_json::Value) -> Result<String> {
    let path = input["path"].as_str();
    let status = run_git(&["status", "--porcelain"], path).await?;
    let log = run_git(&["log", "--oneline", "-5"], path).await.unwrap_or_else(|_| "(no commits)".into());
    Ok(format!("=== Status ===\n{status}\n=== Recent Commits ===\n{log}"))
}

// --- git_diff ---

pub fn git_diff_definition() -> ToolDefinition {
    ToolDefinition {
        name: "git_diff".into(),
        description: "Show git diff (working tree or staged).".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Repository path (default: cwd)" },
                "staged": { "type": "boolean", "description": "Show staged changes instead" }
            }
        }),
    }
}

pub async fn git_diff_execute(input: &serde_json::Value) -> Result<String> {
    let path = input["path"].as_str();
    let staged = input["staged"].as_bool().unwrap_or(false);
    let args = if staged { vec!["diff", "--staged"] } else { vec!["diff"] };
    run_git(&args, path).await
}

// --- git_commit ---

pub fn git_commit_definition() -> ToolDefinition {
    ToolDefinition {
        name: "git_commit".into(),
        description: "Stage all changes and commit with a message.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "message": { "type": "string", "description": "Commit message" },
                "path": { "type": "string", "description": "Repository path (default: cwd)" }
            },
            "required": ["message"]
        }),
    }
}

pub async fn git_commit_execute(input: &serde_json::Value) -> Result<String> {
    let message = input["message"].as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'message' field"))?;
    let path = input["path"].as_str();
    run_git(&["add", "-A"], path).await?;
    run_git(&["commit", "-m", message], path).await
}

// --- git_push ---

pub fn git_push_definition() -> ToolDefinition {
    ToolDefinition {
        name: "git_push".into(),
        description: "Push commits to remote.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Repository path (default: cwd)" },
                "remote": { "type": "string", "description": "Remote name (default: origin)" },
                "branch": { "type": "string", "description": "Branch to push" }
            }
        }),
    }
}

pub async fn git_push_execute(input: &serde_json::Value) -> Result<String> {
    let path = input["path"].as_str();
    let remote = input["remote"].as_str().unwrap_or("origin");
    let mut args = vec!["push", remote];
    if let Some(branch) = input["branch"].as_str() {
        args.push(branch);
    }
    run_git(&args, path).await
}

// --- git_log ---

pub fn git_log_definition() -> ToolDefinition {
    ToolDefinition {
        name: "git_log".into(),
        description: "Show git log (oneline format).".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Repository path (default: cwd)" },
                "count": { "type": "integer", "description": "Number of commits (default: 20)" }
            }
        }),
    }
}

pub async fn git_log_execute(input: &serde_json::Value) -> Result<String> {
    let path = input["path"].as_str();
    let count = input["count"].as_u64().unwrap_or(20);
    let count_str = format!("-{count}");
    run_git(&["log", "--oneline", &count_str], path).await
}

// --- git_branch ---

pub fn git_branch_definition() -> ToolDefinition {
    ToolDefinition {
        name: "git_branch".into(),
        description: "List branches or create/switch branch.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Repository path (default: cwd)" },
                "name": { "type": "string", "description": "Branch name to create or switch to" },
                "create": { "type": "boolean", "description": "Create new branch if true" }
            }
        }),
    }
}

pub async fn git_branch_execute(input: &serde_json::Value) -> Result<String> {
    let path = input["path"].as_str();
    match input["name"].as_str() {
        Some(name) => {
            let create = input["create"].as_bool().unwrap_or(false);
            if create {
                run_git(&["checkout", "-b", name], path).await
            } else {
                run_git(&["checkout", name], path).await
            }
        }
        None => run_git(&["branch", "-a"], path).await,
    }
}
