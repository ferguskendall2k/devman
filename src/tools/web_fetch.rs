use anyhow::Result;
use serde_json::json;

use crate::types::ToolDefinition;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "web_fetch".into(),
        description: "Fetch a URL and extract readable text content.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL to fetch"
                },
                "max_chars": {
                    "type": "integer",
                    "description": "Maximum characters to return (default: 20000)"
                }
            },
            "required": ["url"]
        }),
    }
}

pub async fn execute(input: &serde_json::Value) -> Result<String> {
    let url = input["url"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'url' field"))?;
    let max_chars = input["max_chars"].as_u64().unwrap_or(20_000) as usize;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("DevMan/0.1")
        .build()?;

    let resp = client.get(url).send().await?;

    if !resp.status().is_success() {
        anyhow::bail!("HTTP {}", resp.status());
    }

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let body = resp.text().await?;

    // Simple HTML to text extraction (strip tags)
    let text = if content_type.contains("html") {
        strip_html_tags(&body)
    } else {
        body
    };

    let mut result = text;
    if result.len() > max_chars {
        result.truncate(max_chars);
        result.push_str("\n... (truncated)");
    }

    Ok(result)
}

/// Very basic HTML tag stripping — good enough for readable extraction
fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;
    let mut last_was_space = false;

    let lower = html.to_lowercase();
    let chars: Vec<char> = html.chars().collect();
    let lower_chars: Vec<char> = lower.chars().collect();

    let mut i = 0;
    while i < chars.len() {
        if !in_tag && chars[i] == '<' {
            in_tag = true;
            // Check for script/style start/end
            let remaining: String = lower_chars[i..].iter().take(20).collect();
            if remaining.starts_with("<script") {
                in_script = true;
            } else if remaining.starts_with("</script") {
                in_script = false;
            } else if remaining.starts_with("<style") {
                in_style = true;
            } else if remaining.starts_with("</style") {
                in_style = false;
            }
            // Block elements get newlines
            if remaining.starts_with("<br")
                || remaining.starts_with("<p")
                || remaining.starts_with("<div")
                || remaining.starts_with("<h")
                || remaining.starts_with("<li")
                || remaining.starts_with("<tr")
            {
                if !result.ends_with('\n') {
                    result.push('\n');
                }
            }
            i += 1;
            continue;
        }

        if in_tag {
            if chars[i] == '>' {
                in_tag = false;
            }
            i += 1;
            continue;
        }

        if in_script || in_style {
            i += 1;
            continue;
        }

        // Decode common entities
        if chars[i] == '&' {
            let remaining: String = chars[i..].iter().take(10).collect();
            if remaining.starts_with("&amp;") {
                result.push('&');
                i += 5;
                last_was_space = false;
                continue;
            } else if remaining.starts_with("&lt;") {
                result.push('<');
                i += 4;
                last_was_space = false;
                continue;
            } else if remaining.starts_with("&gt;") {
                result.push('>');
                i += 4;
                last_was_space = false;
                continue;
            } else if remaining.starts_with("&nbsp;") {
                result.push(' ');
                i += 6;
                last_was_space = true;
                continue;
            } else if remaining.starts_with("&quot;") {
                result.push('"');
                i += 6;
                last_was_space = false;
                continue;
            }
        }

        // Collapse whitespace
        if chars[i].is_whitespace() {
            if !last_was_space {
                result.push(' ');
                last_was_space = true;
            }
        } else {
            result.push(chars[i]);
            last_was_space = false;
        }
        i += 1;
    }

    // Clean up excessive blank lines
    let mut cleaned = String::new();
    let mut blank_count = 0;
    for line in result.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            blank_count += 1;
            if blank_count <= 2 {
                cleaned.push('\n');
            }
        } else {
            blank_count = 0;
            cleaned.push_str(trimmed);
            cleaned.push('\n');
        }
    }

    cleaned
}
