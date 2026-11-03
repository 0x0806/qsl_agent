use std::collections::VecDeque;
use std::sync::Arc;
use chrono::Utc;
use crate::config::Config;
use crate::error::Result;
use crate::events::{Event, EventLog, ToolCallRec};
use crate::memory::Memory;
use crate::provider::{ChatMessage, ToolCall};

const ROTATE_AT_BYTES: u64 = 100 * 1024 * 1024;
const SNAPSHOT_DEPTH: usize = 10;

pub struct HistorySnapshots {
    ring: VecDeque<Vec<ChatMessage>>,
}

impl HistorySnapshots {
    pub fn new() -> Self { Self { ring: VecDeque::with_capacity(SNAPSHOT_DEPTH) } }
    pub fn push(&mut self, h: &[ChatMessage]) {
        self.ring.push_back(h.to_vec());
        while self.ring.len() > SNAPSHOT_DEPTH { self.ring.pop_front(); }
    }
    pub fn pop(&mut self) -> Option<Vec<ChatMessage>> { self.ring.pop_back() }
    pub fn depth(&self) -> usize { self.ring.len() }
}

pub struct Session {
    pub id: String,
    pub history: Vec<ChatMessage>,
    pub events: Arc<EventLog>,
    pub memory: Arc<Memory>,
    pub turn_idx: u64,
    pub snapshots: HistorySnapshots,
    pub last_user: Option<String>,
}

impl Session {
    pub async fn open_or_create(
        cfg: &Config, events: Arc<EventLog>, memory: Arc<Memory>,
    ) -> Result<Self> {
        let log_path = events.dir().join("events.jsonl");
        let raw = EventLog::replay(&log_path).await.unwrap_or_default();
        let mut history = vec![ChatMessage::system(cfg.agent.system_prompt.clone())];
        if !raw.is_empty() {
            history.extend(EventLog::to_chat_history(&raw));
            tracing::info!(replayed = raw.len(), "session replayed");
        }
        let id = events.dir().file_name().unwrap().to_string_lossy().to_string();
        Ok(Self {
            id, history, events, memory, turn_idx: 0,
            snapshots: HistorySnapshots::new(), last_user: None,
        })
    }

    pub async fn append_user(&mut self, text: String) -> Result<()> {
        self.last_user = Some(text.clone());
        self.snapshots.push(&self.history);
        self.history.push(ChatMessage::user(text.clone()));
        self.events.append(Event::UserMsg { text, at: Utc::now() }).await
    }

    pub async fn append_assistant(
        &mut self, content: String, tool_calls: Vec<ToolCallRec>,
    ) -> Result<()> {
        let mut m = ChatMessage::assistant(content.clone());
        if !tool_calls.is_empty() {
            m.tool_calls = Some(tool_calls.iter().map(|r| ToolCall {
                id: r.id.clone(), name: r.name.clone(), arguments: r.args.clone(),
            }).collect());
        }
        self.history.push(m);
        self.events.append(Event::Assistant { content, tool_calls, at: Utc::now() }).await
    }

    pub async fn checkpoint(&self, note: &str) -> Result<()> {
        self.events.append(Event::Checkpoint { note: note.into(), at: Utc::now() }).await
    }

    pub async fn maybe_rotate(&self) -> Result<()> {
        let size = self.events.size_bytes().await;
        if size < ROTATE_AT_BYTES { return Ok(()); }
        let src = self.events.dir().join("events.jsonl");
        let dst = self.events.dir().join(format!(
            "events.{}.jsonl", chrono::Utc::now().format("%Y%m%d-%H%M%S")));
        tokio::fs::rename(&src, &dst).await?;
        tracing::info!(size, dst = %dst.display(), "rotated event log");
        Ok(())
    }

    pub async fn undo(&mut self, turns: usize) -> Result<()> {
        let mut restored = None;
        for _ in 0..turns {
            if let Some(snap) = self.snapshots.pop() { restored = Some(snap); }
            else { break; }
        }
        if let Some(h) = restored {
            self.history = h;
            self.turn_idx = self.turn_idx.saturating_sub(turns as u64);
            self.truncate_to(self.history.len()).await?;
        }
        Ok(())
    }

    pub async fn truncate_to(&self, keep: usize) -> Result<()> {
        let path = self.events.dir().join("events.jsonl");
        let raw = EventLog::replay(&path).await.unwrap_or_default();
        let keep_events: Vec<&Event> = raw.iter().filter(|e| matches!(e,
            Event::UserMsg { .. } | Event::Assistant { .. } | Event::ToolResult { .. }
        )).collect();
        let cutoff = keep_events.len().saturating_sub(keep);
        let mut written = 0;
        let mut buf = String::new();
        for ev in &raw {
            let is_hist = matches!(ev,
                Event::UserMsg { .. } | Event::Assistant { .. } | Event::ToolResult { .. });
            if is_hist {
                written += 1;
                if written <= cutoff { continue; }
            }
            buf.push_str(&serde_json::to_string(ev)?);
            buf.push('\n');
        }
        let tmp = path.with_extension("jsonl.tmp");
        tokio::fs::write(&tmp, buf.as_bytes()).await?;
        tokio::fs::rename(&tmp, &path).await?;
        Ok(())
    }

    pub async fn export(&self, format: ExportFormat) -> Result<String> {
        match format {
            ExportFormat::Json => {
                Ok(serde_json::to_string_pretty(&serde_json::json!({
                    "session_id": self.id,
                    "turn_idx": self.turn_idx,
                    "messages": self.history,
                }))?)
            }
            ExportFormat::Markdown => {
                let mut s = format!("# Session {}\n\n", self.id);
                for m in &self.history {
                    s.push_str(&format!("### {:?}\n\n{}\n\n", m.role, m.content));
                }
                Ok(s)
            }
        }
    }

    pub fn history_mut(&mut self) -> &mut Vec<ChatMessage> { &mut self.history }
    pub fn history(&self) -> &[ChatMessage] { &self.history }
}

#[derive(Clone, Copy, Debug)]
pub enum ExportFormat { Json, Markdown }
