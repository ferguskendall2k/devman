pub mod edit;
pub mod git;
pub mod memory;
pub mod read;
pub mod shell;
pub mod web_fetch;
pub mod web_search;
pub mod write;

use anyhow::Result;

use crate::memory::MemoryManager;
use crate::types::ToolDefinition;

/// Execute a tool call by name
pub async fn execute_tool(
    name: &str,
    input: &serde_json::Value,
    brave_api_key: Option<&str>,
    memory_manager: Option<&MemoryManager>,
) -> Result<String> {
    match name {
        "shell" => shell::execute(input).await,
        "git_status" => git::git_status_execute(input).await,
        "git_diff" => git::git_diff_execute(input).await,
        "git_commit" => git::git_commit_execute(input).await,
        "git_push" => git::git_push_execute(input).await,
        "git_log" => git::git_log_execute(input).await,
        "git_branch" => git::git_branch_execute(input).await,
        "read_file" => read::execute(input).await,
        "write_file" => write::execute(input).await,
        "edit_file" => edit::execute(input).await,
        "web_search" => web_search::execute(input, brave_api_key).await,
        "web_fetch" => web_fetch::execute(input).await,
        "memory_search" | "memory_read" | "memory_write" | "memory_load_task"
        | "memory_create_task" | "memory_update_index" => {
            let mm = memory_manager
                .ok_or_else(|| anyhow::anyhow!("memory manager not initialized"))?;
            match name {
                "memory_search" => memory::memory_search_execute(input, mm).await,
                "memory_read" => memory::memory_read_execute(input, mm).await,
                "memory_write" => memory::memory_write_execute(input, mm).await,
                "memory_load_task" => memory::memory_load_task_execute(input, mm).await,
                "memory_create_task" => memory::memory_create_task_execute(input, mm).await,
                "memory_update_index" => memory::memory_update_index_execute(input, mm).await,
                _ => unreachable!(),
            }
        }
        _ => anyhow::bail!("Unknown tool: {name}"),
    }
}

/// Get all built-in tool definitions
pub fn builtin_tool_definitions(web_enabled: bool) -> Vec<ToolDefinition> {
    let mut tools = vec![
        shell::definition(),
        read::definition(),
        write::definition(),
        edit::definition(),
    ];
    if web_enabled {
        tools.push(web_search::definition());
        tools.push(web_fetch::definition());
    }
    // Git tools always available
    tools.push(git::git_status_definition());
    tools.push(git::git_diff_definition());
    tools.push(git::git_commit_definition());
    tools.push(git::git_push_definition());
    tools.push(git::git_log_definition());
    tools.push(git::git_branch_definition());
    // Memory tools always available
    tools.push(memory::memory_search_definition());
    tools.push(memory::memory_read_definition());
    tools.push(memory::memory_write_definition());
    tools.push(memory::memory_load_task_definition());
    tools.push(memory::memory_create_task_definition());
    tools.push(memory::memory_update_index_definition());
    tools
}
