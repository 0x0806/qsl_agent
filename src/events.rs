use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::sync::Mutex;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::provider::{ChatMessage, Role, ToolCall};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    UserMsg   { text: String, at: DateTime<Utc> },
    Assistant { content: String, tool_calls: Vec<ToolCallRec>, at: DateTime<Utc> },
    ToolCall  { id: String, name: String, args: serde_json::Value, at: DateTime<Utc> },
    ToolResult{ id: String, ok: bool, output: String, at: DateTime<Utc> },
    Heartbeat { step: u32, ctx_pct: f32, at: DateTime<Utc> },
    Checkpoint{ note: String, at: DateTime<Utc> },
    Compaction{ kept_msgs: usize, tokens: usize, at: DateTime<Utc> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRec { pub id: String, pub name: String, pub args: serde_json::Value }

pub struct EventLog {
    session_dir: PathBuf,
    file: Mutex<BufWriter<File>>,
    pub events: Mutex<Vec<Event>>,
}

impl EventLog {
    pub async fn open(state_dir: &Path) -> Result<Arc<Self>> {
        Self::open_session(state_dir, &uuid::Uuid::new_v4().to_string()).await
    }

    pub async fn open_session(state_dir: &Path, session_id: &str) -> Result<Arc<Self>> {
        let dir = state_dir.join("sessions").join(session_id);
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join("events.jsonl");
        let file = OpenOptions::new().create(true).append(true).open(&path).await?;
        let bw = BufWriter::with_capacity(64 * 1024, file);
        Ok(Arc::new(Self {
            session_dir: dir,
            file: Mutex::new(bw),
            events: Mutex::new(Vec::new()),
        }))
    }

    pub fn dir(&self) -> &PathBuf { &self.session_dir }

    pub async fn append(&self, ev: Event) -> Result<()> {
        let line = serde_json::to_string(&ev)?;
        {
            let mut f = self.file.lock().await;
            f.write_all(line.as_bytes()).await?;
            f.write_all(b"\n").await?;
        }
        self.events.lock().await.push(ev);
        Ok(())
    }

    pub async fn flush(&self) -> Result<()> {
        let mut f = self.file.lock().await;
        f.flush().await?;
        Ok(())
    }

    pub async fn size_bytes(&self) -> u64 {
        let p = self.session_dir.join("events.jsonl");
        tokio::fs::metadata(p).await.map(|m| m.len()).unwrap_or(0)
    }

    pub async fn replay(path: &Path) -> Result<Vec<Event>> {
        let f = File::open(path).await?;
        let mut lines = BufReader::new(f).lines();
        let mut out = Vec::new();
        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() { continue; }
            if let Ok(ev) = serde_json::from_str::<Event>(&line) { out.push(ev); }
        }
        Ok(out)
    }

    pub fn to_chat_history(events: &[Event]) -> Vec<ChatMessage> {
        let mut msgs: Vec<ChatMessage> = Vec::new();
        for ev in events {
            match ev {
                Event::UserMsg { text, .. } => msgs.push(ChatMessage::user(text.clone())),
                Event::Assistant { content, tool_calls, .. } => {
                    let tcs: Vec<ToolCall> = tool_calls.iter().map(|r| ToolCall {
                        id: r.id.clone(),
                        name: r.name.clone(),
                        arguments: r.args.clone(),
                    }).collect();
                    let mut m = ChatMessage::assistant(content.clone());
                    if !tcs.is_empty() { m.tool_calls = Some(tcs); }
                    msgs.push(m);
                }
                Event::ToolResult { id, output, .. } => {
                    msgs.push(ChatMessage {
                        role: Role::Tool,
                        content: output.clone(),
                        tool_calls: None,
                        tool_call_id: Some(id.clone()),
                        tokens: Default::default(),
                    });
                }
                _ => {}
            }
        }
        msgs
    }
}
