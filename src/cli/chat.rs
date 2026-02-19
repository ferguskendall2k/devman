use anyhow::Result;
use colored::Colorize;
use std::io::{self, BufRead, Write};

use crate::agent::AgentLoop;
use crate::auth::AuthStore;
use crate::client::AnthropicClient;
use crate::config::Config;
use crate::context::ContextManager;
use crate::tools;
use crate::types::Thinking;

/// Interactive chat REPL
pub async fn run(config: &Config) -> Result<()> {
    let auth = AuthStore::load()?;
    let api_key = auth.anthropic_api_key()?;
    let client = AnthropicClient::new(api_key);

    // Resolve conversation persistence path
    let state_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("devman");
    std::fs::create_dir_all(&state_dir)?;
    let context = ContextManager::with_persistence(state_dir.join("conversation.json"));

    let tool_defs = tools::builtin_tool_definitions(config.tools.web_enabled);
    let brave_key = auth.brave_api_key();

    let system_prompt = load_system_prompt();

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
    );

    eprintln!("{}", "DevMan 🔧 — type /quit to exit, /clear to reset".bold());
    eprintln!();

    let stdin = io::stdin();
    let mut reader = stdin.lock();

    loop {
        eprint!("{}", "You: ".green().bold());
        io::stderr().flush()?;

        let mut input = String::new();
        if reader.read_line(&mut input)? == 0 {
            break; // EOF
        }

        let trimmed = input.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Handle commands
        match trimmed {
            "/quit" | "/exit" | "/q" => break,
            "/clear" => {
                agent.context = ContextManager::new();
                eprintln!("{}", "Conversation cleared.".dimmed());
                continue;
            }
            "/cost" => {
                eprintln!(
                    "Tokens: {} input, {} output",
                    agent.context.total_input_tokens,
                    agent.context.total_output_tokens
                );
                continue;
            }
            _ => {}
        }

        eprint!("{}", "Al: ".cyan().bold());

        match agent.run_turn(trimmed).await {
            Ok(result) => {
                eprintln!(
                    "{}",
                    format!(
                        "[{} in / {} out tokens]",
                        result.usage.input_tokens, result.usage.output_tokens
                    )
                    .dimmed()
                );
            }
            Err(e) => {
                eprintln!("{}", format!("Error: {e}").red());
            }
        }

        eprintln!();
    }

    eprintln!("{}", "Bye! 👋".dimmed());
    Ok(())
}

fn load_system_prompt() -> String {
    // Try loading from .devman/system.md or fall back to default
    let candidates = [
        std::path::PathBuf::from(".devman/system.md"),
        dirs::config_dir()
            .unwrap_or_default()
            .join("devman/system.md"),
    ];

    for path in &candidates {
        if let Ok(content) = std::fs::read_to_string(path) {
            return content;
        }
    }

    // Default system prompt
    r#"You are DevMan, a helpful development assistant. You have access to tools for reading/writing files, executing shell commands, and searching the web.

Be concise and helpful. Use tools proactively to answer questions and complete tasks. When modifying code, make precise edits rather than rewriting entire files."#
        .to_string()
}
