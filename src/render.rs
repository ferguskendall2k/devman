use colored::Colorize;
use regex::Regex;
use std::sync::OnceLock;

/// Render markdown text for terminal display
pub fn render_markdown(text: &str) -> String {
    let mut output = Vec::new();
    let mut in_code_block = false;
    let mut code_block_lines: Vec<String> = Vec::new();

    for line in text.lines() {
        if line.starts_with("```") {
            if in_code_block {
                // End code block
                for cl in &code_block_lines {
                    output.push(format!("  {}", cl.dimmed()));
                }
                code_block_lines.clear();
                in_code_block = false;
            } else {
                in_code_block = true;
            }
            continue;
        }

        if in_code_block {
            code_block_lines.push(line.to_string());
            continue;
        }

        // Headings
        if let Some(rest) = line.strip_prefix("### ") {
            output.push(format!("{}", rest.bold()));
        } else if let Some(rest) = line.strip_prefix("## ") {
            output.push(format!("{}", rest.bold().underline()));
        } else if let Some(rest) = line.strip_prefix("# ") {
            output.push(format!("{}", rest.bold().underline()));
        }
        // Blockquotes
        else if let Some(rest) = line.strip_prefix("> ") {
            output.push(format!("{} {}", "│".dimmed(), rest.dimmed()));
        }
        // Bullet lists
        else if let Some(rest) = line.strip_prefix("- ") {
            output.push(format!("  • {}", render_inline(rest)));
        } else if let Some(rest) = line.strip_prefix("* ") {
            output.push(format!("  • {}", render_inline(rest)));
        }
        // Normal line
        else {
            output.push(render_inline(line));
        }
    }

    // Handle unclosed code block
    if in_code_block {
        for cl in &code_block_lines {
            output.push(format!("  {}", cl.dimmed()));
        }
    }

    output.join("\n")
}

/// Render inline markdown formatting
fn render_inline(text: &str) -> String {
    static BOLD_RE: OnceLock<Regex> = OnceLock::new();
    static CODE_RE: OnceLock<Regex> = OnceLock::new();
    static LINK_RE: OnceLock<Regex> = OnceLock::new();

    let bold_re = BOLD_RE.get_or_init(|| Regex::new(r"\*\*(.+?)\*\*").unwrap());
    let code_re = CODE_RE.get_or_init(|| Regex::new(r"`([^`]+)`").unwrap());
    let link_re = LINK_RE.get_or_init(|| Regex::new(r"\[([^\]]+)\]\(([^)]+)\)").unwrap());

    let mut result = text.to_string();

    // Bold: **text**
    result = bold_re
        .replace_all(&result, |caps: &regex::Captures| {
            format!("{}", caps[1].bold())
        })
        .to_string();

    // Inline code: `text`
    result = code_re
        .replace_all(&result, |caps: &regex::Captures| {
            format!("{}", caps[1].cyan())
        })
        .to_string();

    // Links: [text](url)
    result = link_re
        .replace_all(&result, |caps: &regex::Captures| {
            format!("{} ({})", &caps[1], caps[2].dimmed())
        })
        .to_string();

    result
}
