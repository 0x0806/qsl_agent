//! Persistent memory backed by an Obsidian vault (plain Markdown).
//!
//! Layout inside the vault:
//! ```text
//! QSL_Agent/
//!   Profile.md   Memories.md   Daily/YYYY-MM-DD.md
//!   Architecture/ Security/ Research/ Skills/
//! ```

use chrono::Local;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use walkdir::WalkDir;

use crate::util::{floor_char_boundary, smart_clip};

pub struct ObsidianMemory {
    pub vault_path: PathBuf,
}

impl ObsidianMemory {
    pub fn new(vault_path: &Path) -> Self {
        let mem = Self {
            vault_path: vault_path.to_path_buf(),
        };
        if let Err(e) = mem.init_agent_folders() {
            tracing::warn!(error = %e, vault = %vault_path.display(), "could not initialise Obsidian folders");
        }
        mem
    }

    pub fn agent_dir(&self) -> PathBuf {
        self.vault_path.join("QSL_Agent")
    }
    pub fn daily_dir(&self) -> PathBuf {
        self.agent_dir().join("Daily")
    }
    pub fn memories_file(&self) -> PathBuf {
        self.agent_dir().join("Memories.md")
    }
    pub fn profile_file(&self) -> PathBuf {
        self.agent_dir().join("Profile.md")
    }
    pub fn skills_dir(&self) -> PathBuf {
        self.agent_dir().join("Skills")
    }

    pub fn init_agent_folders(&self) -> std::io::Result<()> {
        let agent_dir = self.agent_dir();
        for sub in ["Daily", "Skills", "Architecture", "Security", "Research"] {
            fs::create_dir_all(agent_dir.join(sub))?;
        }
        let memories = self.memories_file();
        if !memories.exists() {
            fs::write(
                &memories,
                "# QSL Agent — Persistent Knowledge & Memories\n\n\
                 This file stores long-term memory, user preferences, and key context.\n\
                 Managed automatically by QSL Agent. One fact per bullet.\n\n---\n",
            )?;
        }
        let profile = self.profile_file();
        if !profile.exists() {
            let hostname = fs::read_to_string("/etc/hostname")
                .map(|s| s.trim().to_string())
                .ok()
                .filter(|s| !s.is_empty())
                .or_else(|| std::env::var("HOSTNAME").ok())
                .unwrap_or_else(|| "unknown".into());
            let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
            fs::write(
                &profile,
                format!(
                    "# QSL Agent — User & System Profile\n\n\
                     - **Role**: QSL Autonomous Agent (Rust)\n\
                     - **Execution**: 100% Local (Ollama / LM Studio / GGUF)\n\
                     - **Privacy**: Zero external telemetry, zero accounts\n\
                     - **Memory Backend**: Obsidian Vault\n\
                     - **System User**: {user}\n\
                     - **Hostname**: {hostname}\n\
                     - **OS**: {}\n\
                     - **Created**: {}\n",
                    std::env::consts::OS,
                    Local::now().format("%Y-%m-%d %H:%M:%S")
                ),
            )?;
        }
        Ok(())
    }

    /// Profile plus the most recent memories, for the (session-stable)
    /// system prompt. Bounded so it never crowds out the conversation.
    pub fn load_profile_and_memories(&self, max_memory_chars: usize) -> String {
        let mut context = String::new();
        if let Ok(profile) = fs::read_to_string(self.profile_file()) {
            if !profile.trim().is_empty() {
                context.push_str("### User & System Profile (Obsidian)\n");
                context.push_str(&smart_clip(profile.trim(), 3000));
                context.push_str("\n\n");
            }
        }
        let entries = self.memory_entries();
        if !entries.is_empty() {
            let mut picked: Vec<&str> = Vec::new();
            let mut used = 0;
            for e in entries.iter().rev() {
                if used + e.len() > max_memory_chars {
                    break;
                }
                used += e.len();
                picked.push(e);
            }
            picked.reverse();
            context.push_str(&format!(
                "### Long-Term Memories (Obsidian; {} most recent of {}; use `memory_search` for older)\n",
                picked.len(),
                entries.len()
            ));
            for e in picked {
                context.push_str(e);
                context.push('\n');
            }
            context.push('\n');
        }
        context
    }

