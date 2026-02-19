use anyhow::Result;
use clap::{Parser, Subcommand};

mod agent;
mod auth;
mod client;
mod cli;
mod config;
mod context;
mod cost;
mod cron;
mod dashboard;
mod improve;
mod manager;
mod memory;
mod orchestrator;
mod telegram;
mod tools;
mod types;
mod voice;

#[derive(Parser)]
#[command(name = "devman", version, about = "Lightweight agentic framework for Claude 🔧")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Interactive chat REPL
    Chat,
    /// Run a single task
    Run {
        /// Task message
        #[arg(short, long)]
        message: String,
    },
    /// Guided first-run setup
    Init,
    /// Show auth status
    Auth,
    /// Start Telegram bot + agent daemon
    Serve,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Init tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();
    let config = config::Config::load().unwrap_or_default();

    match cli.command {
        Some(Commands::Chat) | None => cli::chat::run(&config).await,
        Some(Commands::Run { message }) => cli::run::run(&config, &message).await,
        Some(Commands::Init) => cli::init::run().await,
        Some(Commands::Serve) => cli::serve::run(&config).await,
        Some(Commands::Auth) => {
            let auth = auth::AuthStore::load()?;
            match auth.anthropic_api_key() {
                Ok(key) => {
                    let masked = if key.len() > 20 {
                        format!("{}...{}", &key[..12], &key[key.len() - 4..])
                    } else {
                        "***".into()
                    };
                    let source = if key.starts_with("sk-ant-oat") {
                        "OAuth (claude.ai subscription)"
                    } else {
                        "API key"
                    };
                    println!("Anthropic: {} ({})", masked, source);
                }
                Err(_) => println!("Anthropic: not configured"),
            }
            if auth.brave_api_key().is_some() {
                println!("Brave Search: configured");
            } else {
                println!("Brave Search: not configured");
            }
            if auth.telegram_bot_token().is_some() {
                println!("Telegram: configured");
            } else {
                println!("Telegram: not configured");
            }
            Ok(())
        }
    }
}
