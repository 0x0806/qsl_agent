//! Session persistence: `<state_dir>/sessions/<id>.jsonl`.
//! Line 1 is metadata; every following line is one message. Files are
//! rewritten atomically after each step so a crash never corrupts history.

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::provider::{Message, Role};
use crate::util::{floor_char_boundary, truncate_chars};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub created: DateTime<Local>,
    pub updated: DateTime<Local>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub messages: usize,
    #[serde(default)]
    pub tokens_in: u64,
    #[serde(default)]
    pub tokens_out: u64,
    /// Rolling summary of compacted turns (restored on resume).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

impl SessionMeta {
    pub fn new(model: &str) -> Self {
        let now = Local::now();
        Self {
            id: format!(
                "{}-{}",
                now.format("%Y%m%d-%H%M%S"),
                &crate::util::short_id()[..4]
            ),
            created: now,
            updated: now,
            title: String::new(),
            model: model.to_string(),
            cwd: std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            messages: 0,
            tokens_in: 0,
            tokens_out: 0,
            summary: None,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Line {
    Meta(SessionMeta),
    Msg(Message),
}

pub struct SessionStore {
    pub dir: PathBuf,
    pub enabled: bool,
}

impl SessionStore {
    pub fn new(dir: PathBuf, enabled: bool) -> Self {
        if enabled {
            let _ = fs::create_dir_all(&dir);
        }
        Self { dir, enabled }
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.jsonl"))
    }

    pub fn save(&self, meta: &mut SessionMeta, history: &[Message]) -> std::io::Result<()> {
        if !self.enabled || !history.iter().any(|m| m.role == Role::User) {
            return Ok(());
        }
        meta.updated = Local::now();
        meta.messages = history.iter().filter(|m| m.role != Role::System).count();
        if meta.title.is_empty() {
            if let Some(first) = history.iter().find(|m| m.role == Role::User) {
                meta.title = truncate_chars(first.content.lines().next().unwrap_or("").trim(), 70);
            }
        }
        fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join(format!(".{}.tmp", meta.id));
        {
            let mut f = std::io::BufWriter::new(fs::File::create(&tmp)?);
            serde_json::to_writer(&mut f, &Line::Meta(meta.clone()))?;
            f.write_all(b"\n")?;
            for m in history.iter().filter(|m| m.role != Role::System) {
                serde_json::to_writer(&mut f, &Line::Msg(m.clone()))?;
                f.write_all(b"\n")?;
            }
            f.flush()?;
        }
        fs::rename(&tmp, self.path(&meta.id))
    }

    /// Load by exact id, unique prefix, or list index (1 = most recent).
    pub fn load(&self, key: &str) -> Result<(SessionMeta, Vec<Message>), String> {
        let key = key.trim();
        let id = if let Some(n) = key.parse::<usize>().ok().filter(|n| *n >= 1 && *n <= 999) {
            self.list(n)
                .get(n - 1)
                .map(|m| m.id.clone())
                .ok_or_else(|| format!("no session #{n}"))?
        } else if self.path(key).exists() {
            key.to_string()
        } else {
            let matches: Vec<SessionMeta> = self
                .list(10_000)
                .into_iter()
                .filter(|m| m.id.starts_with(key))
                .collect();
            match matches.len() {
                0 => return Err(format!("no session matches '{key}' (see /sessions)")),
                1 => matches[0].id.clone(),
                n => return Err(format!("'{key}' is ambiguous ({n} sessions match)")),
            }
        };
        read_session(&self.path(&id))
    }

    pub fn latest(&self) -> Option<String> {
        self.list(1).into_iter().next().map(|m| m.id)
    }

    /// Most recently updated sessions first.
    pub fn list(&self, limit: usize) -> Vec<SessionMeta> {
        let Ok(rd) = fs::read_dir(&self.dir) else {
            return vec![];
        };
        let mut metas: Vec<SessionMeta> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
            .filter_map(|p| read_meta(&p))
            .collect();
        metas.sort_by_key(|m| std::cmp::Reverse(m.updated));
        metas.truncate(limit);
        metas
    }

    pub fn delete(&self, id: &str) -> std::io::Result<()> {
        fs::remove_file(self.path(id))
    }
}

fn read_meta(path: &Path) -> Option<SessionMeta> {
    let f = fs::File::open(path).ok()?;
    let mut first = String::new();
    BufReader::new(f).read_line(&mut first).ok()?;
    match serde_json::from_str::<Line>(&first).ok()? {
        Line::Meta(m) => Some(m),
        Line::Msg(_) => None,
    }
}

fn read_session(path: &Path) -> Result<(SessionMeta, Vec<Message>), String> {
    let f = fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mut meta = None;
    let mut msgs = Vec::new();
    for (i, line) in BufReader::new(f).lines().enumerate() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Line>(&line) {
            Ok(Line::Meta(m)) => meta = Some(m),
            Ok(Line::Msg(m)) => msgs.push(m),
            Err(e) => tracing::warn!(line = i + 1, error = %e, "skipping corrupt session line"),
        }
    }
    let meta = meta.ok_or_else(|| format!("{} has no metadata header", path.display()))?;
    Ok((meta, msgs))
}

/// Keyword search across saved sessions (used by the `session_search` tool).
pub fn search_sessions(dir: &Path, query: &str, limit: usize) -> Result<String, String> {
    let q = query.trim().to_lowercase();
    if q.len() < 2 {
        return Err("query too short".into());
    }
    let store = SessionStore::new(dir.to_path_buf(), true);
    let mut out = Vec::new();
    for meta in store.list(500) {
        let Ok((_, msgs)) = read_session(&store.path(&meta.id)) else {
            continue;
        };
        for m in msgs
            .iter()
            .filter(|m| matches!(m.role, Role::User | Role::Assistant))
        {
            let lower = m.content.to_lowercase();
            if let Some(pos) = lower.find(&q) {
                // `lower` can differ in byte length from `content` for some
                // scripts; map the snippet window conservatively.
                let start =
                    floor_char_boundary(&m.content, pos.saturating_sub(80).min(m.content.len()));
                let end =
                    floor_char_boundary(&m.content, (pos + q.len() + 160).min(m.content.len()));
                let snippet = m.content.get(start..end).unwrap_or("").replace('\n', " ");
                out.push(format!(
                    "[{}] {} ({}, {:?}): ...{}...",
                    meta.id,
                    meta.title,
                    meta.updated.format("%Y-%m-%d"),
                    m.role,
                    snippet.trim()
                ));
                if out.len() >= limit {
                    break;
                }
            }
        }
        if out.len() >= limit {
            break;
        }
    }
    if out.is_empty() {
        Ok(format!("No past sessions mention '{query}'."))
    } else {
        Ok(out.join("\n"))
    }
}

pub fn export_markdown(meta: &SessionMeta, history: &[Message]) -> String {
    let mut s = format!(
        "# {}\n\n- Session: `{}`\n- Model: `{}`\n- Created: {}\n\n---\n\n",
        if meta.title.is_empty() {
            "QSL Agent Session"
        } else {
            &meta.title
        },
        meta.id,
        meta.model,
        meta.created.format("%Y-%m-%d %H:%M")
    );
    for m in history.iter().filter(|m| m.role != Role::System) {
        match m.role {
            Role::User => s.push_str(&format!("## User\n\n{}\n\n", m.content.trim())),
            Role::Assistant => {
                if !m.content.trim().is_empty() {
                    s.push_str(&format!("## Agent\n\n{}\n\n", m.content.trim()));
                }
                for tc in &m.tool_calls {
                    s.push_str(&format!(
                        "> **Tool call** `{}` `{}`\n\n",
                        tc.name,
                        truncate_chars(&tc.arguments, 400)
                    ));
                }
            }
            Role::Tool => s.push_str(&format!(
                "<details><summary>Result: {}</summary>\n\n```\n{}\n```\n</details>\n\n",
                m.name.clone().unwrap_or_default(),
                truncate_chars(m.content.trim(), 4000)
            )),
            Role::System => {}
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_list_load_search() {
        let d = tempfile::tempdir().unwrap();
        let store = SessionStore::new(d.path().to_path_buf(), true);
        let mut meta = SessionMeta::new("m");
        let hist = vec![
            Message::system("sys"),
            Message::user("Explain the borrow checker"),
            Message::assistant("Ownership rules..."),
        ];
        store.save(&mut meta, &hist).unwrap();
        assert_eq!(meta.title, "Explain the borrow checker");
        let list = store.list(10);
        assert_eq!(list.len(), 1);
        let (m2, msgs) = store.load(&meta.id[..8]).unwrap();
        assert_eq!(m2.id, meta.id);
        assert_eq!(msgs.len(), 2, "system prompt is not persisted");
        assert_eq!(store.load("1").unwrap().0.id, meta.id);
        let hits = search_sessions(d.path(), "ownership", 5).unwrap();
        assert!(hits.contains(&meta.id));
        assert!(export_markdown(&meta, &hist).contains("## User"));
        assert!(store.load("zzz").is_err());
    }

    #[test]
    fn empty_sessions_are_not_written() {
        let d = tempfile::tempdir().unwrap();
        let store = SessionStore::new(d.path().to_path_buf(), true);
        let mut meta = SessionMeta::new("m");
        store.save(&mut meta, &[Message::system("s")]).unwrap();
        assert!(store.list(5).is_empty());
    }
}
