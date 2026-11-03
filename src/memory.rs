use chrono::Local;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Persistent memory backend using an Obsidian vault (plain Markdown files)
pub struct ObsidianMemory {
    pub vault_path: PathBuf,
}

impl ObsidianMemory {
    pub fn new<P: AsRef<Path>>(vault_path: P) -> Self {
        let vault = vault_path.as_ref().to_path_buf();
        let mem = Self { vault_path: vault };
        let _ = mem.init_agent_folders();
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

    /// Create the agent directory structure inside the Obsidian vault
    pub fn init_agent_folders(&self) -> std::io::Result<()> {
        let agent_dir = self.agent_dir();
        fs::create_dir_all(&agent_dir)?;
        fs::create_dir_all(self.daily_dir())?;
        fs::create_dir_all(agent_dir.join("Skills"))?;
        fs::create_dir_all(agent_dir.join("Architecture"))?;
        fs::create_dir_all(agent_dir.join("Security"))?;
        fs::create_dir_all(agent_dir.join("Research"))?;

        let memories = self.memories_file();
        if !memories.exists() {
            fs::write(
                &memories,
                "# QSL Agent — Persistent Knowledge & Memories\n\n\
                 This file stores long-term memory, user preferences, and key context.\n\
                 Managed automatically by QSL Agent.\n\n\
                 ---\n",
            )?;
        }

        let profile = self.profile_file();
        if !profile.exists() {
            let hostname = std::env::var("HOSTNAME")
                .or_else(|_| std::env::var("HOST"))
                .unwrap_or_else(|_| "unknown".to_string());
            let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());

            fs::write(
                &profile,
                format!(
                    "# QSL Agent — User & System Profile\n\n\
                     - **Role**: QSL Autonomous Agent (Rust)\n\
                     - **Execution**: 100% Local (Ollama / LM Studio / GGUF)\n\
                     - **Privacy**: Zero external telemetry, zero accounts\n\
                     - **Memory Backend**: Obsidian Vault\n\
                     - **System User**: {}\n\
                     - **Hostname**: {}\n\
                     - **Created**: {}\n",
                    user,
                    hostname,
                    Local::now().format("%Y-%m-%d %H:%M:%S")
                ),
            )?;
        }

        Ok(())
    }

    /// Load profile and recent memories as context for the system prompt
    pub fn load_profile_and_memories(&self) -> String {
        let mut context = String::new();

        if let Ok(profile) = fs::read_to_string(self.profile_file()) {
            if !profile.trim().is_empty() {
                context.push_str("### User & System Profile (Obsidian):\n");
                context.push_str(&profile);
                context.push_str("\n\n");
            }
        }

        if let Ok(memories) = fs::read_to_string(self.memories_file()) {
            if !memories.trim().is_empty() {
                // Only load the last 4000 chars to stay within context budget
                let trimmed = if memories.len() > 4000 {
                    // Find a clean line break near the cutoff
                    let start = memories.len() - 4000;
                    let adjusted = memories[start..]
                        .find('\n')
                        .map(|pos| start + pos + 1)
                        .unwrap_or(start);
                    &memories[adjusted..]
                } else {
                    &memories
                };
                context.push_str("### Long-Term Memories (Obsidian):\n");
                context.push_str(trimmed);
                context.push_str("\n\n");
            }
        }

        context
    }

    /// Append a timestamped entry to the persistent memory file
    pub fn append_memory(&self, note: &str) -> Result<String, String> {
        let path = self.memories_file();
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let entry = format!("\n- **[{}]**: {}\n", now, note.trim());

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("Failed to open Memories.md: {}", e))?;

        file.write_all(entry.as_bytes())
            .map_err(|e| format!("Failed to write to Memories.md: {}", e))?;

