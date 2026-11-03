//! Command and path sandboxing.
//!
//! Returns an `ApprovalRequirement` for commands so the caller can decide
//! whether to auto-approve, prompt via `approval.rs`, or reject outright.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use regex::RegexSet;

use crate::config::{Config, ToolsConfig};
use crate::error::{AgentError, Result};

/// Clearly-dangerous patterns — always denied, no approval prompt.
static DENY: LazyLock<RegexSet> = LazyLock::new(|| {
    RegexSet::new([
        r"rm\s+(-[a-zA-Z]*[rf][a-zA-Z]*\s+)+/(?!tmp)",       // rm -rf /
        r"rm\s+(-[a-zA-Z]*[rf][a-zA-Z]*\s+)+~(?:/|$)",       // rm -rf ~
        r":\(\)\s*\{\s*:\|\:&\s*\};",                        // fork bomb
        r"dd\s+.*of=/dev/(sd|nvme|mmcblk|hd)",               // raw disk write
        r">\s*/dev/(sd|nvme|mmcblk)",                        // raw disk redirect
        r"mkfs\.",                                           // format
        r"\bshutdown\b|\breboot\b|\bhalt\b|\bpoweroff\b",
        r"\bchmod\s+(-R\s+)?777\s+/",
        r"\bchown\s+(-R\s+)?\S+\s+/(\s|$)",
        r"\bcurl\s+[^|]*\|\s*(ba)?sh",                       // curl | sh
        r"\bwget\s+[^|]*\|\s*(ba)?sh",
        r"\biptables\s+-F",                                  // firewall flush
        r"\bnc\s+-[a-z]*l",                                  // netcat listener
    ]).expect("deny regex set")
});

