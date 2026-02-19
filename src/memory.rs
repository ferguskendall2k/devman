use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

/// Search result from grep
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub file: String,
    pub line: u32,
    pub text: String,
}

/// Manages task-based memory files
pub struct MemoryManager {
    pub root: PathBuf,
}

impl MemoryManager {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Default memory root: .devman/memory/ in current directory
    pub fn default_root() -> PathBuf {
        PathBuf::from(".devman/memory")
    }

    /// Search memory files using grep -rni
    pub fn search(&self, query: &str) -> Vec<SearchResult> {
        let output = Command::new("grep")
            .args(["-rni", query])
            .arg(&self.root)
            .output();

        let output = match output {
            Ok(o) => o,
            Err(_) => return vec![],
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout
            .lines()
            .filter_map(|line| {
                // Format: file:line:text
                let mut parts = line.splitn(3, ':');
                let file = parts.next()?.to_string();
                let line_num: u32 = parts.next()?.parse().ok()?;
                let text = parts.next()?.to_string();
                Some(SearchResult {
                    file,
                    line: line_num,
                    text,
                })
            })
            .collect()
    }

    /// Read a memory file (path relative to memory root)
    pub fn read_file(&self, path: &str) -> Result<String> {
        let full_path = self.root.join(path);
        std::fs::read_to_string(&full_path)
            .with_context(|| format!("reading memory file: {}", full_path.display()))
    }

    /// Write/create a memory file (path relative to memory root)
    pub fn write_file(&self, path: &str, content: &str) -> Result<()> {
        let full_path = self.root.join(path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&full_path, content)
            .with_context(|| format!("writing memory file: {}", full_path.display()))
    }

    /// Append to a memory file
    pub fn append_file(&self, path: &str, content: &str) -> Result<()> {
        let full_path = self.root.join(path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&full_path)
            .with_context(|| format!("opening memory file for append: {}", full_path.display()))?;
        file.write_all(content.as_bytes())?;
        Ok(())
    }

    /// Load a task by name or alias from INDEX.md
    pub fn load_task(&self, name_or_alias: &str) -> Result<String> {
        let index_content = self.read_file("INDEX.md")
            .with_context(|| "INDEX.md not found — no tasks registered yet")?;

        let query = name_or_alias.to_lowercase();

        // Parse INDEX.md lines looking for task entries
        // Expected format: - [name](tasks/file.md) | alias: ... | status: ...
        for line in index_content.lines() {
            let lower = line.to_lowercase();
            if lower.contains(&query) {
                // Extract file path from markdown link: [name](path)
                if let Some(start) = line.find("](") {
                    if let Some(end) = line[start + 2..].find(')') {
                        let task_path = &line[start + 2..start + 2 + end];
                        return self.read_file(task_path);
                    }
                }
            }
        }

        anyhow::bail!("Task not found: {name_or_alias}")
    }

    /// Create a new task file from template, update INDEX.md
    pub fn create_task(&self, name: &str, _template_type: &str) -> Result<String> {
        let slug = name
            .to_lowercase()
            .replace(' ', "-")
            .replace(|c: char| !c.is_alphanumeric() && c != '-', "");

        let filename = format!("tasks/{slug}.md");

        let content = format!(
            r#"# {name}

## Overview
(describe the task)

## Status: NOT STARTED

## Key Decisions
- (none yet)

## Files
- (none yet)

## Current State
(nothing yet)

## TODO
- [ ] (first step)

## Notes
(none yet)
"#
        );

        self.write_file(&filename, &content)?;

        // Update INDEX.md
        let index_entry = format!("- [{name}]({filename}) | status: NOT STARTED\n");
        self.append_file("INDEX.md", &index_entry)?;

        Ok(format!("Created task: {filename}"))
    }

    /// Update a task's entry in INDEX.md
    pub fn update_index(&self, task_name: &str, status: &str, summary: &str) -> Result<()> {
        let index_path = self.root.join("INDEX.md");
        let content = std::fs::read_to_string(&index_path).unwrap_or_default();

        let query = task_name.to_lowercase();
        let mut found = false;
        let mut new_lines: Vec<String> = Vec::new();

        for line in content.lines() {
            if line.to_lowercase().contains(&query) && line.contains("](") {
                // Replace this line
                // Extract the link part
                if let Some(start) = line.find("](") {
                    if let Some(end) = line[start + 2..].find(')') {
                        let path = &line[start + 2..start + 2 + end];
                        let name_start = line.find('[').unwrap_or(0) + 1;
                        let name_end = line.find(']').unwrap_or(line.len());
                        let name = &line[name_start..name_end];
                        new_lines.push(format!(
                            "- [{name}]({path}) | status: {status} | {summary}"
                        ));
                        found = true;
                        continue;
                    }
                }
            }
            new_lines.push(line.to_string());
        }

        if !found {
            anyhow::bail!("Task not found in INDEX.md: {task_name}");
        }

        let new_content = new_lines.join("\n") + "\n";
        if let Some(parent) = index_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&index_path, new_content)?;
        Ok(())
    }
}