        Ok(format!("Memory appended to {} successfully.", path.display()))
    }

    /// Append a timestamped entry to today's daily log
    pub fn append_daily_log(&self, entry: &str) -> Result<(), String> {
        let date_str = Local::now().format("%Y-%m-%d").to_string();
        let file_path = self.daily_dir().join(format!("{}.md", date_str));
        let time_str = Local::now().format("%H:%M:%S").to_string();

        // Create file with header if new
        if !file_path.exists() {
            let header = format!("# Daily Agent Log — {}\n\n---\n", date_str);
            fs::write(&file_path, header)
                .map_err(|e| format!("Failed to create daily log: {}", e))?;
        }

        let mut file = OpenOptions::new()
            .append(true)
            .open(&file_path)
            .map_err(|e| format!("Failed to open daily log: {}", e))?;

        let content = format!("\n### [{}]\n{}\n", time_str, entry.trim());
        file.write_all(content.as_bytes())
            .map_err(|e| format!("Failed to write to daily log: {}", e))?;

        Ok(())
    }

    /// Full-text search across all markdown files in the Obsidian vault
    pub fn search_vault(&self, query: &str) -> Vec<(String, usize, String)> {
        let mut matches = Vec::new();
        let query_lower = query.to_lowercase();

        for entry in WalkDir::new(&self.vault_path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                // Skip hidden dirs (.obsidian, .git, .trash)
                !e.path()
                    .components()
                    .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
                    || e.path() == self.vault_path
            })
        {
            if entry.path().is_file() {
                if let Some(ext) = entry.path().extension() {
                    if ext == "md" {
                        if let Ok(content) = fs::read_to_string(entry.path()) {
                            let rel_path = entry
                                .path()
                                .strip_prefix(&self.vault_path)
                                .unwrap_or(entry.path())
                                .to_string_lossy()
                                .to_string();

                            for (idx, line) in content.lines().enumerate() {
                                if line.to_lowercase().contains(&query_lower) {
                                    matches.push((rel_path.clone(), idx + 1, line.to_string()));
                                    if matches.len() >= 30 {
                                        return matches;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        matches
    }

    /// Read a note from the Obsidian vault by relative path
    pub fn read_note(&self, rel_path: &str) -> Result<String, String> {
        let clean_path = rel_path.trim_start_matches('/');
        let path = self.vault_path.join(clean_path);
        let path = if path.extension().is_none() {
            path.with_extension("md")
        } else {
            path
        };

        if !path.exists() {
            return Err(format!("Obsidian note '{}' does not exist.", path.display()));
        }

        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read note: {}", e))?;

        // Cap output to prevent flooding the context
        if content.len() > 15_000 {
            Ok(format!(
                "{}\n\n[... truncated, showing first 15000 chars of {} total bytes ...]",
                &content[..15_000],
                content.len()
            ))
        } else {
            Ok(content)
        }
    }

    /// Create or overwrite a note in the Obsidian vault
    pub fn write_note(&self, rel_path: &str, content: &str) -> Result<String, String> {
        let clean_path = rel_path.trim_start_matches('/');
        let path = self.vault_path.join(clean_path);
        let path = if path.extension().is_none() {
            path.with_extension("md")
        } else {
            path
        };

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directories: {}", e))?;
        }

        fs::write(&path, content).map_err(|e| format!("Failed to write note: {}", e))?;
        Ok(format!(
            "Obsidian note '{}' written successfully ({} bytes).",
            path.display(),
            content.len()
        ))
    }

    /// List notes in a subfolder of the Obsidian vault
    pub fn list_notes(&self, subfolder: Option<&str>) -> Vec<String> {
        let target_dir = match subfolder {
            Some(sub) if !sub.is_empty() => self.vault_path.join(sub.trim_start_matches('/')),
            _ => self.vault_path.clone(),
        };

        let mut notes = Vec::new();
        for entry in WalkDir::new(&target_dir)
            .max_depth(5)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                !e.path()
                    .components()
                    .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
            })
        {
            if entry.path().is_file() {
                if let Some(ext) = entry.path().extension() {
                    if ext == "md" {
                        if let Ok(rel) = entry.path().strip_prefix(&self.vault_path) {
                            notes.push(rel.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
        notes.sort();
        notes
    }
}
