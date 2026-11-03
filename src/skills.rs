use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// A discovered skill with name, description, tags, and procedural instructions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub instructions: String,
    pub path: PathBuf,
}

/// Manages skill discovery, seeding, and retrieval from multiple directories
pub struct SkillManager {
    pub skills: Vec<Skill>,
    pub skills_dir: PathBuf,
}

impl SkillManager {
    pub fn new(vault_path: &Path) -> Self {
        let default_skills_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("qsl-agent")
            .join("skills");

        let _ = fs::create_dir_all(&default_skills_dir);

        let mut mgr = Self {
            skills: Vec::new(),
            skills_dir: default_skills_dir,
        };

        // Seed essential built-in skills if not yet created
        mgr.seed_essential_skills(vault_path);

        // Load all skills from config dir and vault
        mgr.reload_skills(vault_path);
        mgr
    }

    /// Seed 18 essential built-in skills into the Obsidian vault
    pub fn seed_essential_skills(&self, vault_path: &Path) {
        let vault_skills = vault_path.join("QSL_Agent").join("Skills");
        let _ = fs::create_dir_all(&vault_skills);

        let essential = vec![
            (
                "codebase-inspection",
                "Inspect codebases, repository metrics, lines of code, and architectural composition.",
                vec!["code", "inspection", "metrics", "architecture"],
                r#"# Codebase Inspection Skill (2026 Edition)

## Purpose
Systematically inspect projects and codebases to map directory hierarchies, lines of code, language distributions, and key module responsibilities.

## Guidelines
1. Start with `list_dir` or `execute_command("find . -maxdepth 2 -not -path '*/.*'")` to identify top-level modules.
2. Read project manifests (`Cargo.toml`, `package.json`, `pyproject.toml`, `go.mod`) to understand external dependencies.
3. Use `execute_command` with ripgrep (`rg`) or `find` to pinpoint core entry points (e.g. `main.rs`, `index.ts`, `app.py`).
4. Count lines: `execute_command("find . -name '*.rs' | xargs wc -l | tail -1")`.
5. Document architectural findings in Obsidian under `QSL_Agent/Architecture/<project_name>.md`.
"#,
            ),
            (
                "software-development",
                "Systematic debugging, test-driven development (TDD), and clean refactoring rules.",
                vec!["debugging", "tdd", "refactoring", "rust", "software"],
                r#"# Software Development & Debugging Skill (2026 Edition)

## Purpose
Enforce robust, defect-free software engineering practices with zero regressions.

## Guidelines
1. **Hypothesis First**: Formulate explicit root-cause hypotheses before editing any code.
2. **Minimal Surgical Changes**: Make targeted edits with `edit_file` rather than rewriting entire files.
3. **Verify Immediately**: Run test suites (`cargo test`, `pytest`, `npm test`) immediately after each modification.
4. **Preserve Documentation**: Maintain existing comments, contracts, and safety invariants.
5. **Version Before Modify**: Use `read_file` to inspect current state before any edit.
6. **One Change at a Time**: Apply changes incrementally and verify each step.
"#,
            ),
            (
                "security-audit",
                "Audit local code and shell commands for security vulnerabilities, secrets leaks, and injections.",
                vec!["security", "audit", "injection", "secrets", "cve"],
                r#"# Security Audit Skill (2026 Edition)

## Purpose
Proactively inspect code and configurations for credential leakage, injection vulnerabilities, and privilege boundary breaches.

## Guidelines
1. Scan for hardcoded credentials (API keys, private tokens, passwords, private SSH keys).
2. Validate user input sanitization and command injection risks in shell invocations.
3. Review file permissions (`chmod`, `chown`) and check for improper file write targets.
4. Check for outdated dependencies with known CVEs (`cargo audit`, `npm audit`).
5. If a vulnerability is found, log it in Obsidian under `QSL_Agent/Security/` with severity, affected files, and remediation.
"#,
            ),
            (
                "obsidian-vault-manager",
                "Advanced Obsidian knowledge management: bidirectional linking, taxonomies, and daily logs.",
                vec!["obsidian", "pkm", "knowledge", "notes", "markdown"],
                r#"# Obsidian Vault Management Skill (2026 Edition)

## Purpose
Maintain a pristine, interconnected personal knowledge base inside the user's Obsidian vault.

## Guidelines
1. Use bidirectional wikilinks `[[Topic Name]]` to connect concepts and related project nodes.
2. Use YAML frontmatter at the top of notes for tags, dates, and aliases:
   ```yaml
   ---
   title: Topic Name
   date: 2026-11-03
   tags: [agent, dev, rust]
   ---
   ```
3. Keep daily notes in `QSL_Agent/Daily/YYYY-MM-DD.md` for task logs and turn records.
4. Keep core persona and operational rules in `QSL_Agent/Profile.md`.
5. Organize research output under `QSL_Agent/Research/` with source URLs and date stamps.
"#,
            ),
            (
                "system-ops",
                "Arch Linux system administration, pacman/yay package status, process inspection, and resource telemetry.",
                vec!["linux", "arch", "systemd", "pacman", "performance"],
                r#"# System Operations Skill (2026 Edition)

## Purpose
Safely monitor and inspect the local Linux environment (Arch Linux / Hyprland).

## Guidelines
1. Inspect running services via `systemctl --user status <service>` or `systemctl status <service>`.
2. Check resource pressure with `free -h`, `df -h`, and process lists (`ps aux --sort=-%mem | head -20`).
3. For package queries, use read-only flags: `pacman -Q <pkg>` or `pacman -Ss <query>`.
4. Check uptime and load: `uptime`, `cat /proc/loadavg`.
5. Inspect network: `ip addr`, `ss -tuln`.
6. Never run destructive system operations without explicit user confirmation.
"#,
            ),
            (
                "git-workflow",
                "Professional Git version control: atomic commits, branch management, and diff reviews.",
                vec!["git", "commits", "branches", "diff", "vcs"],
                r#"# Git Workflow Skill (2026 Edition)

## Purpose
Handle version control operations cleanly with atomic, well-formatted commit messages.

## Guidelines
1. Always run `git status` and `git diff` before committing to inspect unstaged changes.
2. Format commit messages using Conventional Commits: `feat:`, `fix:`, `refactor:`, `docs:`, `chore:`.
3. Never use `git push --force` on main/master branches.
4. Never commit `.env`, private keys, or build artifacts (`target/`, `node_modules/`).
5. Use `git log --oneline -20` to review recent history.
6. Before merging, review diffs with `git diff main..feature-branch`.
"#,
            ),
            (
                "rust-cargo",
                "Rust development, Cargo dependencies, clippy linting, audit, and performance optimization.",
                vec!["rust", "cargo", "clippy", "optimization", "performance"],
                r#"# Rust & Cargo Skill (2026 Edition)

## Purpose
Build, lint, test, and optimize ultra-fast Rust codebases with zero memory leaks.

## Guidelines
1. Check syntax and dependencies quickly using `cargo check`.
2. Run strict linting with `cargo clippy --all-targets -- -D warnings`.
3. Check for security vulnerabilities in dependencies with `cargo audit` (if installed).
4. Run benchmarks or release profiling with `cargo build --release`.
5. Use `cargo tree` to inspect dependency graph and bloat.
6. Profile binary size: `ls -lh target/release/<binary>`.
"#,
            ),
            (
                "docker-container",
                "Container architecture, Dockerfile best practices, multi-stage builds, and container isolation.",
                vec!["docker", "containers", "devops", "cloud", "isolation"],
                r#"# Docker & Container Skill (2026 Edition)

## Purpose
Inspect, optimize, and maintain robust, lightweight container definitions.

## Guidelines
1. Ensure Dockerfiles use minimal base images (`alpine`, `distroless`, `debian-slim`).
2. Implement multi-stage builds to avoid leaking compilers and private build credentials into final images.
3. Validate `.dockerignore` excludes git history, sensitive keys, and temporary artifacts.
4. Never run containers as root without explicit user directive.
5. Check running containers: `docker ps`, `docker images`.
"#,
            ),
            (
                "python-env",
                "Python development, uv/virtual environments, dependency isolation, and pyproject.toml.",
                vec!["python", "uv", "venv", "pip", "packaging"],
                r#"# Python Environment Skill (2026 Edition)

## Purpose
Manage Python projects, isolated virtualenvs, and modern packaging without breaking system site-packages.

## Guidelines
1. Prefer fast tooling like `uv` or `python -m venv` for virtual environment creation.
2. Never install packages into global system Python directly without `--break-system-packages` awareness.
3. Track dependencies in standard `pyproject.toml` or `requirements.txt`.
4. Check installed packages: `pip list` or `uv pip list`.
5. Verify Python version: `python3 --version`.
"#,
            ),
            (
                "api-design",
                "RESTful APIs, OpenAPI specifications, JSON schemas, error response contracts, and endpoint design.",
                vec!["api", "rest", "json", "openapi", "contracts"],
                r#"# API Design Skill (2026 Edition)

## Purpose
Design predictable, consistent, and backward-compatible REST and RPC APIs.

## Guidelines
1. Use consistent HTTP verbs (`GET`, `POST`, `PUT`, `PATCH`, `DELETE`) with semantically accurate status codes.
2. Define structured JSON error response payloads: `{"error": {"code": "...", "message": "..."}}`.
3. Validate request parameters and payload boundaries before processing.
4. Version APIs appropriately: `/v1/`, `/v2/` or via Accept header.
5. Document all endpoints with examples.
"#,
            ),
            (
                "database-sql",
                "SQLite/PostgreSQL schema analysis, query optimization, indexing, and transactional integrity.",
                vec!["database", "sql", "sqlite", "postgres", "queries"],
                r#"# Database & SQL Skill (2026 Edition)

## Purpose
Query, inspect, and optimize local databases while ensuring strict schema and data integrity.

## Guidelines
1. Always run schema queries (`.schema`, `\d`, or `PRAGMA table_info`) before constructing queries.
2. Use parameterized queries to prevent SQL injection vulnerabilities.
3. Wrap multi-table modifications inside explicit transactions (`BEGIN TRANSACTION ... COMMIT`).
4. Check index usage with `EXPLAIN QUERY PLAN` (SQLite) or `EXPLAIN ANALYZE` (PostgreSQL).
5. Back up databases before destructive DDL changes.
"#,
            ),
            (
                "web-research",
                "Offline-first documentation lookup, curl API requests, and web data extraction.",
                vec!["web", "curl", "research", "api", "docs"],
                r#"# Web Research Skill (2026 Edition)

## Purpose
Fetch documentation, query public APIs, and gather external technical references using local CLI tools.

## Guidelines
1. Use `curl -s` with clean timeouts (`--max-time 10`) to fetch documentation or endpoints.
2. Parse JSON responses using `jq` or built-in agent JSON tools.
3. Save distilled reference notes into Obsidian under `QSL_Agent/Research/` with source URLs and date stamps.
4. Always verify SSL certificates unless explicitly testing.
"#,
            ),
            (
                "data-analysis",
                "CSV/JSON data inspection, statistical summaries, and format transformation pipelines.",
                vec!["data", "csv", "json", "analysis", "statistics"],
                r#"# Data Analysis Skill (2026 Edition)

## Purpose
Inspect, transform, and summarize structured data files on the local filesystem.

## Guidelines
1. Use `head`, `tail`, `wc -l` to quickly assess file size and structure.
2. For CSV: `column -s, -t < file.csv | head -20` for formatted preview.
3. For JSON: `cat file.json | jq '.' | head -50` for pretty-printed inspection.
4. Use `awk`, `sort`, `uniq -c` for frequency analysis and aggregation.
5. Save analysis results to Obsidian under `QSL_Agent/Research/` with dataset name and date.
6. Always validate data integrity before transformation (check for nulls, encoding issues).
"#,
            ),
            (
                "network-diagnostics",
                "Network connectivity testing, port scanning, DNS resolution, and HTTP endpoint health checks.",
                vec!["network", "dns", "ports", "connectivity", "http"],
                r#"# Network Diagnostics Skill (2026 Edition)

## Purpose
Diagnose local and remote network connectivity issues using standard CLI utilities.

## Guidelines
1. Test basic connectivity: `ping -c 3 <host>`.
2. Check open ports: `ss -tuln` or `netstat -tuln`.
3. DNS resolution: `dig <domain>` or `nslookup <domain>`.
4. HTTP endpoint health: `curl -sI --max-time 5 <url>`.
5. Trace routes: `traceroute <host>` or `mtr --report <host>`.
6. Check firewall rules: `iptables -L -n` or `nft list ruleset` (requires privileges).
7. Always use timeouts on network commands to prevent hangs.
"#,
            ),
            (
                "text-processing",
                "Text manipulation with sed, awk, grep, and regular expressions for file transformation.",
                vec!["text", "sed", "awk", "grep", "regex"],
                r#"# Text Processing Skill (2026 Edition)

## Purpose
Efficiently search, filter, transform, and extract information from text files and command output.

## Guidelines
1. Search with context: `grep -n -C 3 'pattern' file` for matches with surrounding lines.
2. Use `ripgrep` (`rg`) when available for faster recursive searches.
3. Stream processing: `awk '{print $1, $3}'` for column extraction.
4. In-place edits: `sed -i 's/old/new/g' file` (use with caution, always preview first).
5. Count patterns: `grep -c 'pattern' file`.
6. Multi-file search: `rg 'pattern' --type rust` or `grep -r 'pattern' --include='*.rs'`.
7. Always preview transformations with a dry run before applying.
"#,
            ),
            (
                "yaml-toml-config",
                "YAML, TOML, and JSON configuration file inspection, validation, and editing.",
                vec!["yaml", "toml", "json", "config", "validation"],
                r#"# YAML/TOML/JSON Config Skill (2026 Edition)

## Purpose
Inspect, validate, and safely modify configuration files in common structured formats.

## Guidelines
1. Validate YAML: `python3 -c "import yaml; yaml.safe_load(open('file.yml'))"`.
2. Validate TOML: use `taplo check` or read with `cargo` if available.
3. Validate JSON: `python3 -m json.tool < file.json` or `jq '.' file.json`.
4. Always create a backup before modifying config files.
5. Use `diff` to compare original and modified configurations.
6. Check for syntax errors, missing required keys, and type mismatches.
"#,
            ),
            (
                "markdown-docs",
                "Technical documentation writing, README generation, and Markdown formatting best practices.",
                vec!["markdown", "docs", "readme", "writing", "technical"],
                r#"# Markdown Documentation Skill (2026 Edition)

## Purpose
Create professional, well-structured technical documentation in Markdown format.

## Guidelines
1. Use proper heading hierarchy (H1 for title, H2 for sections, H3 for subsections).
2. Include a table of contents for documents longer than 3 sections.
3. Use fenced code blocks with language specifiers for all code examples.
4. Add badges for build status, version, and license at the top of READMEs.
5. Include installation, usage, configuration, and contributing sections.
6. Use tables for feature comparisons and configuration references.
7. Proofread for grammar, consistency, and broken links.
"#,
            ),
            (
                "file-organizer",
                "File system organization, directory structure cleanup, and project scaffolding.",
                vec!["files", "organize", "scaffold", "cleanup", "structure"],
                r#"# File Organizer Skill (2026 Edition)

## Purpose
Organize project directories, clean up file structures, and scaffold new projects efficiently.

## Guidelines
1. Analyze current structure: `find . -maxdepth 3 -type f | head -50` and `tree -L 2` (if available).
2. Group related files by function (src/, docs/, tests/, config/, scripts/).
3. Remove stale artifacts: identify but confirm with user before deleting.
4. Ensure `.gitignore` covers build artifacts, IDE configs, and secrets.
5. Create missing standard files: README.md, LICENSE, .gitignore, .editorconfig.
6. Follow language-specific conventions (Cargo.toml for Rust, package.json for Node.js).
"#,
            ),
        ];

        for (name, desc, tags, content) in essential {
            let skill_dir = vault_skills.join(name);
            let skill_file = skill_dir.join("SKILL.md");
            if !skill_file.exists() {
                let _ = fs::create_dir_all(&skill_dir);
                let yaml_tags = tags
                    .iter()
                    .map(|t| format!("\"{}\"", t))
                    .collect::<Vec<_>>()
                    .join(", ");
                let header = format!(
                    "---\nname: {}\ndescription: \"{}\"\ntags: [{}]\nversion: 1.0.0\nyear: 2026\n---\n\n{}",
                    name, desc, yaml_tags, content
                );
                let _ = fs::write(&skill_file, header);
            }
        }
    }

