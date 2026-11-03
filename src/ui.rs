use colored::*;
use serde_json::Value;

/// Terminal styling helper providing modern, minimal "CLI CSS" with clean unicode box-drawing frames
pub struct UI;

impl UI {
    pub const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    /// Render a clean, minimal, perfectly aligned ASCII/Unicode banner (no emojis)
    pub fn render_banner(
        provider: &str,
        model: &str,
        vault: &str,
        approval_mode: &str,
        skills_count: usize,
    ) {
        let inner_width = 76;
        let sep = "─".repeat(inner_width);

        println!();
        println!("{}", format!("╭{}╮", sep).bright_cyan());

        // Header: dynamic build version without emojis
        let title_raw = format!("QSL AGENT — AUTONOMOUS LOCAL INTELLIGENCE (v{})", Self::VERSION);
        let title_colored = title_raw.bold().bright_green();
        Self::print_row(&title_raw, &title_colored.to_string(), inner_width);

        // Subheader badges: minimal, sleek
        let badges_raw = "[100% LOCAL]  [ZERO CLOUD]  [OBSIDIAN CORTEX]  [SECURITY GUARD]";
        let badges_colored = format!(
            "{}  {}  {}  {}",
            "[100% LOCAL]".green().bold(),
            "[ZERO CLOUD]".blue().bold(),
            "[OBSIDIAN CORTEX]".magenta().bold(),
            "[SECURITY GUARD]".red().bold()
        );
        Self::print_row(badges_raw, &badges_colored, inner_width);

        println!("{}", format!("├{}┤", sep).bright_cyan());

        // Engine & Endpoint
        let engine_raw = format!("Engine:    {}", provider);
        let engine_col = format!("{:<10} {}", "Engine:".bold().white(), provider.cyan().bold());
        Self::print_row(&engine_raw, &engine_col, inner_width);

        // Active Model
        let model_display = if model.is_empty() {
            "Auto-detecting...".to_string()
        } else {
            model.to_string()
        };
        let model_raw = format!("Model:     {}", model_display);
        let model_col = format!("{:<10} {}", "Model:".bold().white(), model_display.yellow().bold());
        Self::print_row(&model_raw, &model_col, inner_width);

        // Hardware & Parameter Tier
        let tier = crate::provider::ModelTier::from_model_name(&model_display);
        let tier_raw = format!("Tier:      {}", tier.display_label());
        let tier_col = format!("{:<10} {}", "Tier:".bold().white(), tier.display_label().bright_cyan());
        Self::print_row(&tier_raw, &tier_col, inner_width);

        // Approval Mode
        let app_raw = format!("Approval:  {}", approval_mode);
        let app_col = format!("{:<10} {}", "Approval:".bold().white(), approval_mode.green().bold());
        Self::print_row(&app_raw, &app_col, inner_width);

        // Active Skills Count
        let skills_raw = format!("Skills:    {} operational skills active", skills_count);
        let skills_col = format!(
            "{:<10} {}",
            "Skills:".bold().white(),
            format!("{} operational skills active", skills_count).magenta().bold()
        );
        Self::print_row(&skills_raw, &skills_col, inner_width);

        // Obsidian Vault Path
        let vault_display = if vault.chars().count() > 60 {
            let truncated: String = vault.chars().take(57).collect();
            format!("{}...", truncated)
        } else {
            vault.to_string()
        };
        let vault_raw = format!("Vault:     {}", vault_display);
        let vault_col = format!("{:<10} {}", "Vault:".bold().white(), vault_display.bright_black());
        Self::print_row(&vault_raw, &vault_col, inner_width);

        // Org Attribution
        let org_raw = "Org:       Quantum Secular Labs (https://quantumseclabs.com/)";
        let org_col = format!("{:<10} {}", "Org:".bold().white(), "Quantum Secular Labs (https://quantumseclabs.com/)".cyan());
        Self::print_row(&org_raw, &org_col, inner_width);

        println!("{}", format!("├{}┤", sep).bright_cyan());

        // Tip footer
        let tip_raw = "Tip: Type /help for command manual or enter any autonomous task prompt.";
        let tip_col = format!(
            "{} Type {} for command manual or enter any autonomous task prompt.",
            "Tip:".yellow().bold(),
            "/help".cyan().bold()
        );
        Self::print_row(tip_raw, &tip_col, inner_width);

        println!("{}", format!("╰{}╯", sep).bright_cyan());
        println!();
    }

    /// Print a single box row where padding is strictly computed from uncolored text character count
    fn print_row(raw_text: &str, colored_text: &str, inner_width: usize) {
        let char_count = raw_text.chars().count();
        let content_width = inner_width.saturating_sub(2);
        let pad = content_width.saturating_sub(char_count);
        println!("│ {}{} │", colored_text, " ".repeat(pad));
    }

