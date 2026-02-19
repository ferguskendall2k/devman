use anyhow::Result;
use serde_json::json;

use crate::types::ToolDefinition;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "web_search".into(),
        description: "Search the web using Brave Search API.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "count": {
                    "type": "integer",
                    "description": "Number of results (1-10, default: 5)"
                }
            },
            "required": ["query"]
        }),
    }
}

pub async fn execute(input: &serde_json::Value, api_key: Option<&str>) -> Result<String> {
    let query = input["query"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'query' field"))?;
    let count = input["count"].as_u64().unwrap_or(5).min(10);

    let api_key = api_key.ok_or_else(|| anyhow::anyhow!("Brave Search API key not configured"))?;

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
                .map(|r| {
                    format!(
                        "**{}**\n{}\n{}\n",
                        r["title"].as_str().unwrap_or(""),
                        r["url"].as_str().unwrap_or(""),
                        r["description"].as_str().unwrap_or("")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_else(|| "No results found.".into());

    Ok(results)
}
