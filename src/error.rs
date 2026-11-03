use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("provider {provider} unreachable: {source}")]
    LlmUnreachable { provider: String, #[source] source: reqwest::Error },

    #[error("provider {provider} returned HTTP {status}: {body}")]
    LlmStatus { provider: String, status: u16, body: String },

    #[error("provider {provider} circuit open (retry after {retry_after_s}s)")]
    ProviderDegraded { provider: String, retry_after_s: u64 },

    #[error("context overflow: {used}/{limit}")]
    ContextOverflow { used: usize, limit: usize },

    #[error("tool {tool} failed: {reason}")]
    ToolFailed { tool: String, reason: String },

    #[error("tool {tool} denied by security: {reason}")]
    ToolDenied { tool: String, reason: String },

    #[error("tool {tool} argument invalid: {reason}")]
    ToolInvalidArgs { tool: String, reason: String },

    #[error("turn exceeded {seconds}s deadline")]
    TurnDeadline { seconds: u64 },

    #[error("session persisted state is corrupt: {reason}")]
    SessionCorrupt { reason: String },

    #[error("config error: {0}")]
    Config(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("cancelled")]
    Cancelled,
}

impl AgentError {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            AgentError::LlmUnreachable { .. }
                | AgentError::ProviderDegraded { .. }
                | AgentError::LlmStatus { status: 429 | 500..=599, .. }
                | AgentError::Io(_)
        )
    }
}

pub type Result<T> = std::result::Result<T, AgentError>;
