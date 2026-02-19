use chrono::Datelike;
use devman::context::ContextManager;
use devman::cost::CostTracker;
use devman::cron::*;
use devman::memory::MemoryManager;
use devman::types::ContentBlock;
use tempfile::TempDir;

// ───────────────────── Context Manager ─────────────────────

#[test]
fn test_add_messages() {
    let mut ctx = ContextManager::new();
    ctx.add_user_message("hello");
    ctx.add_assistant_message(vec![ContentBlock::Text {
        text: "hi there".into(),
    }]);
    assert_eq!(ctx.messages.len(), 2);
    assert_eq!(ctx.messages[0].role, devman::types::Role::User);
    assert_eq!(ctx.messages[1].role, devman::types::Role::Assistant);
}

#[test]
fn test_compact() {
    let mut ctx = ContextManager::new();
    for i in 0..20 {
        ctx.add_user_message(&format!("msg {i}"));
    }
    assert_eq!(ctx.messages.len(), 20);
    ctx.compact(5);
    // Should be: 1 summary + 1 assistant ack + 5 recent = 7
    assert_eq!(ctx.messages.len(), 7);
    // First message should be the compaction summary
    if let ContentBlock::Text { text } = &ctx.messages[0].content[0] {
        assert!(text.contains("compacted"));
    } else {
        panic!("expected text block");
    }
}

#[test]
fn test_estimated_tokens() {
    let mut ctx = ContextManager::new();
    // 400 chars ≈ 100 tokens
    ctx.add_user_message(&"a".repeat(400));
    let tokens = ctx.estimated_tokens();
    assert!(tokens >= 90 && tokens <= 110, "got {tokens}");
}

#[test]
fn test_persistence() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("ctx.json");

    // Save
    {
        let mut ctx = ContextManager::with_persistence(path.clone());
        ctx.add_user_message("remember me");
        ctx.add_assistant_message(vec![ContentBlock::Text {
            text: "noted".into(),
        }]);
        ctx.save().unwrap();
    }

    // Reload
    {
        let ctx = ContextManager::with_persistence(path);
        assert_eq!(ctx.messages.len(), 2);
        if let ContentBlock::Text { text } = &ctx.messages[0].content[0] {
            assert_eq!(text, "remember me");
        } else {
            panic!("expected text");
        }
    }
}

// ───────────────────── Cost Tracker ─────────────────────

#[test]
fn test_record_cost() {
    let mut ct = CostTracker::new();
    ct.record("sonnet", None, 1000, 500, 0, 0);
    assert_eq!(ct.session_total.input_tokens, 1000);
    assert_eq!(ct.session_total.output_tokens, 500);
    assert!(ct.session_total.estimated_cost_usd > 0.0);
}

#[test]
fn test_cost_by_model() {
    let mut ct = CostTracker::new();
    ct.record("haiku", None, 1000, 1000, 0, 0);
    ct.record("opus", None, 1000, 1000, 0, 0);
    assert_eq!(ct.by_model.len(), 2);
    assert!(ct.by_model.contains_key("haiku"));
    assert!(ct.by_model.contains_key("opus"));
    // Opus should cost more
    let haiku_cost = ct.by_model["haiku"].cost.estimated_cost_usd;
    let opus_cost = ct.by_model["opus"].cost.estimated_cost_usd;
    assert!(opus_cost > haiku_cost);
}

#[test]
fn test_cost_summary() {
    let mut ct = CostTracker::new();
    ct.record("sonnet", Some("task-1"), 500, 200, 0, 0);
    let summary = ct.summary();
    assert!(summary.contains("Session:"));
    assert!(summary.contains("$"));
    assert!(summary.contains("sonnet"));
}

// ───────────────────── Memory Manager ─────────────────────

#[test]
fn test_create_task() {
    let dir = TempDir::new().unwrap();
    let mm = MemoryManager::new(dir.path().to_path_buf());

    // Create INDEX.md first (append needs it to exist or creates it)
    let result = mm.create_task("My Cool Task", "default").unwrap();
    assert!(result.contains("my-cool-task"));

    // Verify task file exists
    assert!(dir.path().join("tasks/my-cool-task.md").exists());

    // Verify INDEX.md updated
    let index = mm.read_file("INDEX.md").unwrap();
    assert!(index.contains("My Cool Task"));
}

#[test]
fn test_write_and_read() {
    let dir = TempDir::new().unwrap();
    let mm = MemoryManager::new(dir.path().to_path_buf());

    mm.write_file("test.md", "hello world").unwrap();
    let content = mm.read_file("test.md").unwrap();
    assert_eq!(content, "hello world");
}

