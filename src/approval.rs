use crate::ui::UI;
use colored::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{self, Write};

/// Three-tier permission model for tool execution safety
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalMode {
    /// Prompt user on mutating tools: allow once (y), allow always for session (a), or deny (n)
    Ask,
    /// Auto-approve all safe tools without prompting
    Always,
    /// Prompt on every mutating tool call, never auto-approve (highest security)
    Strict,
}

impl Default for ApprovalMode {
    fn default() -> Self {
        ApprovalMode::Ask
    }
}

impl std::fmt::Display for ApprovalMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApprovalMode::Ask => write!(f, "Ask"),
            ApprovalMode::Always => write!(f, "Always"),
            ApprovalMode::Strict => write!(f, "Strict"),
        }
    }
}

/// Manages tool execution permissions with session-scoped memory
pub struct ApprovalManager {
    pub mode: ApprovalMode,
    /// Tools the user has approved with "always" for this session
    pub always_allowed_tools: HashSet<String>,
    /// Tools the user has denied for this session
    pub denied_tools: HashSet<String>,
}

impl ApprovalManager {
    pub fn new(mode: ApprovalMode) -> Self {
        Self {
            mode,
            always_allowed_tools: HashSet::new(),
            denied_tools: HashSet::new(),
        }
    }

    /// Determine if a tool modifies state (files, commands, memory)
    pub fn is_mutating_tool(tool_name: &str) -> bool {
        matches!(
            tool_name,
            "execute_command"
                | "write_file"
                | "append_file"
                | "edit_file"
                | "obsidian_write"
                | "obsidian_append"
                | "skill_create"
        )
    }

    /// Check if the tool call is approved, prompting the user if necessary
    pub fn check_approval(
        &mut self,
        tool_name: &str,
        details: &str,
        is_interactive: bool,
    ) -> Result<(), String> {
        // Read-only tools never need approval
        if !Self::is_mutating_tool(tool_name) {
            return Ok(());
        }

        // If in Always mode, pass immediately
        if self.mode == ApprovalMode::Always {
            return Ok(());
        }

        // If user previously denied this tool for the session
        if self.denied_tools.contains(tool_name) {
            return Err(format!(
                "Tool '{}' was denied for this session. Use /approval to reset.",
                tool_name
            ));
        }

        // If user already allowed this tool for the session (Ask mode only)
        if self.always_allowed_tools.contains(tool_name) && self.mode != ApprovalMode::Strict {
            return Ok(());
        }

        // Non-interactive mode: allow in Ask mode, deny in Strict mode
        if !is_interactive {
            if self.mode == ApprovalMode::Strict {
                return Err(format!(
                    "Permission denied: Strict approval mode requires interactive approval for '{}'.",
                    tool_name
                ));
            }
            return Ok(());
        }

        // Interactive approval prompt
        println!();
        println!(
            "{}",
            "╭─── [PERMISSION REVIEW] ───────────────────────────────────────────╮"
                .yellow()
                .bold()
        );
        println!(
            "│  {}  {}",
            "Tool:".bold().white(),
            tool_name.bold().cyan()
        );
        let formatted_details = UI::format_tool_details(tool_name, details);
        for (k, v) in formatted_details {
            println!("│  {}  {}", format!("{}:", k).bold().white(), v.dimmed());
        }
        println!(
            "{}",
            "├───────────────────────────────────────────────────────────────────┤"
                .yellow()
        );
        println!(
            "│  {} Allow once  │  {} Always allow (session)  │  {} Deny",
            "[y]".green().bold(),
            "[a]".cyan().bold(),
            "[n]".red().bold()
        );
        println!(
            "│  {} Select {} to auto-approve subsequent steps without pausing",
            "Tip:".dimmed(),
            "[a]".cyan().bold()
        );
        println!(
            "{}",
            "╰───────────────────────────────────────────────────────────────────╯"
                .yellow()
        );
        print!("{}", "  permission > ".bold());
        let _ = io::stdout().flush();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            return Err("Failed to read user permission input.".to_string());
        }

        match input.trim().to_lowercase().as_str() {
            "y" | "yes" | "" => {
                println!("  {} Approved (once).", "[OK]".green());
                Ok(())
            }
            "a" | "always" => {
                self.always_allowed_tools.insert(tool_name.to_string());
                println!(
                    "  {} '{}' will auto-approve for this session.",
                    "[OK]".green(),
                    tool_name.bold()
                );
                Ok(())
            }
            "n" | "no" | "deny" => {
                self.denied_tools.insert(tool_name.to_string());
                Err(format!("Permission denied by user for tool '{}'.", tool_name))
            }
            _ => Err(format!(
                "Invalid input. Permission denied for tool '{}'.",
                tool_name
            )),
        }
    }

    /// Reset all session approvals and denials
    pub fn reset(&mut self) {
        self.always_allowed_tools.clear();
        self.denied_tools.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mutating_tools_detection() {
        assert!(ApprovalManager::is_mutating_tool("execute_command"));
        assert!(ApprovalManager::is_mutating_tool("write_file"));
        assert!(ApprovalManager::is_mutating_tool("edit_file"));
        assert!(ApprovalManager::is_mutating_tool("obsidian_write"));
        assert!(ApprovalManager::is_mutating_tool("skill_create"));
        assert!(!ApprovalManager::is_mutating_tool("read_file"));
        assert!(!ApprovalManager::is_mutating_tool("list_dir"));
        assert!(!ApprovalManager::is_mutating_tool("skill_list"));
    }

    #[test]
    fn test_always_mode_passes_without_interactive() {
        let mut mgr = ApprovalManager::new(ApprovalMode::Always);
        assert!(mgr.check_approval("execute_command", "ls", false).is_ok());
    }

    #[test]
    fn test_strict_mode_denies_non_interactive() {
        let mut mgr = ApprovalManager::new(ApprovalMode::Strict);
        assert!(mgr.check_approval("execute_command", "ls", false).is_err());
    }
}
