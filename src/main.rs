#![allow(dead_code, unused_imports)]

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
mod render;
mod telegram;
mod tools;
mod types;
mod voice;
mod logging;

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
    /// Show cost tracking summary
    Cost,
    /// Manage cron jobs
    Cron {
        #[command(subcommand)]
        action: CronAction,
    },
}

#[derive(Subcommand)]
enum CronAction {
    /// List all jobs
    List,
    /// Add a job
    Add {
        /// Job name
        #[arg(short, long)]
        name: String,
        /// Cron expression (e.g. "*/5 * * * *")
        #[arg(short, long)]
        schedule: String,
        /// Agent message to run
        #[arg(short, long)]
        message: String,
    },
    /// Remove a job
    Remove {
        /// Job ID
        id: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = config::Config::load().unwrap_or_default();

    // Init logging
    let _ = logging::init(
        &config.logging.level,
        config.logging.file.as_ref(),
    );

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
        Some(Commands::Cost) => {
            let state_dir = dirs::data_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("devman");
            let cost_path = state_dir.join("cost-tracker.json");
            if cost_path.exists() {
                let data = std::fs::read_to_string(&cost_path)?;
                let tracker: cost::CostTracker = serde_json::from_str(&data)?;
                println!("{}", tracker.summary());
            } else {
                println!("No cost data yet. Run `devman chat` or `devman serve` first.");
            }
            Ok(())
        }
        Some(Commands::Cron { action }) => {
            let state_dir = dirs::data_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("devman");
            std::fs::create_dir_all(&state_dir)?;
            let mut scheduler = cron::CronScheduler::new(state_dir.join("cron-jobs.json"));

            match action {
                CronAction::List => {
                    let jobs = scheduler.list();
                    if jobs.is_empty() {
                        println!("No cron jobs configured.");
                    } else {
                        for job in jobs {
                            println!(
                                "{} [{}] {} — {:?} ({})",
                                if job.enabled { "✅" } else { "⏸️" },
                                &job.id[..8],
                                job.name,
                                job.schedule,
                                if job.enabled { "enabled" } else { "disabled" }
                            );
                        }
                    }
                }
                CronAction::Add { name, schedule, message } => {
                    let job = cron::CronJob {
                        id: uuid::Uuid::new_v4().to_string(),
                        name: name.clone(),
                        schedule: cron::Schedule::Cron { expr: schedule },
                        action: cron::CronAction::AgentTask { message, model: None },
                        enabled: true,
                        last_run: None,
                        next_run: None,
                        created: chrono::Utc::now(),
                    };
                    let id = scheduler.add(job);
                    scheduler.save()?;
                    println!("Added cron job: {} ({})", name, &id[..8]);
                }
                CronAction::Remove { id } => {
                    // Match by prefix
                    let full_id = {
                        let jobs = scheduler.list();
                        jobs.iter()
                            .find(|j| j.id.starts_with(&id))
                            .map(|j| j.id.clone())
                    };
                    if let Some(full) = full_id {
                        scheduler.remove(&full)?;
                        scheduler.save()?;
                        println!("Removed job {}", &full[..8]);
                    } else {
                        println!("No job matching '{id}'");
                    }
                }
            }
            Ok(())
        }
    }
}
