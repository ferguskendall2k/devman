use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Cost tracking — per-task, per-model, per-session
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CostTracker {
    pub by_task: HashMap<String, TaskCost>,
    pub by_model: HashMap<String, ModelCost>,
    pub session_total: Cost,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Cost {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub thinking_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TaskCost {
    pub task_id: String,
    pub cost: Cost,
    pub sub_agent_runs: u32,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ModelCost {
    pub model: String,
    pub cost: Cost,
    pub requests: u32,
}

/// Pricing per million tokens (USD) — as of Feb 2026
fn model_pricing(model: &str) -> (f64, f64) {
    // (input_per_1m, output_per_1m)
    if model.contains("haiku") {
        (0.25, 1.25)
    } else if model.contains("opus") {
        (15.0, 75.0)
    } else {
        // Sonnet / default
        (3.0, 15.0)
    }
}

impl CostTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record token usage for a request
    pub fn record(
        &mut self,
        model: &str,
        task_id: Option<&str>,
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
        cache_creation_tokens: u64,
    ) {
        let (input_price, output_price) = model_pricing(model);
        // Cache reads are 90% cheaper than regular input; cache creation costs 25% more
        let cost_usd = (input_tokens as f64 * input_price / 1_000_000.0)
            + (output_tokens as f64 * output_price / 1_000_000.0)
            + (cache_read_tokens as f64 * input_price * 0.1 / 1_000_000.0)
            + (cache_creation_tokens as f64 * input_price * 1.25 / 1_000_000.0);

        // Session total
        self.session_total.input_tokens += input_tokens;
        self.session_total.output_tokens += output_tokens;
        self.session_total.cache_read_tokens += cache_read_tokens;
        self.session_total.cache_creation_tokens += cache_creation_tokens;
        self.session_total.estimated_cost_usd += cost_usd;

        // By model
        let model_entry = self
            .by_model
            .entry(model.to_string())
            .or_insert_with(|| ModelCost {
                model: model.to_string(),
                ..Default::default()
            });
        model_entry.cost.input_tokens += input_tokens;
        model_entry.cost.output_tokens += output_tokens;
        model_entry.cost.cache_read_tokens += cache_read_tokens;
        model_entry.cost.cache_creation_tokens += cache_creation_tokens;
        model_entry.cost.estimated_cost_usd += cost_usd;
        model_entry.requests += 1;

        // By task
        if let Some(tid) = task_id {
            let task_entry = self
                .by_task
                .entry(tid.to_string())
                .or_insert_with(|| TaskCost {
                    task_id: tid.to_string(),
                    ..Default::default()
                });
            task_entry.cost.input_tokens += input_tokens;
            task_entry.cost.output_tokens += output_tokens;
            task_entry.cost.cache_read_tokens += cache_read_tokens;
            task_entry.cost.cache_creation_tokens += cache_creation_tokens;
            task_entry.cost.estimated_cost_usd += cost_usd;
        }
    }

    /// Format a summary report
    pub fn summary(&self) -> String {
        let mut lines = vec![format!(
            "Session: ${:.4} ({} in / {} out tokens)",
            self.session_total.estimated_cost_usd,
            self.session_total.input_tokens,
            self.session_total.output_tokens,
        )];

        if !self.by_model.is_empty() {
            lines.push("\nBy model:".into());
            for (_, mc) in &self.by_model {
                lines.push(format!(
                    "  {}: ${:.4} ({} requests)",
                    mc.model, mc.cost.estimated_cost_usd, mc.requests
                ));
            }
        }

        if !self.by_task.is_empty() {
            lines.push("\nBy task:".into());
            for (_, tc) in &self.by_task {
                lines.push(format!(
                    "  {}: ${:.4} ({} runs)",
                    tc.task_id, tc.cost.estimated_cost_usd, tc.sub_agent_runs
                ));
            }
        }

        lines.join("\n")
    }
}
