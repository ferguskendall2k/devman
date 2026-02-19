pub mod edit;
pub mod read;
pub mod shell;
pub mod web_fetch;
pub mod web_search;
pub mod write;

use anyhow::Result;

use crate::types::ToolDefinition;

/// Execute a tool call by name
pub async fn execute_tool(
    name: &str,
    input: &serde_json::Value,
    brave_api_key: Option<&str>,
) -> Result<String> {
    match name {
        "shell" => shell::execute(input).await,
        "read_file" => read::execute(input).await,
        "write_file" => write::execute(input).await,
        "edit_file" => edit::execute(input).await,
        "web_search" => web_search::execute(input, brave_api_key).await,
        "web_fetch" => web_fetch::execute(input).await,
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
    tools
}
