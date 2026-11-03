use colored::*;

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

    pub fn render_tool_call(name: &str, args: &str) {
        println!("{}", format!("╭── [TOOL CALL: {}] ────────────────────────────────────", name).bold().cyan());
        for line in args.lines().take(6) {
            println!("│  {}", line.dimmed());
        }
        if args.lines().count() > 6 {
            println!("│  {}", format!("... ({} lines total)", args.lines().count()).dimmed());
        }
        println!("{}", "╰───────────────────────────────────────────────────────────────────".bold().cyan());
    }

    pub fn render_tool_result(name: &str, output: &str) {
        println!("{}", format!("╭── [RESULT: {}] ───────────────────────────────────────", name).bold().green());
        let trimmed = output.trim();
        for line in trimmed.lines().take(6) {
            println!("│  {}", line.bright_black());
        }
        let total_lines = trimmed.lines().count();
        if total_lines > 6 {
            println!(
                "│  {}",
                format!("... ({} more lines, {} total bytes)", total_lines - 6, output.len()).dimmed()
            );
        }
        println!("{}", "╰───────────────────────────────────────────────────────────────────".bold().green());
    }

    pub fn render_tool_error(name: &str, error: &str) {
        println!("{}", format!("╭── [BLOCKED / ERROR: {}] ────────────────────────", name).bold().red());
        for line in error.lines() {
            println!("│  {}", line.bright_red());
        }
        println!("{}", "╰───────────────────────────────────────────────────────────────────".bold().red());
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
