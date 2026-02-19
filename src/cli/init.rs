use anyhow::Result;
use colored::Colorize;
use std::io::{self, Write};

/// Guided first-run setup
pub async fn run() -> Result<()> {
    eprintln!("{}", "Welcome to DevMan 🔧\n".bold());

    // 1. Check Anthropic auth
    eprintln!("{}", "1. Anthropic Authentication".bold());

    // Check for Claude Code credentials
    let home = dirs::home_dir().unwrap_or_default();
    let claude_creds = home.join(".claude/.credentials.json");
    if claude_creds.exists() {
        eprintln!("   Found Claude Code credentials (~/.claude/.credentials.json)");
        eprintln!(
            "   → {}",
            "Using your claude.ai subscription (no separate API costs)".green()
        );
        eprintln!("   {} Authenticated", "✓".green());
    } else {
        eprintln!("   No Claude Code credentials found.");
        eprintln!("   Options:");
        eprintln!("     a) Install Claude Code and login (recommended)");
        eprintln!("        → npm install -g @anthropic-ai/claude-code && claude login");
        eprintln!("     b) Set ANTHROPIC_API_KEY environment variable");

        if std::env::var("ANTHROPIC_API_KEY").is_ok() {
            eprintln!("   {} Found ANTHROPIC_API_KEY in environment", "✓".green());
        } else {
            eprintln!(
                "   {} No API key found — run Claude Code login or set ANTHROPIC_API_KEY",
                "⚠".yellow()
            );
        }
    }

    // 2. Check for Brave Search
    eprintln!("\n{}", "2. Web Search (Brave)".bold());
    if std::env::var("BRAVE_API_KEY").is_ok() {
        eprintln!("   {} Brave Search API key found", "✓".green());
    } else {
        eprintln!(
            "   {} Not configured — set BRAVE_API_KEY for web search",
            "○".dimmed()
        );
    }

    // 3. Create config directory
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("devman");
    std::fs::create_dir_all(&config_dir)?;

    // 4. Create default config if it doesn't exist
    let config_path = config_dir.join("config.toml");
    if !config_path.exists() {
        let default_config = crate::config::Config::default();
        default_config.save()?;
        eprintln!(
            "\n{} Config written to {}",
            "✓".green(),
            config_path.display()
        );
    } else {
        eprintln!(
            "\n{} Config already exists at {}",
            "✓".green(),
            config_path.display()
        );
    }

    eprintln!("\n{}", "Ready! Run `devman chat` to start.".green().bold());
    Ok(())
}
