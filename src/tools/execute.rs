async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutcome> {
    let cmd = args.get("cmd").and_then(|c| c.as_str())
        .ok_or_else(|| AgentError::ToolFailed {
            tool: "execute_command".into(), reason: "missing cmd".into() })?;

    // New: classify first
    match ctx.security.classify_command(cmd) {
        crate::security::ApprovalRequirement::Denied { reason } => {
            return Err(AgentError::ToolDenied {
                tool: "execute_command".into(), reason,
            });
        }
        crate::security::ApprovalRequirement::NeedsApproval { reason } => {
            // Wire this to your approval.rs. Signature below assumes:
            //   approval::request(reason: &str, cmd: &str) -> Result<bool>
            let approved = ctx.approval
                .as_ref()
                .ok_or_else(|| AgentError::ToolDenied {
                    tool: "execute_command".into(),
                    reason: format!("needs approval but no approver wired: {reason}"),
                })?
                .request(&reason, cmd)
                .await?;
            if !approved {
                return Err(AgentError::ToolDenied {
                    tool: "execute_command".into(),
                    reason: "user declined".into(),
                });
            }
        }
        crate::security::ApprovalRequirement::Allowed => {}
    }

    // ... rest of the execute body unchanged ...
}
