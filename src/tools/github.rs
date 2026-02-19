use anyhow::Result;
use serde_json::json;

use crate::types::ToolDefinition;

async fn github_api(
    method: &str,
    path: &str,
    token: &str,
    body: Option<&serde_json::Value>,
) -> Result<serde_json::Value> {
    let client = reqwest::Client::new();
    let url = format!("https://api.github.com{}", path);
    let mut req = match method {
        "POST" => client.post(&url),
        "PATCH" => client.patch(&url),
        "PUT" => client.put(&url),
        "DELETE" => client.delete(&url),
        _ => client.get(&url),
    };
    req = req
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "DevMan/0.1")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(b) = body {
        req = req.json(b);
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("GitHub API {status}: {text}");
    }
    let text = resp.text().await?;
    if text.is_empty() {
        Ok(json!({"status": "ok"}))
    } else {
        Ok(serde_json::from_str(&text)?)
    }
}

fn detect_repo() -> Option<(String, String)> {
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // Handle SSH: git@github.com:owner/repo.git
    // Handle HTTPS: https://github.com/owner/repo.git
    let path = if let Some(rest) = url.strip_prefix("git@github.com:") {
        rest.to_string()
    } else if url.contains("github.com/") {
        url.split("github.com/").last()?.to_string()
    } else {
        return None;
    };
    let path = path.trim_end_matches(".git");
    let parts: Vec<&str> = path.splitn(2, '/').collect();
    if parts.len() == 2 {
        Some((parts[0].to_string(), parts[1].to_string()))
    } else {
        None
    }
}

fn parse_repo(input: &serde_json::Value) -> Result<(String, String)> {
    if let Some(repo) = input["repo"].as_str() {
        let parts: Vec<&str> = repo.splitn(2, '/').collect();
        if parts.len() == 2 {
            return Ok((parts[0].to_string(), parts[1].to_string()));
        }
        anyhow::bail!("repo must be in 'owner/repo' format");
    }
    detect_repo().ok_or_else(|| anyhow::anyhow!("Could not detect repo from git remote. Provide 'repo' parameter."))
}

fn require_token(token: Option<&str>) -> Result<&str> {
    token.ok_or_else(|| anyhow::anyhow!("GitHub token not configured. Set GITHUB_TOKEN or add [github] to credentials.toml."))
}

fn current_branch() -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

// --- github_pr_create ---

pub fn github_pr_create_definition() -> ToolDefinition {
    ToolDefinition {
        name: "github_pr_create".into(),
        description: "Create a GitHub pull request.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "description": "PR title" },
                "head": { "type": "string", "description": "Head branch name" },
                "base": { "type": "string", "description": "Base branch (default: main)" },
                "body": { "type": "string", "description": "PR description" },
                "repo": { "type": "string", "description": "owner/repo (auto-detected from git remote)" }
            },
            "required": ["title", "head"]
        }),
    }
}

pub async fn github_pr_create_execute(input: &serde_json::Value, token: Option<&str>) -> Result<String> {
    let token = require_token(token)?;
    let (owner, repo) = parse_repo(input)?;
    let title = input["title"].as_str().ok_or_else(|| anyhow::anyhow!("missing 'title'"))?;
    let head = input["head"].as_str().ok_or_else(|| anyhow::anyhow!("missing 'head'"))?;
    let base = input["base"].as_str().unwrap_or("main");

    let mut body_json = json!({
        "title": title,
        "head": head,
        "base": base,
    });
    if let Some(b) = input["body"].as_str() {
        body_json["body"] = json!(b);
    }

    let result = github_api("POST", &format!("/repos/{owner}/{repo}/pulls"), token, Some(&body_json)).await?;
    let number = result["number"].as_u64().unwrap_or(0);
    let url = result["html_url"].as_str().unwrap_or("unknown");
    Ok(format!("Created PR #{number}: {url}"))
}

// --- github_pr_list ---

pub fn github_pr_list_definition() -> ToolDefinition {
    ToolDefinition {
        name: "github_pr_list".into(),
        description: "List pull requests on a GitHub repository.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "repo": { "type": "string", "description": "owner/repo (auto-detected from git remote)" },
                "state": { "type": "string", "description": "PR state: open, closed, all (default: open)" }
            }
        }),
    }
}

pub async fn github_pr_list_execute(input: &serde_json::Value, token: Option<&str>) -> Result<String> {
    let token = require_token(token)?;
    let (owner, repo) = parse_repo(input)?;
    let state = input["state"].as_str().unwrap_or("open");

    let result = github_api("GET", &format!("/repos/{owner}/{repo}/pulls?state={state}&per_page=30"), token, None).await?;
    let prs = result.as_array().ok_or_else(|| anyhow::anyhow!("unexpected response"))?;
    if prs.is_empty() {
        return Ok(format!("No {state} pull requests in {owner}/{repo}."));
    }
    let mut lines = vec![format!("{state} PRs in {owner}/{repo}:")];
    for pr in prs {
        let number = pr["number"].as_u64().unwrap_or(0);
        let title = pr["title"].as_str().unwrap_or("(no title)");
        let author = pr["user"]["login"].as_str().unwrap_or("unknown");
        lines.push(format!("  #{number} {title} (@{author})"));
    }
    Ok(lines.join("\n"))
}

// --- github_issues_list ---

