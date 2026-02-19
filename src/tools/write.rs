use anyhow::Result;
use serde_json::json;
use std::fs;

use crate::types::ToolDefinition;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "write_file".into(),
        description: "Write content to a file. Creates parent directories if needed.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write"
                }
            },
            "required": ["path", "content"]
        }),
    }
}

pub async fn execute(input: &serde_json::Value) -> Result<String> {
    let path = input["path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'path' field"))?;
    let content = input["content"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'content' field"))?;

    let expanded = if path.starts_with("~/") {
        let home = dirs::home_dir().unwrap_or_default();
        home.join(&path[2..])
    } else {
        std::path::PathBuf::from(path)
    };

    if let Some(parent) = expanded.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(&expanded, content)?;
    Ok(format!("Wrote {} bytes to {}", content.len(), expanded.display()))
}
