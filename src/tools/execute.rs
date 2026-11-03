use serde_json::{json, Value};
use std::time::Duration;
use crate::error::{AgentError, Result};
use crate::tools::{Tool, ToolClass, ToolContext, ToolOutcome};

pub struct ExecuteCommand;

#[async_trait::async_trait]
impl Tool for ExecuteCommand {
    fn name(&self) -> &'static str { "execute_command" }
    fn class(&self) -> ToolClass { ToolClass::Mutating }
    fn schema(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "execute_command",
                "description": "Run a shell command (sandboxed by SecurityGuard).",
                "parameters": {
                    "type": "object",
                    "properties": { "cmd": { "type": "string" } },
                    "required": ["cmd"]
                }
            }
        })
    }
    fn validate(&self, args: &Value) -> Result<()> {
        if args.get("cmd").and_then(|c| c.as_str()).is_none() {
            return Err(AgentError::ToolInvalidArgs {
                tool: "execute_command".into(), reason: "missing cmd".into() });
        }
        Ok(())
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutcome> {
        let cmd = args.get("cmd").and_then(|c| c.as_str())
            .ok_or_else(|| AgentError::ToolFailed { tool: "execute_command".into(), reason: "missing cmd".into() })?;
        ctx.security.check_command(cmd)?;

        let out = tokio::time::timeout(
            Duration::from_secs(ctx.command_timeout_s),
            tokio::process::Command::new("sh").arg("-c").arg(cmd).output(),
        ).await;

        match out {
            Err(_) => Err(AgentError::ToolFailed {
                tool: "execute_command".into(),
                reason: format!("timeout after {}s", ctx.command_timeout_s),
            }),
            Ok(Err(e)) => Err(AgentError::ToolFailed {
                tool: "execute_command".into(), reason: e.to_string(),
            }),
            Ok(Ok(output)) => {
                let mut s = String::from_utf8_lossy(&output.stdout).to_string();
                if !output.stderr.is_empty() {
                    s.push_str("\n[stderr]\n");
                    s.push_str(&String::from_utf8_lossy(&output.stderr));
                }
                if s.len() > ctx.max_output {
                    s.truncate(ctx.max_output);
                    s.push_str("\n[...truncated]");
                }
                Ok(ToolOutcome { output: s, ok: output.status.success() })
            }
        }
    }
}
