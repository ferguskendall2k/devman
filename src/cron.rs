use anyhow::{bail, Result};
use chrono::{DateTime, Datelike, Duration, NaiveTime, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: String,
    pub name: String,
    pub schedule: Schedule,
    pub action: CronAction,
    pub enabled: bool,
    pub last_run: Option<DateTime<Utc>>,
    pub next_run: Option<DateTime<Utc>>,
    pub created: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Schedule {
    At { at: DateTime<Utc> },
    Every { interval_ms: u64, anchor: Option<DateTime<Utc>> },
    Cron { expr: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CronAction {
    SystemEvent { text: String },
    AgentTask { message: String, model: Option<String> },
}

// ---------------------------------------------------------------------------
// CronScheduler
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct CronScheduler {
    jobs: Vec<CronJob>,
    #[serde(skip)]
    state_path: PathBuf,
}

impl CronScheduler {
    pub fn new(state_path: PathBuf) -> Self {
        if state_path.exists() {
            match Self::load(&state_path) {
                Ok(mut s) => {
                    s.state_path = state_path;
                    return s;
                }
                Err(e) => {
                    tracing::warn!("failed to load cron state: {e}, starting fresh");
                }
            }
        }
        Self { jobs: Vec::new(), state_path }
    }

    pub fn add(&mut self, mut job: CronJob) -> String {
        if job.id.is_empty() {
            job.id = uuid::Uuid::new_v4().to_string();
        }
        if job.next_run.is_none() {
            job.next_run = compute_next_run(&job.schedule, Utc::now());
        }
        let id = job.id.clone();
        self.jobs.push(job);
        id
    }

    pub fn remove(&mut self, id: &str) -> Result<()> {
        let before = self.jobs.len();
        self.jobs.retain(|j| j.id != id);
        if self.jobs.len() == before {
            bail!("no job with id: {id}");
        }
        Ok(())
    }

    pub fn update(
        &mut self,
        id: &str,
        enabled: Option<bool>,
        schedule: Option<Schedule>,
    ) -> Result<()> {
        let job = self.jobs.iter_mut().find(|j| j.id == id);
        match job {
            Some(j) => {
                if let Some(e) = enabled {
                    j.enabled = e;
                }
                if let Some(s) = schedule {
                    j.schedule = s;
                    j.next_run = compute_next_run(&j.schedule, Utc::now());
                }
                Ok(())
            }
            None => bail!("no job with id: {id}"),
        }
    }

    pub fn list(&self) -> &[CronJob] {
        &self.jobs
    }

    /// Returns clones of jobs that are due, updates their last_run/next_run.
    pub fn tick(&mut self) -> Vec<CronJob> {
        let now = Utc::now();
        let mut due = Vec::new();

        for job in &mut self.jobs {
            if !job.enabled {
                continue;
            }
            if let Some(next) = job.next_run {
                if next <= now {
                    due.push(job.clone());
                    job.last_run = Some(now);
                    job.next_run = compute_next_run(&job.schedule, now);
                }
            }
        }

        // Remove completed one-shot jobs
        self.jobs.retain(|j| {
            if let Schedule::At { .. } = &j.schedule {
                j.next_run.is_some() || j.last_run.is_none()
            } else {
                true
            }
        });

        due
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.state_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.jobs)?;
        std::fs::write(&self.state_path, json)?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let jobs: Vec<CronJob> = serde_json::from_str(&content)?;
        Ok(Self { jobs, state_path: path.to_path_buf() })
    }
}

// ---------------------------------------------------------------------------
// Next-run computation
// ---------------------------------------------------------------------------

pub fn compute_next_run(schedule: &Schedule, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    match schedule {
        Schedule::At { at } => {
            if *at > after { Some(*at) } else { None }
        }
        Schedule::Every { interval_ms, anchor } => {
            let base = anchor.unwrap_or(after);
            if base > after {
                return Some(base);
            }
            // Advance from anchor by multiples of interval
            let elapsed = (after - base).num_milliseconds();
            let periods = elapsed / (*interval_ms as i64) + 1;
            Some(base + Duration::milliseconds(periods * (*interval_ms as i64)))
        }
        Schedule::Cron { expr } => cron_next(expr, after),
    }
}

// ---------------------------------------------------------------------------
// Simple cron parser — 5 fields: min hour dom month dow
// Supports: *, */N, single numbers, comma-separated lists
// ---------------------------------------------------------------------------

fn parse_cron_field(field: &str, min: u32, max: u32) -> Vec<u32> {
    let mut values = Vec::new();
    for part in field.split(',') {
        let part = part.trim();
        if part == "*" {
            return (min..=max).collect();
        } else if let Some(step) = part.strip_prefix("*/") {
            if let Ok(n) = step.parse::<u32>() {
                if n > 0 {
                    let mut v = min;
                    while v <= max {
                        values.push(v);
                        v += n;
                    }
                }
            }
        } else if let Ok(n) = part.parse::<u32>() {
            if n >= min && n <= max {
                values.push(n);
            }
        }
    }
    values.sort();
    values.dedup();
    values
}

fn cron_next(expr: &str, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    if fields.len() != 5 {
        return None;
    }

    let minutes = parse_cron_field(fields[0], 0, 59);
    let hours = parse_cron_field(fields[1], 0, 23);
    let doms = parse_cron_field(fields[2], 1, 31);
    let months = parse_cron_field(fields[3], 1, 12);
    let dows = parse_cron_field(fields[4], 0, 6); // 0=Sun

    if minutes.is_empty() || hours.is_empty() || doms.is_empty() || months.is_empty() || dows.is_empty() {
        return None;
    }

    // Brute-force search from after+1min, up to 1 year ahead
    let start = after + Duration::minutes(1);
    // Truncate to start of minute
    let start = start
        .date_naive()
        .and_time(NaiveTime::from_hms_opt(start.hour(), start.minute(), 0)?)
        .and_utc();

    let limit = after + Duration::days(366);
    let mut candidate = start;

    while candidate < limit {
        let m = candidate.month();
        let d = candidate.day();
        let h = candidate.hour();
        let min = candidate.minute();
        let dow = candidate.weekday().num_days_from_sunday(); // 0=Sun

        if months.contains(&m)
            && doms.contains(&d)
            && dows.contains(&dow)
            && hours.contains(&h)
            && minutes.contains(&min)
        {
            return Some(candidate);
        }

        candidate = candidate + Duration::minutes(1);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_every_interval() {
        let now = Utc::now();
        let schedule = Schedule::Every { interval_ms: 60_000, anchor: Some(now) };
        let next = compute_next_run(&schedule, now).unwrap();
        assert!(next > now);
        assert!((next - now).num_seconds() <= 61);
    }

    #[test]
    fn test_at_future() {
        let future = Utc::now() + Duration::hours(1);
        let schedule = Schedule::At { at: future };
        assert_eq!(compute_next_run(&schedule, Utc::now()), Some(future));
    }

    #[test]
    fn test_at_past() {
        let past = Utc::now() - Duration::hours(1);
        let schedule = Schedule::At { at: past };
        assert_eq!(compute_next_run(&schedule, Utc::now()), None);
    }

    #[test]
    fn test_cron_field_star() {
        assert_eq!(parse_cron_field("*", 0, 59).len(), 60);
    }

    #[test]
    fn test_cron_field_step() {
        assert_eq!(parse_cron_field("*/15", 0, 59), vec![0, 15, 30, 45]);
    }

    #[test]
    fn test_cron_field_specific() {
        assert_eq!(parse_cron_field("5", 0, 59), vec![5]);
    }

    #[test]
    fn test_cron_next_every_5_min() {
        let now = Utc::now();
        let next = cron_next("*/5 * * * *", now);
        assert!(next.is_some());
        assert!(next.unwrap() > now);
    }

    #[test]
    fn test_scheduler_add_remove() {
        let mut sched = CronScheduler::new(PathBuf::from("/tmp/devman-test-cron.json"));
        let id = sched.add(CronJob {
            id: String::new(),
            name: "test".into(),
            schedule: Schedule::Every { interval_ms: 1000, anchor: None },
            action: CronAction::SystemEvent { text: "hi".into() },
            enabled: true,
            last_run: None,
            next_run: None,
            created: Utc::now(),
        });
        assert_eq!(sched.list().len(), 1);
        sched.remove(&id).unwrap();
        assert_eq!(sched.list().len(), 0);
    }
}
