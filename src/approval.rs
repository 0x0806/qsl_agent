//! Terminal-based approval hook for sensitive tool calls.
//!
//! Implements `crate::tools::ApprovalHook` and defers classification to
//! `SecurityGuard::classify_command()` so there's one source of truth.

use std::sync::Arc;
use async_trait::async_trait;
use crate::error::{AgentError, Result};
use crate::security::{ApprovalRequirement, SecurityGuard};
use crate::tools::ApprovalHook;

pub struct TerminalApprover {
    security: Arc<SecurityGuard>,
    auto_approve: bool,
    quiet: bool,
}

impl TerminalApprover {
    pub fn new(security: Arc<SecurityGuard>, auto_approve: bool) -> Self {
        Self { security, auto_approve, quiet: false }
    }

    pub fn quiet(mut self, quiet: bool) -> Self {
        self.quiet = quiet;
        self
    }
}

#[async_trait]
impl ApprovalHook for TerminalApprover {
    async fn request(&self, reason: &str, cmd: &str) -> Result<bool> {
        // Defense in depth: re-classify even though the caller already did.
        match self.security.classify_command(cmd) {
            ApprovalRequirement::Denied { reason: r } => {
                if !self.quiet {
                    eprintln!("\x1b[31mdenied\x1b[0m: {r}");
                }
                return Ok(false);
            }
            ApprovalRequirement::Allowed => return Ok(true),
            ApprovalRequirement::NeedsApproval { .. } => {}
        }

        if self.auto_approve {
            if !self.quiet {
                eprintln!("\x1b[33m⚠ auto-approved\x1b[0m: {cmd}");
            }
            return Ok(true);
        }

        let reason = reason.to_string();
        let cmd_display = cmd.to_string();
        let quiet = self.quiet;

        let approved = tokio::task::spawn_blocking(move || {
            use std::io::Write;
            if !quiet {
                eprintln!("\x1b[33m⚠ approval needed\x1b[0m: {reason}");
                eprintln!("  command: {cmd_display}");
                eprint!("  allow? [y/N] ");
                std::io::stderr().flush().ok();
            }
            let mut line = String::new();
            std::io::stdin().read_line(&mut line).ok();
            matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
        })
        .await
        .map_err(|e| AgentError::Io(std::io::Error::other(e.to_string())))?;

        Ok(approved)
    }
}

// ---------------------------------------------------------------------------
// Non-interactive auto-approver for tests / systemd / docker
// ---------------------------------------------------------------------------

pub struct AlwaysApprove;
pub struct AlwaysDeny;

#[async_trait]
impl ApprovalHook for AlwaysApprove {
    async fn request(&self, _reason: &str, _cmd: &str) -> Result<bool> { Ok(true) }
}

#[async_trait]
impl ApprovalHook for AlwaysDeny {
    async fn request(&self, _reason: &str, _cmd: &str) -> Result<bool> { Ok(false) }
}