    /// Reload all skills from all known directories
    pub fn reload_skills(&mut self, vault_path: &Path) {
        self.skills.clear();

        let search_dirs = vec![
            self.skills_dir.clone(),
            vault_path.join("QSL_Agent").join("Skills"),
            PathBuf::from("./skills"),
        ];

        for base_dir in search_dirs {
            if !base_dir.exists() {
                continue;
            }

            for entry in WalkDir::new(&base_dir)
                .max_depth(4)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                if entry.file_name() == "SKILL.md" {
                    if let Ok(content) = fs::read_to_string(entry.path()) {
                        let (name, desc, tags, instructions) =
                            Self::parse_skill_file(&content, entry.path());
                        if !self.skills.iter().any(|s| s.name == name) {
                            self.skills.push(Skill {
                                name,
                                description: desc,
                                tags,
                                instructions,
                                path: entry.path().to_path_buf(),
                            });
                        }
                    }
                }
            }
        }

        // Sort skills alphabetically
        self.skills.sort_by(|a, b| a.name.cmp(&b.name));
    }

    /// Parse a SKILL.md file extracting YAML frontmatter and markdown body
    fn parse_skill_file(content: &str, path: &Path) -> (String, String, Vec<String>, String) {
        let fallback_name = path
            .parent()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown-skill".to_string());

        if !content.starts_with("---") {
            return (
                fallback_name,
                "Custom skill".to_string(),
                vec![],
                content.to_string(),
            );
        }

        let parts: Vec<&str> = content.splitn(3, "---").collect();
        if parts.len() < 3 {
            return (
                fallback_name,
                "Custom skill".to_string(),
                vec![],
                content.to_string(),
            );
        }

        let frontmatter = parts[1];
        let instructions = parts[2].trim().to_string();

        let mut name = fallback_name;
        let mut description = "Custom skill".to_string();
        let mut tags = Vec::new();

        for line in frontmatter.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("name:") {
                name = rest.trim().trim_matches('"').trim_matches('\'').to_string();
            } else if let Some(rest) = line.strip_prefix("description:") {
                description = rest.trim().trim_matches('"').trim_matches('\'').to_string();
            } else if let Some(rest) = line.strip_prefix("tags:") {
                let clean = rest.trim().trim_matches('[').trim_matches(']');
                tags = clean
                    .split(',')
                    .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }

        (name, description, tags, instructions)
    }

    /// Return a formatted summary of all loaded skills
    pub fn list_skills_summary(&self) -> String {
        if self.skills.is_empty() {
            return "No skills loaded.".to_string();
        }
        let mut summary = String::new();
        for s in &self.skills {
            summary.push_str(&format!("- **`{}`**: {}\n", s.name, s.description));
        }
        summary
    }

    /// Get the instructions for a specific skill by name (case-insensitive)
    pub fn get_skill_instructions(&self, name: &str) -> Option<String> {
        let query = name.to_lowercase();
        self.skills
            .iter()
            .find(|s| s.name.to_lowercase() == query)
            .map(|s| s.instructions.clone())
    }

    /// Create and persist a new skill into the Obsidian vault
    pub fn create_skill(
        &mut self,
        name: &str,
        description: &str,
        tags: &[String],
        instructions: &str,
        vault_path: &Path,
    ) -> Result<String, String> {
        let target_dir = vault_path.join("QSL_Agent").join("Skills").join(name);
        fs::create_dir_all(&target_dir)
            .map_err(|e| format!("Failed to create skill directory: {}", e))?;

        let skill_file = target_dir.join("SKILL.md");
        let yaml_tags = tags
            .iter()
            .map(|t| format!("\"{}\"", t))
            .collect::<Vec<_>>()
            .join(", ");

        let content = format!(
            "---\nname: {}\ndescription: \"{}\"\ntags: [{}]\nversion: 1.0.0\nyear: 2026\n---\n\n{}",
            name, description, yaml_tags, instructions.trim()
        );

        fs::write(&skill_file, content)
            .map_err(|e| format!("Failed to write SKILL.md: {}", e))?;

        self.reload_skills(vault_path);
        Ok(format!(
            "Skill '{}' created successfully at '{}'",
            name,
            skill_file.display()
        ))
    }
}
