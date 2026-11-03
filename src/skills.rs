//! Skills: Markdown procedures (`SKILL.md` with YAML-ish frontmatter) the
//! agent can load on demand. Only names + descriptions go into the system
//! prompt (progressive disclosure); full instructions are fetched with the
//! `skill_read` tool or pinned with `/use <skill>`.

use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::skills_builtin::BUILTIN_SKILLS;

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub instructions: String,
    pub path: PathBuf,
}

pub struct SkillManager {
    pub skills: Vec<Skill>,
    vault_skills_dir: PathBuf,
    config_skills_dir: PathBuf,
}

impl SkillManager {
    pub fn new(vault_skills_dir: &Path) -> Self {
        let config_skills_dir = crate::config::Config::config_dir().join("skills");
        let mut mgr = Self {
            skills: Vec::new(),
            vault_skills_dir: vault_skills_dir.to_path_buf(),
            config_skills_dir,
        };
        mgr.seed_builtin();
        mgr.reload();
        mgr
    }

    /// Write built-in skills that do not exist yet (user edits are preserved).
    fn seed_builtin(&self) {
        if fs::create_dir_all(&self.vault_skills_dir).is_err() {
            return;
        }
        for (name, desc, tags, content) in BUILTIN_SKILLS {
            let file = self.vault_skills_dir.join(name).join("SKILL.md");
            if file.exists() {
                continue;
            }
            let _ = fs::create_dir_all(file.parent().expect("skill dir"));
            let _ = fs::write(&file, render_skill(name, desc, tags, content));
        }
    }

    fn search_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = vec![
            PathBuf::from(".qsl/skills"),
            PathBuf::from("skills"),
            self.config_skills_dir.clone(),
            self.vault_skills_dir.clone(),
        ];
        dirs.dedup();
        dirs
    }

    /// Reload from all locations; earlier directories win on name clashes,
    /// so a project-local skill overrides the vault copy.
    pub fn reload(&mut self) {
        self.skills.clear();
        for base in self.search_dirs() {
            if !base.is_dir() {
                continue;
            }
            for entry in WalkDir::new(&base)
                .max_depth(4)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                if entry.file_name() != "SKILL.md" {
                    continue;
                }
                let Ok(content) = fs::read_to_string(entry.path()) else {
                    continue;
                };
                let skill = parse_skill_file(&content, entry.path());
                if !self
                    .skills
                    .iter()
                    .any(|s| s.name.eq_ignore_ascii_case(&skill.name))
                {
                    self.skills.push(skill);
                }
            }
        }
        if self.skills.is_empty() {
            // Vault not writable: still expose the built-ins from memory.
            for (name, desc, tags, content) in BUILTIN_SKILLS {
                self.skills.push(Skill {
                    name: name.to_string(),
                    description: desc.to_string(),
                    tags: tags.iter().map(|t| t.to_string()).collect(),
                    instructions: content.to_string(),
                    path: PathBuf::from("<builtin>"),
                });
            }
        }
        self.skills.sort_by(|a, b| a.name.cmp(&b.name));
    }

    pub fn set_vault_dir(&mut self, dir: &Path) {
        self.vault_skills_dir = dir.to_path_buf();
        self.seed_builtin();
        self.reload();
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        let q = name.trim().to_lowercase();
        self.skills
            .iter()
            .find(|s| s.name.to_lowercase() == q)
            .or_else(|| {
                self.skills
                    .iter()
                    .find(|s| s.name.to_lowercase().starts_with(&q))
            })
    }

    /// One line per skill for the system prompt.
    pub fn summary(&self) -> String {
        if self.skills.is_empty() {
            return "No skills loaded.".into();
        }
        self.skills
            .iter()
            .map(|s| format!("- `{}`: {}\n", s.name, s.description))
            .collect()
    }

    /// Names only, for small context windows.
    pub fn summary_compact(&self) -> String {
        let names: Vec<&str> = self.skills.iter().map(|s| s.name.as_str()).collect();
        format!("{}\n", names.join(", "))
    }

    /// Skills whose name/tags/description overlap with the query.
    pub fn suggest(&self, query: &str, k: usize) -> Vec<&Skill> {
        let q = query.to_lowercase();
        let words: Vec<&str> = q
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() >= 3)
            .collect();
        let mut scored: Vec<(usize, &Skill)> = self
            .skills
            .iter()
            .map(|s| {
                let hay =
                    format!("{} {} {}", s.name, s.tags.join(" "), s.description).to_lowercase();
                (words.iter().filter(|w| hay.contains(*w)).count(), s)
            })
            .filter(|(n, _)| *n > 0)
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.cmp(&b.1.name)));
        scored.into_iter().take(k).map(|(_, s)| s).collect()
    }

    pub fn create(
        &mut self,
        name: &str,
        description: &str,
        tags: &[String],
        instructions: &str,
    ) -> Result<String, String> {
        let slug = slugify(name);
        if slug.is_empty() {
            return Err("skill name must contain letters or digits".into());
        }
        if instructions.trim().len() < 20 {
            return Err("skill instructions are too short to be useful".into());
        }
        let file = self.vault_skills_dir.join(&slug).join("SKILL.md");
        fs::create_dir_all(file.parent().expect("skill dir"))
            .map_err(|e| format!("failed to create skill directory: {e}"))?;
        let tag_refs: Vec<&str> = tags.iter().map(String::as_str).collect();
        let existed = file.exists();
        fs::write(
            &file,
            render_skill(&slug, description.trim(), &tag_refs, instructions.trim()),
        )
        .map_err(|e| format!("failed to write SKILL.md: {e}"))?;
        self.reload();
        Ok(format!(
            "Skill '{slug}' {} at '{}'",
            if existed { "updated" } else { "created" },
            file.display()
        ))
    }
}

