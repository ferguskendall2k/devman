use anyhow::Result;
use serde_json::json;
use std::io::Write;
use std::path::PathBuf;
use tokio::process::Command;

use crate::types::ToolDefinition;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "apply_patch".into(),
        description: "Apply a unified diff patch to one or more files".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "patch": {
                    "type": "string",
                    "description": "The unified diff content to apply"
                },
                "workdir": {
                    "type": "string",
                    "description": "Working directory (optional, defaults to cwd)"
                }
            },
            "required": ["patch"]
        }),
    }
}

pub async fn execute(input: &serde_json::Value) -> Result<String> {
    let patch_content = input["patch"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'patch' field"))?;

    // Write patch to temp file
    let tmp_path = PathBuf::from(format!("/tmp/devman-patch-{}.patch", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp_path)?;
        f.write_all(patch_content.as_bytes())?;
        f.flush()?;
    }

    let mut cmd = Command::new("patch");
    cmd.args(["-p1", "--forward"])
        .stdin(std::process::Stdio::from(std::fs::File::open(&tmp_path)?));

    if let Some(workdir) = input["workdir"].as_str() {
        cmd.current_dir(workdir);
    }

    let output = cmd.output().await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut result = String::new();
    if !stdout.is_empty() {
        result.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(&stderr);
    }

    if output.status.success() {
        if result.is_empty() {
            result = "Patch applied successfully (no output).".into();
        }
    } else {
        result.push_str(&format!(
            "\nPatch failed with exit code: {}",
            output.status.code().unwrap_or(-1)
        ));
    }

    // Clean up temp file
    let _ = std::fs::remove_file(&tmp_path);

    Ok(result)
}