pub fn github_issues_list_definition() -> ToolDefinition {
    ToolDefinition {
        name: "github_issues_list".into(),
        description: "List issues on a GitHub repository.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "repo": { "type": "string", "description": "owner/repo (auto-detected from git remote)" },
                "state": { "type": "string", "description": "Issue state: open, closed, all (default: open)" },
                "labels": { "type": "string", "description": "Comma-separated label filter" }
            }
        }),
    }
}

pub async fn github_issues_list_execute(input: &serde_json::Value, token: Option<&str>) -> Result<String> {
    let token = require_token(token)?;
    let (owner, repo) = parse_repo(input)?;
    let state = input["state"].as_str().unwrap_or("open");
    let mut query = format!("state={state}&per_page=30");
    if let Some(labels) = input["labels"].as_str() {
        query.push_str(&format!("&labels={labels}"));
    }

    let result = github_api("GET", &format!("/repos/{owner}/{repo}/issues?{query}"), token, None).await?;
    let issues = result.as_array().ok_or_else(|| anyhow::anyhow!("unexpected response"))?;
    // Filter out pull requests (GitHub returns PRs in issues endpoint)
    let issues: Vec<_> = issues.iter().filter(|i| i.get("pull_request").is_none()).collect();
    if issues.is_empty() {
        return Ok(format!("No {state} issues in {owner}/{repo}."));
    }
    let mut lines = vec![format!("{state} issues in {owner}/{repo}:")];
    for issue in &issues {
        let number = issue["number"].as_u64().unwrap_or(0);
        let title = issue["title"].as_str().unwrap_or("(no title)");
        let author = issue["user"]["login"].as_str().unwrap_or("unknown");
        let labels: Vec<&str> = issue["labels"].as_array()
            .map(|arr| arr.iter().filter_map(|l| l["name"].as_str()).collect())
            .unwrap_or_default();
        let label_str = if labels.is_empty() { String::new() } else { format!(" [{}]", labels.join(", ")) };
        lines.push(format!("  #{number} {title} (@{author}){label_str}"));
    }
    Ok(lines.join("\n"))
}

// --- github_issue_create ---

pub fn github_issue_create_definition() -> ToolDefinition {
    ToolDefinition {
        name: "github_issue_create".into(),
        description: "Create a GitHub issue.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "description": "Issue title" },
                "body": { "type": "string", "description": "Issue body" },
                "labels": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Labels to apply"
                },
                "repo": { "type": "string", "description": "owner/repo (auto-detected from git remote)" }
            },
            "required": ["title"]
        }),
    }
}

pub async fn github_issue_create_execute(input: &serde_json::Value, token: Option<&str>) -> Result<String> {
    let token = require_token(token)?;
    let (owner, repo) = parse_repo(input)?;
    let title = input["title"].as_str().ok_or_else(|| anyhow::anyhow!("missing 'title'"))?;

    let mut body_json = json!({ "title": title });
    if let Some(b) = input["body"].as_str() {
        body_json["body"] = json!(b);
    }
    if let Some(labels) = input["labels"].as_array() {
        body_json["labels"] = json!(labels);
    }

    let result = github_api("POST", &format!("/repos/{owner}/{repo}/issues"), token, Some(&body_json)).await?;
    let number = result["number"].as_u64().unwrap_or(0);
    let url = result["html_url"].as_str().unwrap_or("unknown");
    Ok(format!("Created issue #{number}: {url}"))
}

// --- github_actions_status ---

pub fn github_actions_status_definition() -> ToolDefinition {
    ToolDefinition {
        name: "github_actions_status".into(),
        description: "Check GitHub Actions CI/CD workflow run status.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "repo": { "type": "string", "description": "owner/repo (auto-detected from git remote)" },
                "branch": { "type": "string", "description": "Branch to check (default: current branch)" }
            }
        }),
    }
}

pub async fn github_actions_status_execute(input: &serde_json::Value, token: Option<&str>) -> Result<String> {
    let token = require_token(token)?;
    let (owner, repo) = parse_repo(input)?;
    let branch = input["branch"].as_str()
        .map(|s| s.to_string())
        .or_else(current_branch)
        .unwrap_or_else(|| "main".to_string());

    let result = github_api(
        "GET",
        &format!("/repos/{owner}/{repo}/actions/runs?branch={branch}&per_page=5"),
        token,
        None,
    ).await?;

    let runs = result["workflow_runs"].as_array()
        .ok_or_else(|| anyhow::anyhow!("unexpected response"))?;
    if runs.is_empty() {
        return Ok(format!("No workflow runs found for branch '{branch}' in {owner}/{repo}."));
    }
    let mut lines = vec![format!("Recent workflow runs ({owner}/{repo}, branch: {branch}):")];
    for run in runs {
        let name = run["name"].as_str().unwrap_or("unknown");
        let status = run["status"].as_str().unwrap_or("unknown");
        let conclusion = run["conclusion"].as_str().unwrap_or("pending");
        let run_number = run["run_number"].as_u64().unwrap_or(0);
        let display = if status == "completed" { conclusion } else { status };
        let emoji = match display {
            "success" => "✅",
            "failure" => "❌",
            "cancelled" => "⏹️",
            "in_progress" => "🔄",
            "queued" => "⏳",
            _ => "❓",
        };
        lines.push(format!("  {emoji} #{run_number} {name}: {display}"));
    }
    Ok(lines.join("\n"))
}