    /// Render structured reasoning/thought process emitted by reasoning models (e.g. DeepSeek-R1, Qwen35)
    pub fn render_reasoning(thought: &str) {
        let trimmed = thought.trim();
        if trimmed.is_empty() {
            return;
        }

        println!("{}", "╭── [REASONING & COGNITIVE PLANNING] ───────────────────────────────────".bold().bright_magenta());
        let lines: Vec<&str> = trimmed.lines().collect();
        let max_display = 14;
        for line in lines.iter().take(max_display) {
            println!("│  {}", line.bright_black());
        }
        if lines.len() > max_display {
            println!(
                "│  {}",
                format!("... ({} additional reasoning steps omitted for brevity)", lines.len() - max_display).dimmed()
            );
        }
        println!("{}", "╰───────────────────────────────────────────────────────────────────────".bold().bright_magenta());
    }

    /// Format tool call arguments into clean, structured key-value pairs rather than dumping raw JSON
    pub fn format_tool_details(name: &str, raw_args: &str) -> Vec<(String, String)> {
        let mut details = Vec::new();
        if let Ok(val) = serde_json::from_str::<Value>(raw_args) {
            match name {
                "execute_command" => {
                    if let Some(cmd) = val.get("command").and_then(|v| v.as_str()) {
                        details.push(("Command".to_string(), cmd.to_string()));
                    }
                }
                "write_file" | "append_file" => {
                    let action_label = if name == "append_file" { "Appended" } else { "Payload" };
                    if let Some(path) = val.get("path").and_then(|v| v.as_str()) {
                        details.push(("Target".to_string(), path.to_string()));
                    }
                    if let Some(content) = val.get("content").and_then(|v| v.as_str()) {
                        details.push((
                            action_label.to_string(),
                            format!("{} bytes ({} lines)", content.len(), content.lines().count()),
                        ));
                    }
                }
                "read_file" => {
                    if let Some(path) = val.get("path").and_then(|v| v.as_str()) {
                        details.push(("Target".to_string(), path.to_string()));
                    }
                    let offset = val.get("offset").and_then(|v| v.as_u64()).unwrap_or(1);
                    let limit = val.get("limit").and_then(|v| v.as_u64()).unwrap_or(100);
                    details.push(("Range".to_string(), format!("lines {} to {}", offset, offset + limit)));
                }
                "edit_file" => {
                    if let Some(path) = val.get("path").and_then(|v| v.as_str()) {
                        details.push(("Target".to_string(), path.to_string()));
                    }
                    let target_len = val.get("target").and_then(|v| v.as_str()).map(|s| s.len()).unwrap_or(0);
                    let repl_len = val.get("replacement").and_then(|v| v.as_str()).map(|s| s.len()).unwrap_or(0);
                    details.push(("Diff".to_string(), format!("replace {} bytes with {} bytes", target_len, repl_len)));
                }
                "list_dir" => {
                    if let Some(path) = val.get("path").and_then(|v| v.as_str()) {
                        details.push(("Directory".to_string(), path.to_string()));
                    }
                }
                "obsidian_write" | "obsidian_append" => {
                    if let Some(f) = val.get("filename").and_then(|v| v.as_str()) {
                        details.push(("Note".to_string(), f.to_string()));
                    }
                    if let Some(content) = val.get("content").and_then(|v| v.as_str()) {
                        details.push(("Size".to_string(), format!("{} bytes", content.len())));
                    }
                }
                "obsidian_search" => {
                    if let Some(q) = val.get("query").and_then(|v| v.as_str()) {
                        details.push(("Query".to_string(), q.to_string()));
                    }
                }
                "skill_read" => {
                    if let Some(s) = val.get("skill_name").and_then(|v| v.as_str()) {
                        details.push(("Skill".to_string(), s.to_string()));
                    }
                }
                "skill_create" => {
                    if let Some(n) = val.get("name").and_then(|v| v.as_str()) {
                        details.push(("Name".to_string(), n.to_string()));
                    }
                    if let Some(d) = val.get("description").and_then(|v| v.as_str()) {
                        details.push(("Description".to_string(), d.to_string()));
                    }
                }
                _ => {
                    if let Some(obj) = val.as_object() {
                        for (k, v) in obj {
                            let val_str = match v {
                                Value::String(s) => {
                                    if s.len() > 60 {
                                        format!("{}... ({} bytes)", &s[..57], s.len())
                                    } else {
                                        s.clone()
                                    }
                                }
                                other => other.to_string(),
                            };
                            details.push((k.clone(), val_str));
                        }
                    }
                }
            }
        }

        if details.is_empty() {
            let clean = raw_args.trim().replace('\n', " ");
            let preview = if clean.len() > 70 {
                format!("{}...", &clean[..67])
            } else {
                clean
            };
            details.push(("Args".to_string(), preview));
        }

        details
    }