    /// Bullet entries in Memories.md (each `- ` line plus continuation lines).
    pub fn memory_entries(&self) -> Vec<String> {
        let Ok(text) = fs::read_to_string(self.memories_file()) else {
            return vec![];
        };
        let mut entries: Vec<String> = Vec::new();
        for line in text.lines() {
            if line.starts_with("- ") {
                entries.push(line.to_string());
            } else if line.starts_with("  ") && !line.trim().is_empty() {
                if let Some(last) = entries.last_mut() {
                    last.push('\n');
                    last.push_str(line);
                }
            }
        }
        entries
    }

    pub fn append_memory(&self, note: &str) -> Result<String, String> {
        let note = note.trim();
        if note.is_empty() {
            return Err("memory text is empty".into());
        }
        let normalized = note.to_lowercase();
        if self
            .memory_entries()
            .iter()
            .any(|e| e.to_lowercase().contains(&normalized))
        {
            return Ok("This fact is already stored in memory (duplicate skipped).".into());
        }
        let now = Local::now().format("%Y-%m-%d %H:%M");
        let body = note.replace('\n', "\n  ");
        let entry = format!("- **[{now}]** {body}\n");
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.memories_file())
            .map_err(|e| format!("failed to open Memories.md: {e}"))?;
        file.write_all(entry.as_bytes())
            .map_err(|e| format!("failed to write Memories.md: {e}"))?;
        Ok(format!(
            "Memory saved to {}",
            self.memories_file().display()
        ))
    }

    /// Remove memory entries containing `needle` (case-insensitive).
    pub fn forget(&self, needle: &str) -> Result<usize, String> {
        let needle = needle.trim().to_lowercase();
        if needle.len() < 3 {
            return Err("give at least 3 characters to match".into());
        }
        let path = self.memories_file();
        let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let mut out = String::new();
        let mut removed = 0;
        let mut skipping = false;
        for line in text.lines() {
            if line.starts_with("- ") {
                skipping = line.to_lowercase().contains(&needle);
                if skipping {
                    removed += 1;
                }
            } else if !line.starts_with("  ") {
                skipping = false;
            }
            if !skipping {
                out.push_str(line);
                out.push('\n');
            }
        }
        if removed > 0 {
            fs::write(&path, out).map_err(|e| e.to_string())?;
        }
        Ok(removed)
    }

    /// Rank memory entries against a query (term overlap weighted by rarity,
    /// with a mild recency bonus). Returns the top `k`.
    pub fn search_memories(&self, query: &str, k: usize) -> Vec<String> {
        let entries = self.memory_entries();
        let terms = tokenize(query);
        if terms.is_empty() || entries.is_empty() {
            return vec![];
        }
        let docs: Vec<Vec<String>> = entries.iter().map(|e| tokenize(e)).collect();
        let n = docs.len() as f64;
        let mut scored: Vec<(f64, usize)> = docs
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let mut score = 0.0;
                for t in &terms {
                    let tf = d
                        .iter()
                        .filter(|w| *w == t || (t.len() > 4 && w.starts_with(t.as_str())))
                        .count() as f64;
                    if tf > 0.0 {
                        let df = docs.iter().filter(|o| o.contains(t)).count().max(1) as f64;
                        score += (1.0 + (n / df).ln()) * (tf / (tf + 1.2));
                    }
                }
                if score > 0.0 {
                    score += 0.25 * (i as f64 + 1.0) / n;
                }
                (score, i)
            })
            .filter(|(s, _)| *s > 0.0)
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored
            .into_iter()
            .take(k)
            .map(|(_, i)| entries[i].clone())
            .collect()
    }

    pub fn append_daily_log(&self, entry: &str) -> Result<(), String> {
        let date = Local::now().format("%Y-%m-%d").to_string();
        let path = self.daily_dir().join(format!("{date}.md"));
        fs::create_dir_all(self.daily_dir()).map_err(|e| e.to_string())?;
        if !path.exists() {
            fs::write(&path, format!("# Daily Agent Log — {date}\n\n---\n"))
                .map_err(|e| format!("failed to create daily log: {e}"))?;
        }
        let mut file = OpenOptions::new()
            .append(true)
            .open(&path)
            .map_err(|e| format!("failed to open daily log: {e}"))?;
        let time = Local::now().format("%H:%M:%S");
        file.write_all(format!("\n### [{time}]\n{}\n", entry.trim()).as_bytes())
            .map_err(|e| format!("failed to write daily log: {e}"))
    }

    /// Resolve a vault-relative note path, refusing anything that escapes the
    /// vault (`../`, absolute paths, hidden folders such as `.obsidian`).
    pub fn note_path(&self, rel: &str) -> Result<PathBuf, String> {
        let clean = rel.trim().trim_start_matches('/');
        if clean.is_empty() {
            return Err("note path is empty".into());
        }
        let rel_path = Path::new(clean);
        for c in rel_path.components() {
            match c {
                Component::Normal(s) if s.to_string_lossy().starts_with('.') => {
                    return Err(format!(
                        "hidden path component '{}' is not allowed",
                        s.to_string_lossy()
                    ));
                }
                Component::Normal(_) => {}
                _ => return Err(format!("note path '{rel}' must stay inside the vault")),
            }
        }
        let mut path = self.vault_path.join(rel_path);
        if path.extension().is_none() {
            path.set_extension("md");
        }
        Ok(path)
    }

    pub fn read_note(&self, rel: &str) -> Result<String, String> {
        let path = self.note_path(rel)?;
        if !path.exists() {
            // Fall back to a case-insensitive basename match, Obsidian-style.
            let wanted = path
                .file_name()
                .map(|f| f.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let hit = self
                .list_notes(None)
                .into_iter()
                .find(|n| n.to_lowercase().ends_with(&wanted));
            return match hit {
                Some(h) => self.read_note(&h),
                None => Err(format!(
                    "note '{}' does not exist (try obsidian_list or obsidian_search)",
                    rel
                )),
            };
        }
        let content = fs::read_to_string(&path).map_err(|e| format!("failed to read note: {e}"))?;
        Ok(smart_clip(&content, 15_000))
    }

    pub fn write_note(&self, rel: &str, content: &str, append: bool) -> Result<String, String> {
        let path = self.note_path(rel)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("failed to create folders: {e}"))?;
        }
        if append {
            let mut f = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|e| e.to_string())?;
            f.write_all(format!("\n{}\n", content.trim_end()).as_bytes())
                .map_err(|e| e.to_string())?;
        } else {
            fs::write(&path, content).map_err(|e| format!("failed to write note: {e}"))?;
        }
        Ok(format!(
            "Obsidian note '{}' {} ({} bytes).",
            path.strip_prefix(&self.vault_path)
                .unwrap_or(&path)
                .display(),
            if append { "appended" } else { "written" },
            content.len()
        ))
    }

    fn visible_md_files(
        &self,
        root: &Path,
        max_depth: usize,
    ) -> impl Iterator<Item = walkdir::DirEntry> {
        let vault = self.vault_path.clone();
        WalkDir::new(root)
            .max_depth(max_depth)
            .into_iter()
            .filter_entry(move |e| {
                e.path() == vault || !e.file_name().to_string_lossy().starts_with('.')
            })
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "md"))
    }

    pub fn search_vault(&self, query: &str, limit: usize) -> Vec<(String, usize, String)> {
        let q = query.to_lowercase();
        let mut matches = Vec::new();
        for entry in self.visible_md_files(&self.vault_path, 12) {
            let Ok(content) = fs::read_to_string(entry.path()) else {
                continue;
            };
            let rel = entry
                .path()
                .strip_prefix(&self.vault_path)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .to_string();
            for (idx, line) in content.lines().enumerate() {
                if line.to_lowercase().contains(&q) {
                    let end = floor_char_boundary(line, 240);
                    matches.push((rel.clone(), idx + 1, line[..end].to_string()));
                    if matches.len() >= limit {
                        return matches;
                    }
                }
            }
        }
        matches
    }

    pub fn list_notes(&self, subfolder: Option<&str>) -> Vec<String> {
        let target = match subfolder.map(str::trim).filter(|s| !s.is_empty()) {
            Some(sub) => match self.note_path(sub) {
                Ok(p) => p.with_extension(""),
                Err(_) => return vec![],
            },
            None => self.vault_path.clone(),
        };
        let mut notes: Vec<String> = self
            .visible_md_files(&target, 6)
            .filter_map(|e| {
                e.path()
                    .strip_prefix(&self.vault_path)
                    .ok()
                    .map(|r| r.to_string_lossy().to_string())
            })
            .collect();
        notes.sort();
        notes
    }
}