/// Worth a confirmation prompt — routed to `approval.rs` if present.
static ASK: LazyLock<RegexSet> = LazyLock::new(|| {
    RegexSet::new([
        r"\bsudo\b",
        r"\bsu\s+-",
        r"\brm\s+",                       // any rm at all
        r"\bmv\s+/",                      // moving from root
        r"\bcp\s+/",                      // copying from root
        r"\bgit\s+push\b",
        r"\bgit\s+reset\s+--hard",
        r"\bgit\s+clean\s+-[a-z]*f",
        r"\bcargo\s+publish\b",
        r"\bnpm\s+publish\b",
        r"\bdocker\s+(rm|rmi|system)\s+",
        r"\bkillall\b|\bpkill\b",
        r"\bsystemctl\s+(stop|disable|mask)\b",
    ]).expect("ask regex set")
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalRequirement {
    /// Safe — auto-execute.
    Allowed,
    /// Needs confirmation — pass to `approval.rs` for user prompt.
    NeedsApproval { reason: String },
    /// Blocked — never execute, even with approval.
    Denied { reason: String },
}

pub struct SecurityGuard {
    cfg: ToolsConfig,
    allowed_roots: Vec<PathBuf>,
}

impl SecurityGuard {
    pub fn new(cfg: &Config) -> Result<Self> {
        let mut roots = Vec::with_capacity(cfg.tools.allowed_paths.len());
        for p in &cfg.tools.allowed_paths {
            // Canonicalize once at startup; nonexistent roots are a config error
            let canonical = p.canonicalize().map_err(|e| AgentError::Config(format!(
                "allowed_paths entry {} not resolvable: {e}", p.display())))?;
            roots.push(canonical);
        }
        Ok(Self { cfg: cfg.tools.clone(), allowed_roots: roots })
    }

    /// Filesystem path check — must be inside at least one allowed root.
    pub fn check_path(&self, path: &Path) -> Result<()> {
        // If the file doesn't exist yet (write_file to a new file), canonicalize
        // the parent instead.
        let canonical = match path.canonicalize() {
            Ok(p) => p,
            Err(_) => {
                let parent = path.parent().unwrap_or_else(|| Path::new("."));
                let canon_parent = parent.canonicalize().unwrap_or_else(|_| parent.to_path_buf());
                canon_parent.join(path.file_name().unwrap_or_default())
            }
        };
        let ok = self.allowed_roots.iter().any(|root| canonical.starts_with(root));
        if !ok {
            return Err(AgentError::ToolDenied {
                tool: "path".into(),
                reason: format!("outside allowed roots: {}", canonical.display()),
            });
        }
        Ok(())
    }

    /// Command check. Returns a requirement rather than just Ok/Err so
    /// `approval.rs` can handle the "ask" case.
    pub fn classify_command(&self, cmd: &str) -> ApprovalRequirement {
        if !self.cfg.allow_shell {
            return ApprovalRequirement::Denied {
                reason: "shell execution disabled in config".into(),
            };
        }

        // Deny takes precedence over ask
        if DENY.is_match(cmd) {
            let hits: Vec<String> = DENY.matches(cmd).iter()
                .map(|i| DENY.patterns()[i].to_string())
                .collect();
            return ApprovalRequirement::Denied {
                reason: format!("matched dangerous pattern(s): {hits:?}"),
            };
        }

        if ASK.is_match(cmd) {
            let hits: Vec<String> = ASK.matches(cmd).iter()
                .map(|i| ASK.patterns()[i].to_string())
                .collect();
            return ApprovalRequirement::NeedsApproval {
                reason: format!("matched sensitive pattern(s): {hits:?}"),
            };
        }

        ApprovalRequirement::Allowed
    }

    /// Convenience wrapper for callers that don't have an approval hook —
    /// treats `NeedsApproval` as `Allowed` (the old behavior).
    pub fn check_command(&self, cmd: &str) -> Result<()> {
        match self.classify_command(cmd) {
            ApprovalRequirement::Allowed | ApprovalRequirement::NeedsApproval { .. } => Ok(()),
            ApprovalRequirement::Denied { reason } => Err(AgentError::ToolDenied {
                tool: "execute_command".into(),
                reason,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> SecurityGuard {
        let cfg: Config = toml::from_str(r#"
            state_dir = "/tmp/qsl-test"
            [provider]
            base_url = "http://localhost:11434"
            model = "test"
            tier = "small"
            request_timeout_s = 10
            max_retries = 1
            [agent]
            max_steps_per_turn = 1
            max_turn_seconds = 10
            max_concurrent_tools = 1
            tool_timeout_s = 10
            system_prompt = "test"
            temperature = 0.0
            [tools]
            max_output_bytes = 1024
            command_timeout_s = 5
            allow_shell = true
            allowed_paths = ["/tmp"]
            [health]
            heartbeat_interval_s = 30
            stall_threshold_s = 60
            [memory]
            notes_file = "/tmp/qsl-test-mem.md"
            bm25_top_k = 4
            [cost]
            input_usd_per_million = 0.0
            output_usd_per_million = 0.0
        "#).unwrap();
        SecurityGuard::new(&cfg).unwrap()
    }

    #[test]
    fn denies_fork_bomb() {
        let g = guard();
        assert!(matches!(g.classify_command(":(){ :|:& };:"), ApprovalRequirement::Denied { .. }));
    }

    #[test]
    fn denies_rm_root() {
        let g = guard();
        assert!(matches!(g.classify_command("rm -rf /"), ApprovalRequirement::Denied { .. }));
    }

    #[test]
    fn asks_for_sudo() {
        let g = guard();
        assert!(matches!(g.classify_command("sudo apt install foo"), ApprovalRequirement::NeedsApproval { .. }));
    }

    #[test]
    fn allows_ls() {
        let g = guard();
        assert_eq!(g.classify_command("ls -la"), ApprovalRequirement::Allowed);
    }

    #[test]
    fn path_inside_root_allowed() {
        let g = guard();
        assert!(g.check_path(Path::new("/tmp/foo.txt")).is_ok());
    }

    #[test]
    fn path_outside_root_denied() {
        let g = guard();
        assert!(g.check_path(Path::new("/etc/passwd")).is_err());
    }
}
