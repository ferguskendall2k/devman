use anyhow::Result;
use serde_json::json;
use std::fs;

use crate::types::ToolDefinition;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "read_file".into(),
        description: "Read the contents of a file. Supports offset/limit for large files.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read"
                },
                "offset": {
                    "type": "integer",
                    "description": "Line number to start reading from (1-indexed)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to read"
                }
            },
            "required": ["path"]
        }),
    }
}

pub async fn execute(input: &serde_json::Value) -> Result<String> {
    let path = input["path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'path' field"))?;

    // Expand ~ to home dir
    let expanded = if path.starts_with("~/") {
        let home = dirs::home_dir().unwrap_or_default();
        home.join(&path[2..])
    } else {
        std::path::PathBuf::from(path)
    };

    let content = fs::read_to_string(&expanded)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", expanded.display()))?;

    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    let offset = input["offset"].as_u64().unwrap_or(1).max(1) as usize - 1;
    let limit = input["limit"].as_u64().map(|l| l as usize);

    let end = match limit {
        Some(l) => (offset + l).min(total_lines),
        None => total_lines.min(offset + 2000), // default max 2000 lines
    };

    if offset >= total_lines {
        return Ok(format!("(file has {total_lines} lines, offset {offset} is past end)"));
    }

    let selected: Vec<&str> = lines[offset..end].to_vec();
    let mut result = selected.join("\n");

    if end < total_lines {
        result.push_str(&format!(
            "\n\n[{} more lines. Use offset={} to continue.]",
            total_lines - end,
            end + 1
        ));
    }

    // Truncate at 50KB
    if result.len() > 50_000 {
        result.truncate(50_000);
        result.push_str("\n... (truncated at 50KB)");
    }

    Ok(result)
}
