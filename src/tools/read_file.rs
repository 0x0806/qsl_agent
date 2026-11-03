use serde_json::{json, Value};
use crate::error::{AgentError, Result};
use crate::tools::{Tool, ToolClass, ToolContext, ToolOutcome};

pub struct ReadFile;

#[async_trait::async_trait]
impl Tool for ReadFile {
    fn name(&self) -> &'static str { "read_file" }
    fn class(&self) -> ToolClass { ToolClass::ReadOnly }
    fn schema(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read a UTF-8 text file.",
                "parameters": {
                    "type": "object",
                    "properties": { "path": { "type": "string" } },
                    "required": ["path"]
                }
            }
        })
    }
    fn validate(&self, args: &Value) -> Result<()> {
        if args.get("path").and_then(|p| p.as_str()).is_none() {
            return Err(AgentError::ToolInvalidArgs {
                tool: "read_file".into(), reason: "missing path".into() });
        }
        Ok(())
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutcome> {
        let path = args.get("path").and_then(|p| p.as_str())
            .ok_or_else(|| AgentError::ToolFailed { tool: "read_file".into(), reason: "missing path".into() })?;
        let p = std::path::Path::new(path);
        ctx.security.check_path(p)?;
        let text = tokio::fs::read_to_string(p).await
            .map_err(|e| AgentError::ToolFailed { tool: "read_file".into(), reason: e.to_string() })?;
        let out = if text.len() > ctx.max_output {
            format!("{}\n[...truncated {} bytes]", &text[..ctx.max_output], text.len() - ctx.max_output)
        } else { text };
        Ok(ToolOutcome { output: out, ok: true })
    }
}