    /// Render clean, enterprise-grade action badges instead of dumping raw JSON
    pub fn render_tool_call(name: &str, args: &str) {
        println!("{}", format!("╭── [ACTION: {}] ───────────────────────────────────────────", name).bold().cyan());
        let details = Self::format_tool_details(name, args);
        for (k, v) in details {
            println!("│  {:<12} {}", format!("{}:", k).bold().white(), v.dimmed());
        }
        println!("{}", "╰───────────────────────────────────────────────────────────────────".bold().cyan());
    }

    /// Render tool output cleanly with line limits and byte metadata
    pub fn render_tool_result(name: &str, output: &str) {
        let trimmed = output.trim();
        let total_lines = trimmed.lines().count();
        let is_ok = !trimmed.starts_with("ERROR") && !trimmed.starts_with("DENIED");

        let header_badge = if is_ok {
            format!("╭── [RESULT: {}] [OK] ───────────────────────────────────────", name).bold().green()
        } else {
            format!("╭── [RESULT: {}] [WARN] ─────────────────────────────────────", name).bold().yellow()
        };

        println!("{}", header_badge);
        let max_lines = 6;
        for line in trimmed.lines().take(max_lines) {
            println!("│  {}", line.bright_black());
        }
        if total_lines > max_lines {
            println!(
                "│  {}",
                format!("... ({} more lines omitted, {} bytes total)", total_lines - max_lines, output.len()).dimmed()
            );
        }
        if is_ok {
            println!("{}", "╰───────────────────────────────────────────────────────────────────".bold().green());
        } else {
            println!("{}", "╰───────────────────────────────────────────────────────────────────".bold().yellow());
        }
    }

    /// Render explicit tool failure or security block
    pub fn render_tool_error(name: &str, error: &str) {
        println!("{}", format!("╭── [BLOCKED / ERROR: {}] ──────────────────────────────────", name).bold().red());
        for line in error.lines() {
            println!("│  {}", line.bright_red());
        }
        println!("{}", "╰───────────────────────────────────────────────────────────────────".bold().red());
    }

    /// Render the final completed response in an executive card container
    pub fn render_final_response(content: &str) {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return;
        }

        println!();
        println!("{}", "╭── [EXECUTIVE RESPONSE] ───────────────────────────────────────────────".bold().bright_green());
        println!("│");
        for line in trimmed.lines() {
            println!("│  {}", line);
        }
        println!("│");
        println!("{}", "╰───────────────────────────────────────────────────────────────────────".bold().bright_green());
        println!();
    }

    pub fn render_skills_table(skills: &[(String, String, Vec<String>)]) {
        println!();
        println!("{}", "╭───────────────────────┬──────────────────────────────┬────────────────────────────────╮".bright_magenta());
        println!(
            "│ {:<21} │ {:<28} │ {:<30} │",
            "Skill Name".bold().white(),
            "Tags".bold().white(),
            "Description".bold().white()
        );
        println!("{}", "├───────────────────────┼──────────────────────────────┼────────────────────────────────┤".bright_magenta());
        for (name, desc, tags) in skills {
            let tag_str = tags.join(", ");
            let tag_preview = if tag_str.len() > 28 {
                format!("{}...", &tag_str[..25])
            } else {
                tag_str
            };
            let desc_preview = if desc.len() > 30 {
                format!("{}...", &desc[..27])
            } else {
                desc.clone()
            };
            println!(
                "│ {:<21} │ {:<28} │ {:<30} │",
                name.cyan().bold(),
                tag_preview.dimmed(),
                desc_preview
            );
        }
        println!("{}", "╰───────────────────────┴──────────────────────────────┴────────────────────────────────╯".bright_magenta());
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_tool_details_write_file() {
        let args = r#"{"path":"src/test.rs","content":"fn main() {\n    println!(\"hello\");\n}\n"}"#;
        let details = UI::format_tool_details("write_file", args);
        assert_eq!(details.len(), 2);
        assert_eq!(details[0].0, "Target");
        assert_eq!(details[0].1, "src/test.rs");
        assert_eq!(details[1].0, "Payload");
        assert!(details[1].1.contains("bytes"));
    }

    #[test]
    fn test_format_tool_details_execute_command() {
        let args = r#"{"command":"cargo test --lib"}"#;
        let details = UI::format_tool_details("execute_command", args);
        assert_eq!(details.len(), 1);
        assert_eq!(details[0].0, "Command");
        assert_eq!(details[0].1, "cargo test --lib");
    }

    #[test]
    fn test_format_tool_details_read_file() {
        let args = r#"{"path":"Cargo.toml","offset":10,"limit":50}"#;
        let details = UI::format_tool_details("read_file", args);
        assert_eq!(details.len(), 2);
        assert_eq!(details[0].0, "Target");
        assert_eq!(details[0].1, "Cargo.toml");
        assert_eq!(details[1].0, "Range");
        assert_eq!(details[1].1, "lines 10 to 60");
    }
}
