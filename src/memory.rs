use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tracing::instrument;

use crate::config::Config;
use crate::error::Result;

pub struct Memory {
    notes_path: PathBuf,
    pending: Arc<Mutex<String>>,
    flush_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Memory {
    pub async fn new(cfg: &Config) -> Result<Self> {
        let path = cfg.memory.notes_file.clone();
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        OpenOptions::new().create(true).append(true).open(&path).await?;

        let pending = Arc::new(Mutex::new(String::new()));
        let path_for_flush = path.clone();
        let pending_for_flush = pending.clone();

        let handle = tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(250));
            loop {
                tick.tick().await;
                let mut buf = pending_for_flush.lock().await;
                if buf.is_empty() { continue; }
                let data = std::mem::take(&mut *buf);
                drop(buf);
                if let Err(e) = append_to_file(&path_for_flush, &data).await {
                    tracing::warn!(?e, "memory flush failed");
                }
            }
        });

        Ok(Self {
            notes_path: path,
            pending,
            flush_handle: Mutex::new(Some(handle)),
        })
    }

    pub fn path(&self) -> &PathBuf { &self.notes_path }

    #[instrument(skip(self, content), fields(len = content.len()))]
    pub async fn append(&self, content: &str) -> Result<()> {
        let mut buf = self.pending.lock().await;
        buf.push_str(content);
        buf.push('\n');
        Ok(())
    }

    pub async fn append_compaction_event(&self, kept_msgs: usize, tokens: usize) -> Result<()> {
        self.append(&format!(
            "\n---\n<!-- compaction: kept={kept_msgs} tokens={tokens} at={} -->",
            chrono::Utc::now().to_rfc3339(),
        )).await
    }

    pub async fn flush(&self) -> Result<()> {
        let mut buf = self.pending.lock().await;
        let data = std::mem::take(&mut *buf);
        drop(buf);
        if !data.is_empty() {
            append_to_file(&self.notes_path, &data).await?;
        }
        Ok(())
    }

    pub async fn read_all(&self) -> Result<String> {
        Ok(tokio::fs::read_to_string(&self.notes_path).await.unwrap_or_default())
    }

    pub async fn retrieve(&self, query: &str, k: usize) -> Result<String> {
        let all = self.read_all().await?;
        let sections: Vec<&str> = all.split("\n---\n").filter(|s| !s.trim().is_empty()).collect();
        if sections.is_empty() { return Ok(String::new()); }

        let q_terms: Vec<String> = query
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() >= 3)
            .map(String::from)
            .collect();

        let mut scored: Vec<(f32, usize)> = sections.iter().enumerate().map(|(i, s)| {
            let lower = s.to_lowercase();
            let hits = q_terms.iter().filter(|t| lower.contains(t.as_str())).count();
            let recency = (i as f32 + 1.0) / sections.len() as f32;
            (hits as f32 + recency * 0.3, i)
        }).collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let top: Vec<&str> = scored.iter().take(k).map(|(_, i)| sections[*i]).collect();
        Ok(top.join("\n---\n"))
    }
}

async fn append_to_file(path: &PathBuf, data: &str) -> Result<()> {
    let mut f = OpenOptions::new().create(true).append(true).open(path).await?;
    f.write_all(data.as_bytes()).await?;
    f.flush().await?;
    Ok(())
}

impl Drop for Memory {
    fn drop(&mut self) {
        if let Ok(mut h) = self.flush_handle.try_lock() {
            if let Some(handle) = h.take() { handle.abort(); }
        }
    }
}
