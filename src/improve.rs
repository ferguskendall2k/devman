use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub struct ImprovementEngine {
    learnings_path: PathBuf,
    stats_path: PathBuf,
    learnings: Vec<Learning>,
    stats: ModelStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Learning {
    pub id: String,
    pub category: LearningCategory,
    pub text: String,
    pub source: String,
    pub created: DateTime<Utc>,
    pub applied: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LearningCategory {
    Mistake,
    Preference,
    Pattern,
    Optimization,
    Tool,
}

impl LearningCategory {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "mistake" => Some(Self::Mistake),
            "preference" => Some(Self::Preference),
            "pattern" => Some(Self::Pattern),
            "optimization" => Some(Self::Optimization),
            "tool" => Some(Self::Tool),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Mistake => "mistake",
            Self::Preference => "preference",
            Self::Pattern => "pattern",
            Self::Optimization => "optimization",
            Self::Tool => "tool",
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ModelStats {
    pub total_requests: u64,
    pub by_model: HashMap<String, ModelUsage>,
    pub by_task_type: HashMap<String, u64>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ModelUsage {
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub avg_latency_ms: u64,
    pub errors: u64,
}

impl ImprovementEngine {
    pub fn new(state_dir: PathBuf) -> Result<Self> {
        Self::load(&state_dir)
    }

    pub fn add_learning(&mut self, category: LearningCategory, text: &str, source: &str) -> String {
        let id = Uuid::new_v4().to_string()[..8].to_string();
        self.learnings.push(Learning {
            id: id.clone(),
            category,
            text: text.to_string(),
            source: source.to_string(),
            created: Utc::now(),
            applied: false,
        });
        id
    }

    pub fn list_learnings(&self, category: Option<LearningCategory>) -> Vec<&Learning> {
        self.learnings
            .iter()
            .filter(|l| match &category {
                Some(cat) => l.category == *cat,
                None => true,
            })
            .collect()
    }

    pub fn record_request(
        &mut self,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
        latency_ms: u64,
        error: bool,
    ) {
        self.stats.total_requests += 1;
        let usage = self.stats.by_model.entry(model.to_string()).or_default();
        let prev_total_latency = usage.avg_latency_ms * usage.requests;
        usage.requests += 1;
        usage.input_tokens += input_tokens;
        usage.output_tokens += output_tokens;
        usage.avg_latency_ms = (prev_total_latency + latency_ms) / usage.requests;
        if error {
            usage.errors += 1;
        }
    }

    pub fn get_stats(&self) -> &ModelStats {
        &self.stats
    }

    pub fn generate_retrospective(&self) -> String {
        let mut lines = vec!["# Retrospective".to_string(), String::new()];

        // Learnings summary
        lines.push(format!("## Learnings ({})", self.learnings.len()));
        let categories = [
            LearningCategory::Mistake,
            LearningCategory::Preference,
            LearningCategory::Pattern,
            LearningCategory::Optimization,
            LearningCategory::Tool,
        ];
        for cat in &categories {
            let count = self.learnings.iter().filter(|l| l.category == *cat).count();
            if count > 0 {
                lines.push(format!("- {}: {}", cat.as_str(), count));
            }
        }

        // Recent learnings
        let recent: Vec<_> = self.learnings.iter().rev().take(5).collect();
        if !recent.is_empty() {
            lines.push(String::new());
            lines.push("### Recent".to_string());
            for l in recent {
                lines.push(format!(
                    "- [{}] {} (from: {})",
                    l.category.as_str(),
                    l.text,
                    l.source
                ));
            }
        }

        // Model stats
        lines.push(String::new());
        lines.push(format!(
            "## Model Usage ({} total requests)",
            self.stats.total_requests
        ));
        for (model, usage) in &self.stats.by_model {
            lines.push(format!(
                "- {}: {} requests, {}in/{}out tokens, ~{}ms avg, {} errors",
                model,
                usage.requests,
                usage.input_tokens,
                usage.output_tokens,
                usage.avg_latency_ms,
                usage.errors
            ));
        }

        lines.join("\n")
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.learnings_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let learnings_json = serde_json::to_string_pretty(&self.learnings)?;
        std::fs::write(&self.learnings_path, learnings_json)
            .with_context(|| "writing learnings.json")?;

        let stats_json = serde_json::to_string_pretty(&self.stats)?;
        std::fs::write(&self.stats_path, stats_json)
            .with_context(|| "writing model-stats.json")?;
        Ok(())
    }

    pub fn load(state_dir: &Path) -> Result<Self> {
        let learnings_path = state_dir.join("learnings.json");
        let stats_path = state_dir.join("model-stats.json");

        let learnings = if learnings_path.exists() {
            let data = std::fs::read_to_string(&learnings_path)?;
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            Vec::new()
        };

        let stats = if stats_path.exists() {
            let data = std::fs::read_to_string(&stats_path)?;
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            ModelStats::default()
        };

        Ok(Self {
            learnings_path,
            stats_path,
            learnings,
            stats,
        })
    }
}
