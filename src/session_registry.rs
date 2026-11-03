use std::path::{Path, PathBuf};
use serde::Serialize;
use crate::error::{AgentError, Result};

#[derive(Debug, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub modified: Option<String>,
}

pub struct SessionRegistry { root: PathBuf }

impl SessionRegistry {
    pub fn new(state_dir: &Path) -> Self {
        Self { root: state_dir.join("sessions") }
    }

    pub async fn list(&self) -> Result<Vec<SessionInfo>> {
        let mut out = Vec::new();
        let mut rd = match tokio::fs::read_dir(&self.root).await {
            Ok(r) => r,
            Err(_) => return Ok(out),
        };
        while let Some(entry) = rd.next_entry().await? {
            let path = entry.path();
            let events = path.join("events.jsonl");
            let meta = match tokio::fs::metadata(&events).await {
                Ok(m) => m,
                Err(_) => continue,
            };
            out.push(SessionInfo {
                id: path.file_name().unwrap().to_string_lossy().to_string(),
                path: events,
                size_bytes: meta.len(),
                modified: meta.modified().ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| chrono::DateTime::<chrono::Utc>::from(
                        std::time::UNIX_EPOCH + d).to_rfc3339()),
            });
        }
        out.sort_by(|a, b| b.modified.cmp(&a.modified));
        Ok(out)
    }

    pub fn validate_id(id: &str) -> Result<()> {
        if id.is_empty() || id.contains('/') || id.contains("..") {
            return Err(AgentError::Config(format!("invalid session id: {id}")));
        }
        Ok(())
    }
}
