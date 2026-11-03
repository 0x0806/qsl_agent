//! Permission gate for mutating tools.
//!
//! | mode   | normal mutating tool          | risky command (security "ask" tier) |
//! |--------|-------------------------------|-------------------------------------|
//! | ask    | prompt (y/a/n/d), `a` sticks  | prompt every time                   |
//! | always | auto                          | prompt every time                   |
//! | strict | prompt every time             | prompt every time                   |
//! | yolo   | auto                          | auto                                |
//!
//! Hard-blocked commands never reach this gate. In non-interactive runs a
//! prompt cannot be shown: `ask`/`always` allow normal mutations, while risky
//! commands (and everything in `strict`) are denied with an explanation the
//! model can act on.

use serde_json::Value;
use std::collections::HashSet;

use crate::config::ApprovalMode;
use crate::ui::{self, ApprovalChoice};

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Approved,
    Denied(String),
}

pub struct ApprovalManager {
    pub mode: ApprovalMode,
    allowed: HashSet<String>,
    denied: HashSet<String>,
}

/// Mutating tools that only write into the agent's own memory area.
fn low_risk(tool: &str) -> bool {
    matches!(tool, "obsidian_append" | "skill_create")
}

impl ApprovalManager {
    pub fn new(mode: ApprovalMode) -> Self {
        Self {
            mode,
            allowed: HashSet::new(),
            denied: HashSet::new(),
        }
    }

    pub fn reset(&mut self) {
        self.allowed.clear();
        self.denied.clear();
    }

    pub fn set_mode(&mut self, mode: ApprovalMode) {
        self.mode = mode;
        self.reset();
    }

    fn needs_prompt(&self, tool: &str, risky: bool) -> bool {
        match self.mode {
            ApprovalMode::Strict => true,
            ApprovalMode::Always => risky,
            ApprovalMode::Ask => risky || !(self.allowed.contains(tool) || low_risk(tool)),
            ApprovalMode::Yolo => false,
        }
    }

    /// Whether `check` would show the interactive review card (which then
    /// replaces the ACTION card in the UI).
    pub fn will_prompt(&self, tool: &str, risky: bool, interactive: bool) -> bool {
        interactive
            && self.mode != ApprovalMode::Yolo
            && !self.denied.contains(tool)
            && self.needs_prompt(tool, risky)
    }

    /// `risk` is `Some(reason)` for commands in the security "ask" tier.
    pub fn check(
        &mut self,
        tool: &str,
        args: &Value,
        risk: Option<&str>,
        interactive: bool,
    ) -> Decision {
        if self.mode == ApprovalMode::Yolo {
            return Decision::Approved;
        }
        if self.denied.contains(tool) {
            return Decision::Denied(format!("'{tool}' was denied by the user for this session. Do not retry it; choose another approach or ask the user."));
        }
        let risky = risk.is_some();
        if !self.needs_prompt(tool, risky) {
            return Decision::Approved;
        }
        if !interactive {
            if !risky && self.mode != ApprovalMode::Strict {
                return Decision::Approved;
            }
            return Decision::Denied(format!(
                "this action needs interactive user approval ({}), which is unavailable in non-interactive mode. \
                 Use a safer alternative or explain to the user what to run manually.",
                risk.unwrap_or("strict approval mode")
            ));
        }
        let allow_session = self.mode == ApprovalMode::Ask && !risky;
        match ui::approval_prompt(tool, args, risk, allow_session) {
            ApprovalChoice::Once => {
                ui::info("Approved (once).");
                Decision::Approved
            }
            ApprovalChoice::Session => {
                self.allowed.insert(tool.to_string());
                ui::info(&format!("'{tool}' will auto-approve for this session."));
                Decision::Approved
            }
            ApprovalChoice::Deny => Decision::Denied("the user denied this action. Do not retry the same call; adjust the plan or ask the user.".into()),
            ApprovalChoice::DenySession => {
                self.denied.insert(tool.to_string());
                Decision::Denied(format!("the user denied '{tool}' for the rest of this session."))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn non_interactive_matrix() {
        let a = json!({});
        let mut m = ApprovalManager::new(ApprovalMode::Ask);
        assert_eq!(m.check("write_file", &a, None, false), Decision::Approved);
        assert!(matches!(
            m.check("execute_command", &a, Some("deletes files"), false),
            Decision::Denied(_)
        ));

        let mut m = ApprovalManager::new(ApprovalMode::Always);
        assert_eq!(
            m.check("execute_command", &a, None, false),
            Decision::Approved
        );
        assert!(matches!(
            m.check("execute_command", &a, Some("privilege escalation"), false),
            Decision::Denied(_)
        ));

        let mut m = ApprovalManager::new(ApprovalMode::Strict);
        assert!(matches!(
            m.check("write_file", &a, None, false),
            Decision::Denied(_)
        ));

        let mut m = ApprovalManager::new(ApprovalMode::Yolo);
        assert_eq!(
            m.check("execute_command", &a, Some("deletes files"), false),
            Decision::Approved
        );
    }

    #[test]
    fn low_risk_memory_writes_skip_prompt_in_ask_mode() {
        let mut m = ApprovalManager::new(ApprovalMode::Ask);
        assert_eq!(
            m.check("obsidian_append", &json!({}), None, true),
            Decision::Approved
        );
    }
}