#[test]
fn test_search() {
    let dir = TempDir::new().unwrap();
    let mm = MemoryManager::new(dir.path().to_path_buf());

    mm.write_file("notes.md", "The quick brown fox\njumps over the lazy dog")
        .unwrap();

    let results = mm.search("quick brown");
    assert!(!results.is_empty());
    assert!(results[0].text.contains("quick brown fox"));
}

// ───────────────────── Cron Scheduler ─────────────────────

#[test]
fn test_add_remove_job() {
    let dir = TempDir::new().unwrap();
    let mut sched = CronScheduler::new(dir.path().join("cron.json"));

    let id = sched.add(CronJob {
        id: String::new(),
        name: "test-job".into(),
        schedule: Schedule::Every {
            interval_ms: 60_000,
            anchor: None,
        },
        action: CronAction::SystemEvent {
            text: "ping".into(),
        },
        enabled: true,
        last_run: None,
        next_run: None,
        created: chrono::Utc::now(),
    });

    assert_eq!(sched.list().len(), 1);
    assert_eq!(sched.list()[0].name, "test-job");

    sched.remove(&id).unwrap();
    assert_eq!(sched.list().len(), 0);
}

#[test]
fn test_tick_fires_due_job() {
    let dir = TempDir::new().unwrap();
    let mut sched = CronScheduler::new(dir.path().join("cron.json"));

    let past = chrono::Utc::now() - chrono::Duration::seconds(10);
    sched.add(CronJob {
        id: "due-job".into(),
        name: "due".into(),
        schedule: Schedule::Every {
            interval_ms: 60_000,
            anchor: Some(past),
        },
        action: CronAction::SystemEvent {
            text: "fired".into(),
        },
        enabled: true,
        last_run: None,
        next_run: Some(past), // already due
        created: chrono::Utc::now(),
    });

    let due = sched.tick();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].id, "due-job");
}

#[test]
fn test_one_shot_removed() {
    let dir = TempDir::new().unwrap();
    let mut sched = CronScheduler::new(dir.path().join("cron.json"));

    let past = chrono::Utc::now() - chrono::Duration::seconds(5);
    sched.add(CronJob {
        id: "oneshot".into(),
        name: "once".into(),
        schedule: Schedule::At { at: past },
        action: CronAction::SystemEvent {
            text: "boom".into(),
        },
        enabled: true,
        last_run: None,
        next_run: Some(past),
        created: chrono::Utc::now(),
    });

    assert_eq!(sched.list().len(), 1);
    let due = sched.tick();
    assert_eq!(due.len(), 1);
    // After tick, one-shot should be removed
    assert_eq!(sched.list().len(), 0);
}

#[test]
fn test_cron_expression_parsing() {
    let now = chrono::Utc::now();

    // Every 5 minutes
    let next = compute_next_run(
        &Schedule::Cron {
            expr: "*/5 * * * *".into(),
        },
        now,
    );
    assert!(next.is_some());
    assert!(next.unwrap() > now);

    // Mondays at 9am
    let next = compute_next_run(
        &Schedule::Cron {
            expr: "0 9 * * 1".into(),
        },
        now,
    );
    assert!(next.is_some());
    let n = next.unwrap();
    assert_eq!(n.format("%H:%M").to_string(), "09:00");
    // Should be a Monday (weekday num_days_from_monday == 0)
    assert_eq!(n.weekday().num_days_from_monday(), 0);
}

// ───────────────────── File Tools ─────────────────────

#[tokio::test]
async fn test_write_read_file() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("test.txt");
    let path_str = file_path.to_str().unwrap();

    // Write
    let input = serde_json::json!({
        "path": path_str,
        "content": "Hello, DevMan!"
    });
    let result = devman::tools::write::execute(&input).await.unwrap();
    assert!(result.contains("14 bytes"));

    // Read
    let input = serde_json::json!({ "path": path_str });
    let result = devman::tools::read::execute(&input).await.unwrap();
    assert_eq!(result, "Hello, DevMan!");
}

#[tokio::test]
async fn test_edit_file() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("edit_me.txt");
    let path_str = file_path.to_str().unwrap();

    // Write initial content
    std::fs::write(&file_path, "foo bar baz").unwrap();

    // Edit
    let input = serde_json::json!({
        "path": path_str,
        "old_text": "bar",
        "new_text": "qux"
    });
    let result = devman::tools::edit::execute(&input).await.unwrap();
    assert!(result.contains("Edited"));

    // Verify
    let content = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "foo qux baz");
}
