use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
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

    /// Validate that a path stays within the memory root (prevent traversal)
    fn safe_path(&self, path: &str) -> Result<PathBuf> {
        let full_path = self.root.join(path);
        let canonical = full_path
            .canonicalize()
            .or_else(|_| {
                // File may not exist yet — canonicalize parent
                if let Some(parent) = full_path.parent() {
                    std::fs::create_dir_all(parent).ok();
                    let canon_parent = parent.canonicalize()?;
                    Ok(canon_parent.join(full_path.file_name().unwrap_or_default()))
                } else {
                    Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid path"))
                }
            })?;
        let canon_root = self.root.canonicalize().unwrap_or_else(|_| self.root.clone());
        if !canonical.starts_with(&canon_root) {
            anyhow::bail!("Path traversal blocked: {} escapes memory root", path);
        }
        Ok(canonical)
    }

    /// Read a memory file (path relative to memory root)
    pub fn read_file(&self, path: &str) -> Result<String> {
        let full_path = self.safe_path(path)?;
        std::fs::read_to_string(&full_path)
            .with_context(|| format!("reading memory file: {}", full_path.display()))
    }

    /// Write/create a memory file (path relative to memory root)
    pub fn write_file(&self, path: &str, content: &str) -> Result<()> {
        let full_path = self.safe_path(path)?;
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&full_path, content)
            .with_context(|| format!("writing memory file: {}", full_path.display()))
    }

    /// Append to a memory file
    pub fn append_file(&self, path: &str, content: &str) -> Result<()> {
        let full_path = self.safe_path(path)?;
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

    /// Get storage for a specific task (scoped to tasks/<slug>/storage/)
    pub fn task_storage(&self, task_slug: &str) -> TaskStorage {
        let storage_root = self.root.join("tasks").join(task_slug).join("storage");
        TaskStorage::new(storage_root, false)
    }

    /// Get global storage (manager-level, can see all task storage)
    pub fn global_storage(&self) -> TaskStorage {
        TaskStorage::new(self.root.clone(), true)
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

/// Scoped file storage for tasks.
/// - Sub-agents get a TaskStorage rooted at their task's storage dir (can only see own files)
/// - Manager gets a global TaskStorage that can traverse all task storage dirs
pub struct TaskStorage {
    pub root: PathBuf,
    /// If true, this is manager-level (can access tasks/*/storage/ via task prefix)
    pub global: bool,
}

impl TaskStorage {
    pub fn new(root: PathBuf, global: bool) -> Self {
        Self { root, global }
    }

    /// Resolve a path, ensuring it stays within scope
    fn resolve(&self, path: &str) -> Result<PathBuf> {
        let full = self.root.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let canonical = full.canonicalize().or_else(|_| {
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).ok();
                let cp = parent.canonicalize()?;
                Ok(cp.join(full.file_name().unwrap_or_default()))
            } else {
                Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid path"))
            }
        })?;
        let canon_root = self.root.canonicalize().unwrap_or_else(|_| self.root.clone());
        if !canonical.starts_with(&canon_root) {
            anyhow::bail!("Path traversal blocked: {path} escapes storage root");
        }
        Ok(canonical)
    }

    /// Write a file to storage (text or base64 binary)
    pub fn write_file(&self, path: &str, content: &str, base64: bool) -> Result<()> {
        let full = self.resolve(path)?;
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if base64 {
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(content)
                .with_context(|| "invalid base64 content")?;
            std::fs::write(&full, bytes)?;
        } else {
            std::fs::write(&full, content)?;
        }
        Ok(())
    }

    /// Read a file from storage
    pub fn read_file(&self, path: &str) -> Result<String> {
        let full = self.resolve(path)?;
        // Try as text first; if invalid UTF-8, return base64
        match std::fs::read_to_string(&full) {
            Ok(s) => Ok(s),
            Err(_) => {
                use base64::Engine;
                let bytes = std::fs::read(&full)
                    .with_context(|| format!("reading storage file: {}", full.display()))?;
                Ok(format!(
                    "[base64] {}",
                    base64::engine::general_purpose::STANDARD.encode(&bytes)
                ))
            }
        }
    }

    /// List files in storage (optionally under a subdirectory)
    pub fn list_files(&self, subdir: Option<&str>) -> Result<Vec<String>> {
        let dir = match subdir {
            Some(s) => self.resolve(s)?,
            None => {
                std::fs::create_dir_all(&self.root).ok();
                self.root.canonicalize().unwrap_or_else(|_| self.root.clone())
            }
        };
        if !dir.exists() {
            return Ok(vec![]);
        }
        let canon_root = self.root.canonicalize().unwrap_or_else(|_| self.root.clone());
        let mut files = Vec::new();
        collect_files(&dir, &canon_root, &mut files)?;
        Ok(files)
    }

    /// Delete a file from storage
    pub fn delete_file(&self, path: &str) -> Result<()> {
        let full = self.resolve(path)?;
        if full.is_dir() {
            std::fs::remove_dir_all(&full)?;
        } else {
            std::fs::remove_file(&full)?;
        }
        Ok(())
    }

    /// Get storage usage (total bytes, file count)
    pub fn usage(&self) -> Result<(u64, usize)> {
        let files = self.list_files(None)?;
        let mut total: u64 = 0;
        let canon_root = self.root.canonicalize().unwrap_or_else(|_| self.root.clone());
        for f in &files {
            if let Ok(meta) = std::fs::metadata(canon_root.join(f)) {
                total += meta.len();
            }
        }
        Ok((total, files.len()))
    }
}

fn collect_files(dir: &Path, root: &Path, out: &mut Vec<String>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, root, out)?;
        } else {
            let rel = path.strip_prefix(root).unwrap_or(&path);
            out.push(rel.to_string_lossy().to_string());
        }
    }
    Ok(())
}
