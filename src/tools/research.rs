use anyhow::Result;
use serde_json::json;
use std::collections::HashSet;

use crate::types::ToolDefinition;

struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

struct Source {
    title: String,
    url: String,
    content: String,
}

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "deep_research".into(),
        description: "Run autonomous multi-step web research. Searches multiple queries, reads pages, and produces a synthesised report with citations.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The research question"
                },
                "depth": {
                    "type": "string",
                    "description": "Research depth: quick, standard, or thorough (default: standard)",
                    "enum": ["quick", "standard", "thorough"]
                },
                "save_to": {
                    "type": "string",
                    "description": "File path to save the report (optional)"
                }
            },
            "required": ["question"]
        }),
    }
}

pub async fn execute(input: &serde_json::Value, brave_api_key: Option<&str>) -> Result<String> {
    let question = input["question"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'question' field"))?;
    let depth = input["depth"].as_str().unwrap_or("standard");

    let (max_searches, max_reads) = match depth {
        "quick" => (2, 3),
        "thorough" => (5, 10),
        _ => (3, 5),
    };

    // Step 1: Generate sub-queries
    let queries = generate_sub_queries(question, max_searches);

    // Step 2: Search each query
    let mut all_results = Vec::new();
    for query in &queries {
        match brave_search(query, brave_api_key, 5).await {
            Ok(results) => all_results.extend(results),
            Err(e) => eprintln!("Search error for '{}': {}", query, e),
        }
    }

    if all_results.is_empty() {
        return Ok(format!("# Research: {}\n\nNo search results found. Check that the Brave API key is configured.", question));
    }

    // Step 3: Deduplicate by URL
    deduplicate_and_rank(&mut all_results);
    let top_results = &all_results[..all_results.len().min(max_reads)];

    // Step 4: Fetch content
    let mut sources = Vec::new();
    for result in top_results {
        match fetch_and_extract(&result.url, 5000).await {
            Ok(content) => sources.push(Source {
                title: result.title.clone(),
                url: result.url.clone(),
                content,
            }),
            Err(_) => {
                // Use snippet as fallback
                if !result.snippet.is_empty() {
                    sources.push(Source {
                        title: result.title.clone(),
                        url: result.url.clone(),
                        content: result.snippet.clone(),
                    });
                }
            }
        }
    }

    // Step 5: Format report
    let report = format_report(question, &sources);

    // Step 6: Optionally save
    if let Some(path) = input["save_to"].as_str() {
        std::fs::write(path, &report)?;
    }

    Ok(report)
}

fn generate_sub_queries(question: &str, count: usize) -> Vec<String> {
    let mut queries = vec![question.to_string()];

    if count >= 2 {
        queries.push(format!("{} best practices", question));
    }
    if count >= 3 {
        queries.push(format!("{} comparison alternatives", question));
    }
    if count >= 4 {
        queries.push(format!("{} tutorial guide", question));
    }
    if count >= 5 {
        queries.push(format!("{} 2025 2026 latest", question));
    }

    queries.truncate(count);
    queries
}

async fn brave_search(
    query: &str,
    api_key: Option<&str>,
    count: u64,
) -> Result<Vec<SearchResult>> {
    let api_key =
        api_key.ok_or_else(|| anyhow::anyhow!("Brave Search API key not configured"))?;

    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.search.brave.com/res/v1/web/search")
        .header("X-Subscription-Token", api_key)
        .query(&[("q", query), ("count", &count.to_string())])
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Brave Search error {status}: {body}");
    }

    let data: serde_json::Value = resp.json().await?;
    let results = data["web"]["results"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|r| SearchResult {
                    title: r["title"].as_str().unwrap_or("").to_string(),
                    url: r["url"].as_str().unwrap_or("").to_string(),
                    snippet: r["description"].as_str().unwrap_or("").to_string(),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(results)
}

async fn fetch_and_extract(url: &str, max_chars: usize) -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
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

    let text = if content_type.contains("html") {
        strip_html_basic(&body)
    } else {
        body
    };

    let mut result = text;
    if result.len() > max_chars {
        result.truncate(max_chars);
    }
    Ok(result)
}

/// Minimal HTML stripping for research extraction
fn strip_html_basic(html: &str) -> String {
    let mut result = String::with_capacity(html.len() / 3);
    let mut in_tag = false;
    let mut in_script = false;
    let mut last_was_space = false;

    let lower = html.to_lowercase();
    let chars: Vec<char> = html.chars().collect();
    let lower_chars: Vec<char> = lower.chars().collect();

    let mut i = 0;
    while i < chars.len() {
        if !in_tag && chars[i] == '<' {
            in_tag = true;
            let remaining: String = lower_chars[i..].iter().take(20).collect();
            if remaining.starts_with("<script") || remaining.starts_with("<style") {
                in_script = true;
            } else if remaining.starts_with("</script") || remaining.starts_with("</style") {
                in_script = false;
            }
            if remaining.starts_with("<br")
                || remaining.starts_with("<p")
                || remaining.starts_with("<div")
                || remaining.starts_with("<h")
                || remaining.starts_with("<li")
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
        if in_script {
            i += 1;
            continue;
        }
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
    result
}

fn deduplicate_and_rank(results: &mut Vec<SearchResult>) {
    let mut seen = HashSet::new();
    results.retain(|r| {
        if r.url.is_empty() {
            return false;
        }
        seen.insert(r.url.clone())
    });
}

fn format_report(question: &str, sources: &[Source]) -> String {
    let mut report = String::new();

    report.push_str(&format!("# Research Report: {}\n\n", question));

    // Summary section with snippets from sources
    report.push_str("## Summary\n\n");
    if sources.is_empty() {
        report.push_str("No sources could be retrieved for this question.\n\n");
    } else {
        report.push_str(&format!(
            "Research on \"{}\" gathered information from {} sources.\n\n",
            question,
            sources.len()
        ));
    }

    // Key findings with citations
    report.push_str("## Key Findings\n\n");
    for (i, source) in sources.iter().enumerate() {
        let idx = i + 1;
        // Take first ~500 chars as a finding excerpt
        let excerpt = if source.content.len() > 500 {
            &source.content[..500]
        } else {
            &source.content
        };
        // Clean up the excerpt
        let excerpt = excerpt.trim().replace('\n', " ");
        report.push_str(&format!(
            "### From: {} [{}]\n\n{}\n\n",
            source.title, idx, excerpt
        ));
    }

    // Sources list
    report.push_str("## Sources\n\n");
    for (i, source) in sources.iter().enumerate() {
        report.push_str(&format!("[{}] {} — {}\n", i + 1, source.title, source.url));
    }

    report
}
