use serde_json::{json, Value};
use crate::error::{AgentError, Result};
use crate::tools::{Tool, ToolClass, ToolContext, ToolOutcome};

pub struct WriteFile;

#[async_trait::async_trait]
impl Tool for WriteFile {
    fn name(&self) -> &'static str { "write_file" }
    fn class(&self) -> ToolClass { ToolClass::Mutating }
    fn schema(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "write_file",
                "description": "Atomically write a file.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "content": { "type": "string" }
                    },
                    "required": ["path", "content"]
                }
            }
        })
    }
    fn validate(&self, args: &Value) -> Result<()> {
        if args.get("path").and_then(|p| p.as_str()).is_none() {
            return Err(AgentError::ToolInvalidArgs {
                tool: "write_file".into(), reason: "missing path".into() });
        }
        if args.get("content").and_then(|p| p.as_str()).is_none() {
            return Err(AgentError::ToolInvalidArgs {
                tool: "write_file".into(), reason: "missing content".into() });
        }
        Ok(())
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutcome> {
        let path = args.get("path").and_then(|p| p.as_str())
            .ok_or_else(|| AgentError::ToolFailed { tool: "write_file".into(), reason: "missing path".into() })?;
        let content = args.get("content").and_then(|c| c.as_str())
            .ok_or_else(|| AgentError::ToolFailed { tool: "write_file".into(), reason: "missing content".into() })?;
        let p = std::path::Path::new(path);
        ctx.security.check_path(p)?;

        let tmp = p.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
        tokio::fs::write(&tmp, content).await
            .map_err(|e| AgentError::ToolFailed { tool: "write_file".into(), reason: e.to_string() })?;
        tokio::fs::rename(&tmp, p).await
            .map_err(|e| AgentError::ToolFailed { tool: "write_file".into(), reason: e.to_string() })?;

        Ok(ToolOutcome { output: format!("wrote {} bytes to {}", content.len(), p.display()), ok: true })
    }
}
