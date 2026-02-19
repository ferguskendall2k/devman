use anyhow::Result;
use colored::Colorize;

use crate::agent::AgentLoop;
use crate::auth::AuthStore;
use crate::client::AnthropicClient;
use crate::config::Config;
use crate::context::ContextManager;
use crate::tools;
use crate::types::Thinking;

/// Run a single task and exit
pub async fn run(config: &Config, message: &str) -> Result<()> {
    let auth = AuthStore::load()?;
    let api_key = auth.anthropic_api_key()?;
    let client = AnthropicClient::new(api_key);

    let context = ContextManager::new();
    let tool_defs = tools::builtin_tool_definitions(config.tools.web_enabled, config.github.is_some());
    let brave_key = auth.brave_api_key();
    let github_token = auth.github_token();

    let system_prompt = r#"You are DevMan, a helpful development assistant. Complete the given task using your tools. Be thorough but concise."#.to_string();

    let mut agent = AgentLoop::new(
        client,
        context,
        config.models.standard.clone(),
        system_prompt,
        tool_defs,
        config.agents.max_turns,
        config.agents.max_tokens,
        Thinking::Off,
        brave_key,
        github_token,
    );

    let result = agent.run_turn(message).await?;

    // Print final result to stdout (not stderr) for piping
    println!("{}", result.text);

    eprintln!(
        "{}",
        format!(
            "[{} in / {} out tokens]",
            result.usage.input_tokens, result.usage.output_tokens
        )
        .dimmed()
    );

    Ok(())
}
