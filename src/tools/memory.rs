use anyhow::Result;
use serde_json::json;

use crate::memory::MemoryManager;
use crate::types::ToolDefinition;

pub fn memory_search_definition() -> ToolDefinition {
    ToolDefinition {
        name: "memory_search".into(),
        description: "Search across memory files using grep.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query (grep pattern)"
                }
            },
            "required": ["query"]
        }),
    }
}

pub async fn memory_search_execute(input: &serde_json::Value, memory: &MemoryManager) -> Result<String> {
    let query = input["query"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'query' field"))?;

    let results = memory.search(query);
    if results.is_empty() {
        return Ok("No results found.".into());
    }

    let output: Vec<String> = results
        .iter()
        .map(|r| format!("{}:{}: {}", r.file, r.line, r.text))
        .collect();
    Ok(output.join("\n"))
}

pub fn memory_read_definition() -> ToolDefinition {
    ToolDefinition {
        name: "memory_read".into(),
        description: "Read a memory file (path relative to memory root).".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to memory root"
                }
            },
            "required": ["path"]
        }),
    }
}

pub async fn memory_read_execute(input: &serde_json::Value, memory: &MemoryManager) -> Result<String> {
    let path = input["path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'path' field"))?;

    memory.read_file(path)
}

pub fn memory_write_definition() -> ToolDefinition {
    ToolDefinition {
        name: "memory_write".into(),
        description: "Write or append to a memory file.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to memory root"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write"
                },
                "append": {
                    "type": "boolean",
                    "description": "If true, append instead of overwrite (default: false)"
                }
            },
            "required": ["path", "content"]
        }),
    }
}

pub async fn memory_write_execute(input: &serde_json::Value, memory: &MemoryManager) -> Result<String> {
    let path = input["path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'path' field"))?;
    let content = input["content"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'content' field"))?;
    let append = input["append"].as_bool().unwrap_or(false);

    if append {
        memory.append_file(path, content)?;
        Ok(format!("Appended to {path}"))
    } else {
        memory.write_file(path, content)?;
        Ok(format!("Wrote {path}"))
    }
}

pub fn memory_load_task_definition() -> ToolDefinition {
    ToolDefinition {
        name: "memory_load_task".into(),
        description: "Load a task by name or alias from INDEX.md.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Task name or alias to search for"
                }
            },
            "required": ["name"]
        }),
    }
}

pub async fn memory_load_task_execute(input: &serde_json::Value, memory: &MemoryManager) -> Result<String> {
    let name = input["name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'name' field"))?;

    memory.load_task(name)
}

pub fn memory_create_task_definition() -> ToolDefinition {
    ToolDefinition {
        name: "memory_create_task".into(),
        description: "Create a new task from template and update INDEX.md.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Task name"
                },
                "template_type": {
                    "type": "string",
                    "description": "Template type (default: 'standard')"
                }
            },
            "required": ["name"]
        }),
    }
}

pub async fn memory_create_task_execute(input: &serde_json::Value, memory: &MemoryManager) -> Result<String> {
    let name = input["name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'name' field"))?;
    let template_type = input["template_type"].as_str().unwrap_or("standard");

    memory.create_task(name, template_type)
}

pub fn memory_update_index_definition() -> ToolDefinition {
    ToolDefinition {
        name: "memory_update_index".into(),
        description: "Update a task's status and summary in INDEX.md.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "task_name": {
                    "type": "string",
                    "description": "Task name to update"
                },
                "status": {
                    "type": "string",
                    "description": "New status (e.g. IN PROGRESS, DONE, BLOCKED)"
                },
                "summary": {
                    "type": "string",
                    "description": "Brief summary of current state"
                }
            },
            "required": ["task_name", "status", "summary"]
        }),
    }
}

pub async fn memory_update_index_execute(input: &serde_json::Value, memory: &MemoryManager) -> Result<String> {
    let task_name = input["task_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'task_name' field"))?;
    let status = input["status"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'status' field"))?;
    let summary = input["summary"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'summary' field"))?;

    memory.update_index(task_name, status, summary)?;
    Ok(format!("Updated {task_name} → {status}"))
}
