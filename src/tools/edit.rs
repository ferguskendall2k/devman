use anyhow::Result;
use serde_json::json;
use std::fs;

use crate::types::ToolDefinition;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "edit_file".into(),
        description: "Edit a file by replacing exact text. The old_text must match exactly.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit"
                },
                "old_text": {
                    "type": "string",
                    "description": "Exact text to find and replace"
                },
                "new_text": {
                    "type": "string",
                    "description": "New text to replace with"
                }
            },
            "required": ["path", "old_text", "new_text"]
        }),
    }
}

pub async fn execute(input: &serde_json::Value) -> Result<String> {
    let path = input["path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'path' field"))?;
    let old_text = input["old_text"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'old_text' field"))?;
    let new_text = input["new_text"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'new_text' field"))?;

    let expanded = if path.starts_with("~/") {
        let home = dirs::home_dir().unwrap_or_default();
        home.join(&path[2..])
    } else {
        std::path::PathBuf::from(path)
    };

    let content = fs::read_to_string(&expanded)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", expanded.display()))?;

    let count = content.matches(old_text).count();
    if count == 0 {
        anyhow::bail!("old_text not found in {}", expanded.display());
    }
    if count > 1 {
        anyhow::bail!(
            "old_text found {count} times in {} — must be unique",
            expanded.display()
        );
    }

    let new_content = content.replacen(old_text, new_text, 1);
    fs::write(&expanded, &new_content)?;

    Ok(format!("Edited {}", expanded.display()))
}