fn tokenize(s: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "the", "and", "for", "with", "that", "this", "from", "are", "was", "you", "your", "use",
        "uses", "user",
    ];
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3 && !STOP.contains(w))
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> (tempfile::TempDir, ObsidianMemory) {
        let d = tempfile::tempdir().unwrap();
        let m = ObsidianMemory::new(d.path());
        (d, m)
    }

    #[test]
    fn creates_structure() {
        let (_d, m) = mem();
        assert!(m.memories_file().exists());
        assert!(m.profile_file().exists());
        assert!(m.daily_dir().is_dir());
    }

    #[test]
    fn append_search_forget() {
        let (_d, m) = mem();
        m.append_memory("User prefers ripgrep over grep for code search")
            .unwrap();
        m.append_memory("Project QSL uses Rust 2021 edition and tokio")
            .unwrap();
        m.append_memory("Favourite editor is Helix").unwrap();
        let dup = m.append_memory("favourite editor is helix").unwrap();
        assert!(dup.contains("duplicate"));
        let hits = m.search_memories("which editor does the user like? helix?", 2);
        assert!(hits[0].contains("Helix"));
        assert_eq!(m.forget("ripgrep").unwrap(), 1);
        assert_eq!(m.memory_entries().len(), 2);
        let ctx = m.load_profile_and_memories(4000);
        assert!(ctx.contains("Helix") && ctx.contains("Profile"));
    }

    #[test]
    fn note_path_traversal_blocked() {
        let (_d, m) = mem();
        assert!(m.note_path("../../etc/passwd").is_err());
        assert!(m.note_path(".obsidian/app.json").is_err());
        assert!(m.write_note("../escape", "x", false).is_err());
        assert!(m
            .note_path("Projects/QSL")
            .unwrap()
            .ends_with("Projects/QSL.md"));
    }

    #[test]
    fn notes_roundtrip_and_search() {
        let (_d, m) = mem();
        m.write_note("Research/rust async", "Tokio runtime notes ünïcode", false)
            .unwrap();
        m.write_note("Research/rust async", "more", true).unwrap();
        assert!(m.read_note("Research/rust async").unwrap().contains("more"));
        assert!(
            m.read_note("RUST ASYNC").is_ok(),
            "case-insensitive basename fallback"
        );
        assert_eq!(m.search_vault("tokio", 10).len(), 1);
        assert!(m
            .list_notes(Some("Research"))
            .iter()
            .any(|n| n.contains("rust async")));
        m.append_daily_log("**User:** hi").unwrap();
    }
}
