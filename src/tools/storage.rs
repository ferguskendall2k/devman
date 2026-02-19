use anyhow::Result;
use serde_json::Value;

use crate::memory::TaskStorage;
use crate::types::ToolDefinition;

pub fn storage_write_definition() -> ToolDefinition {
    ToolDefinition {
        name: "storage_write".to_string(),
        description: "Write a file to task storage. Use base64=true for binary files (images, PDFs).".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to storage root (e.g. 'references/spec.pdf', 'data/config.json')"
                },
                "content": {
                    "type": "string",
                    "description": "File content (text or base64-encoded binary)"
                },
                "base64": {
                    "type": "boolean",
                    "description": "If true, content is base64-encoded binary data",
                    "default": false
                }
            },
            "required": ["path", "content"]
        }),
    }
}

pub async fn storage_write_execute(input: &Value, storage: &TaskStorage) -> Result<String> {
    let path = input["path"].as_str().unwrap_or("");
    let content = input["content"].as_str().unwrap_or("");
    let base64 = input["base64"].as_bool().unwrap_or(false);

    if path.is_empty() {
        anyhow::bail!("path is required");
    }

    storage.write_file(path, content, base64)?;
    let (bytes, _) = storage.usage()?;
    Ok(format!("Written: {path} (storage total: {} files, {bytes} bytes)", storage.list_files(None)?.len()))
}

pub fn storage_read_definition() -> ToolDefinition {
    ToolDefinition {
        name: "storage_read".to_string(),
        description: "Read a file from task storage. Binary files are returned as base64.".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to storage root"
                }
            },
            "required": ["path"]
        }),
    }
}

pub async fn storage_read_execute(input: &Value, storage: &TaskStorage) -> Result<String> {
    let path = input["path"].as_str().unwrap_or("");
    if path.is_empty() {
        anyhow::bail!("path is required");
    }
    storage.read_file(path)
}

pub fn storage_list_definition() -> ToolDefinition {
    ToolDefinition {
        name: "storage_list".to_string(),
        description: "List files in task storage. Optionally list a subdirectory.".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "subdir": {
                    "type": "string",
                    "description": "Optional subdirectory to list"
                }
            }
        }),
    }
}

pub async fn storage_list_execute(input: &Value, storage: &TaskStorage) -> Result<String> {
    let subdir = input["subdir"].as_str();
    let files = storage.list_files(subdir)?;
    if files.is_empty() {
        return Ok("Storage is empty.".to_string());
    }
    let (bytes, count) = storage.usage()?;
    let mut out = format!("{count} files, {bytes} bytes total:\n");
    for f in &files {
        out.push_str(&format!("  {f}\n"));
    }
    Ok(out)
}

pub fn storage_delete_definition() -> ToolDefinition {
    ToolDefinition {
        name: "storage_delete".to_string(),
        description: "Delete a file or directory from task storage.".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File or directory path to delete"
                }
            },
            "required": ["path"]
        }),
    }
}

pub async fn storage_delete_execute(input: &Value, storage: &TaskStorage) -> Result<String> {
    let path = input["path"].as_str().unwrap_or("");
    if path.is_empty() {
        anyhow::bail!("path is required");
    }
    storage.delete_file(path)?;
    Ok(format!("Deleted: {path}"))
}