fn render_skill(name: &str, desc: &str, tags: &[&str], body: &str) -> String {
    let yaml_tags = tags
        .iter()
        .map(|t| format!("\"{}\"", t.replace('"', "'")))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "---\nname: {name}\ndescription: \"{}\"\ntags: [{yaml_tags}]\nversion: 1.0.0\n---\n\n{}\n",
        desc.replace('"', "'"),
        body.trim_end()
    )
}

/// Lowercase ASCII slug: letters, digits and single dashes only. Prevents
/// `../` or absolute paths in model-supplied skill names.
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_matches('-').chars().take(64).collect()
}

fn parse_skill_file(content: &str, path: &Path) -> Skill {
    let fallback_name = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown-skill".into());
    let mut skill = Skill {
        name: fallback_name,
        description: "Custom skill".into(),
        tags: vec![],
        instructions: content.trim().to_string(),
        path: path.to_path_buf(),
    };
    let Some(rest) = content.strip_prefix("---") else {
        return skill;
    };
    let Some(end) = rest.find("\n---") else {
        return skill;
    };
    let frontmatter = &rest[..end];
    skill.instructions = rest[end + 4..].trim().to_string();
    let unquote = |s: &str| s.trim().trim_matches('"').trim_matches('\'').to_string();
    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("name:") {
            let v = unquote(v);
            if !v.is_empty() {
                skill.name = v;
            }
        } else if let Some(v) = line.strip_prefix("description:") {
            skill.description = unquote(v);
        } else if let Some(v) = line.strip_prefix("tags:") {
            skill.tags = v
                .trim()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .split(',')
                .map(unquote)
                .filter(|t| !t.is_empty())
                .collect();
        }
    }
    skill
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_and_loads_all_builtins() {
        let d = tempfile::tempdir().unwrap();
        let mgr = SkillManager::new(d.path());
        assert!(mgr.skills.len() >= BUILTIN_SKILLS.len());
        let s = mgr.get("security-audit").unwrap();
        assert!(!s.instructions.starts_with("---"));
        assert!(!s.tags.is_empty());
        assert!(mgr.get("rust").is_some(), "prefix lookup");
        assert!(mgr.summary().contains("codebase-inspection"));
    }

    #[test]
    fn create_sanitizes_name() {
        let d = tempfile::tempdir().unwrap();
        let mut mgr = SkillManager::new(d.path());
        let msg = mgr
            .create(
                "../../Evil Name!",
                "desc",
                &["x".into()],
                "Step one: do the thing carefully.",
            )
            .unwrap();
        assert!(msg.contains("evil-name"));
        assert!(d.path().join("evil-name/SKILL.md").exists());
        assert!(mgr.get("evil-name").is_some());
    }

    #[test]
    fn suggests_by_keywords() {
        let d = tempfile::tempdir().unwrap();
        let mgr = SkillManager::new(d.path());
        let s = mgr.suggest("write a Dockerfile for my docker app", 3);
        assert_eq!(s[0].name, "docker-container");
    }

    #[test]
    fn parses_frontmatter_without_tags() {
        let s = parse_skill_file(
            "---\nname: x\ndescription: 'hello'\n---\nbody",
            Path::new("/a/x/SKILL.md"),
        );
        assert_eq!(s.name, "x");
        assert_eq!(s.description, "hello");
        assert_eq!(s.instructions, "body");
    }
}
