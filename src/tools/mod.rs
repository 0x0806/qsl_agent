//! Tool definitions and dispatch.
//!
//! Each tool is declared once in `specs()` (schema shown to the model) and
//! executed in `execute()`. Argument extraction accepts common aliases
//! (`file_path` for `path`, `old_string` for `target`, ...) because small
//! local models frequently guess parameter names.

pub mod fs;
pub mod shell;
pub mod web;

use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;

use crate::checkpoint::Checkpoints;
use crate::interrupt::Interrupt;
use crate::memory::ObsidianMemory;
use crate::provider::ToolSpec;
use crate::security::SecurityGuard;
use crate::skills::SkillManager;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolClass {
    /// No side effects; never needs approval; may run concurrently.
    ReadOnly,
    /// Changes files, memory or system state; goes through approval.
    Mutating,
}

pub fn class_of(name: &str) -> ToolClass {
    match name {
        "execute_command" | "write_file" | "append_file" | "edit_file" | "obsidian_write"
        | "obsidian_append" | "memory_forget" | "skill_create" => ToolClass::Mutating,
        _ => ToolClass::ReadOnly,
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TodoItem {
    pub content: String,
    pub status: String,
}

/// Everything a tool may touch. All fields are shared references so
/// read-only tools can run concurrently.
pub struct ToolCtx<'a> {
    pub security: &'a SecurityGuard,
    pub memory: &'a ObsidianMemory,
    pub skills: &'a Mutex<SkillManager>,
    pub todos: &'a Mutex<Vec<TodoItem>>,
    pub checkpoints: &'a Checkpoints,
    pub http: &'a reqwest::Client,
    pub sessions_dir: PathBuf,
    pub turn: u64,
    pub command_timeout_secs: u64,
    pub max_output_chars: usize,
    pub interrupt: &'a Interrupt,
}

pub struct SpecOptions {
    pub allow_web: bool,
    pub allow_delegate: bool,
    pub save_sessions: bool,
}

pub fn specs(opts: &SpecOptions) -> Vec<ToolSpec> {
    let mut v = vec![
        spec(
            "execute_command",
            "Run a shell command (bash) on the local machine. Returns exit code, stdout and stderr. Use for builds, tests, git, \
             package queries and inspection. Dangerous commands are blocked or require user approval.",
            json!({
                "command": { "type": "string", "description": "The exact shell command to execute" },
                "workdir": { "type": "string", "description": "Optional working directory (default: current directory)" },
                "timeout": { "type": "integer", "description": "Optional timeout in seconds" }
            }),
            &["command"],
        ),
        spec(
            "read_file",
            "Read a text file. For large files pass start_line/end_line (1-based, inclusive) to read a slice.",
            json!({
                "path": { "type": "string", "description": "File path (absolute, relative or ~/...)" },
                "start_line": { "type": "integer", "description": "Optional first line (1-based)" },
                "end_line": { "type": "integer", "description": "Optional last line (inclusive)" }
            }),
            &["path"],
        ),
        spec(
            "write_file",
            "Create or overwrite a file with the given content (parent folders are created). Prefer edit_file for small changes \
             to existing files. For files over ~200 lines, write a first part then use append_file.",
            json!({
                "path": { "type": "string", "description": "File path to create or overwrite" },
                "content": { "type": "string", "description": "Full file content" }
            }),
            &["path", "content"],
        ),
        spec(
            "append_file",
            "Append content to a file (created if missing). Use to build large files incrementally across several calls.",
            json!({
                "path": { "type": "string", "description": "File path" },
                "content": { "type": "string", "description": "Text to append" }
            }),
            &["path", "content"],
        ),
        spec(
            "edit_file",
            "Replace an exact text snippet in a file. `target` must match the file exactly (copy it from read_file) and be unique \
             unless replace_all is true. Include enough surrounding lines to make it unique.",
            json!({
                "path": { "type": "string", "description": "File to edit" },
                "target": { "type": "string", "description": "Exact existing text to replace" },
                "replacement": { "type": "string", "description": "New text" },
                "replace_all": { "type": "boolean", "description": "Replace every occurrence (default false)" }
            }),
            &["path", "target", "replacement"],
        ),
        spec(
            "list_dir",
            "List a directory with entry types and sizes. depth 1-3 (default 1).",
            json!({
                "path": { "type": "string", "description": "Directory (default: current directory)" },
                "depth": { "type": "integer", "description": "Recursion depth 1-3" }
            }),
            &[],
        ),
        spec(
            "search_files",
            "Search file contents recursively with a regular expression (like ripgrep). Skips .git, target, node_modules, \
             hidden and binary files. Returns path:line: text.",
            json!({
                "pattern": { "type": "string", "description": "Regex (or plain text) to search for" },
                "path": { "type": "string", "description": "Directory or file to search (default: .)" },
                "glob": { "type": "string", "description": "Optional filename filter, e.g. *.rs" },
                "ignore_case": { "type": "boolean", "description": "Case-insensitive search" },
                "max_results": { "type": "integer", "description": "Maximum matches (default 60)" }
            }),
            &["pattern"],
        ),
        spec(
            "find_files",
            "Find files by name using a glob pattern (e.g. *.toml, **/test_*.py).",
            json!({
                "pattern": { "type": "string", "description": "Glob pattern" },
                "path": { "type": "string", "description": "Root directory (default: .)" }
            }),
            &["pattern"],
        ),
        spec(
            "todo",
            "Maintain a task plan for multi-step work. Call with the full list to replace it (statuses: pending, in_progress, \
             completed, cancelled); call without arguments to read it. Keep exactly one item in_progress.",
            json!({
                "todos": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": { "content": { "type": "string" }, "status": { "type": "string" } },
                        "required": ["content", "status"]
                    }
                }
            }),
            &[],
        ),
        spec(
            "obsidian_search",
            "Full-text search across all Markdown notes in the Obsidian vault.",
            json!({ "query": { "type": "string", "description": "Keyword or phrase" } }),
            &["query"],
        ),
        spec(
            "obsidian_read",
            "Read a note from the Obsidian vault by vault-relative path (e.g. 'QSL_Agent/Daily/2026-11-03').",
            json!({ "note_path": { "type": "string", "description": "Vault-relative note path" } }),
            &["note_path"],
        ),
        spec(
            "obsidian_write",
            "Create, overwrite or append to a Markdown note in the Obsidian vault. Use [[wikilinks]] and YAML frontmatter.",
            json!({
                "note_path": { "type": "string", "description": "Vault-relative path, e.g. 'QSL_Agent/Research/topic'" },
                "content": { "type": "string", "description": "Markdown content" },
                "append": { "type": "boolean", "description": "Append instead of overwrite" }
            }),
            &["note_path", "content"],
        ),
        spec(
            "obsidian_list",
            "List notes in the vault, optionally inside a subfolder.",
            json!({ "subfolder": { "type": "string", "description": "Optional vault-relative folder" } }),
            &[],
        ),
        spec(
            "obsidian_append",
            "Save a durable fact to long-term memory (QSL_Agent/Memories.md): user preferences, environment details, project \
             conventions, lessons learned. One concise fact per call. Do not store secrets.",
            json!({ "memory": { "type": "string", "description": "The fact to remember" } }),
            &["memory"],
        ),
        spec(
            "memory_search",
            "Search long-term memories for facts relevant to a query.",
            json!({ "query": { "type": "string", "description": "What to look for" } }),
            &["query"],
        ),
        spec(
            "memory_forget",
            "Delete long-term memory entries that contain the given text (use when a stored fact is wrong or outdated).",
            json!({ "match": { "type": "string", "description": "Text identifying the entries to remove" } }),
            &["match"],
        ),
        spec("skill_list", "List available skills (procedural playbooks) with descriptions.", json!({}), &[]),
        spec(
            "skill_read",
            "Load the step-by-step instructions of a skill. Do this before specialised tasks. Long skills come in pages (part=2, 3, ...); \
             skills with companion files list them, and you read one with file=\"NAME.md\".",
            json!({
                "name": { "type": "string", "description": "Skill name, e.g. 'rust-cargo'" },
                "file": { "type": "string", "description": "Optional companion file inside the skill directory, e.g. 'HUNTING.md'" },
                "part": { "type": "integer", "description": "Page number for long text (default 1)" }
            }),
            &["name"],
        ),
        spec(
            "skill_create",
            "Save a reusable procedure as a new skill after solving a non-trivial task, so it can be reused later.",
            json!({
                "name": { "type": "string", "description": "Short slug, e.g. 'deploy-hugo-site'" },
                "description": { "type": "string", "description": "One-line description" },
                "tags": { "type": "array", "items": { "type": "string" } },
                "instructions": { "type": "string", "description": "Markdown procedure with numbered steps" }
            }),
            &["name", "description", "instructions"],
        ),
    ];
    if opts.save_sessions {
        v.push(spec(
            "session_search",
            "Search past conversations (other sessions) for a keyword; returns matching snippets with session ids.",
            json!({ "query": { "type": "string", "description": "Keyword or phrase" } }),
            &["query"],
        ));
    }
    if opts.allow_web {
        v.push(spec(
            "web_fetch",
            "Fetch a web page or API URL via HTTP GET and return readable text (HTML is converted to text).",
            json!({
                "url": { "type": "string", "description": "http(s) URL" },
                "max_chars": { "type": "integer", "description": "Maximum characters to return (default 12000)" }
            }),
            &["url"],
        ));
    }
    if opts.allow_delegate {
        v.push(spec(
            "delegate_task",
            "Hand a self-contained subtask (research, analysis, investigation) to a sub-agent with a fresh context. It can use \
             the same tools and returns a concise report. Give it all the context it needs.",
            json!({
                "task": { "type": "string", "description": "What the sub-agent should do and report back" },
                "context": { "type": "string", "description": "Relevant facts, paths, constraints" }
            }),
            &["task"],
        ));
    }
    v
}

fn spec(name: &'static str, description: &str, properties: Value, required: &[&str]) -> ToolSpec {
    ToolSpec {
        name,
        description: description.to_string(),
        parameters: json!({ "type": "object", "properties": properties, "required": required }),
    }
}

// ---------------------------------------------------------------------------
// Argument helpers
// ---------------------------------------------------------------------------

/// First present string argument among `keys` (numbers/bools are stringified).
pub fn arg_str(args: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| match args.get(*k) {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        Some(Value::Bool(b)) => Some(b.to_string()),
        _ => None,
    })
}

pub fn req_str(args: &Value, keys: &[&str]) -> Result<String, String> {
    arg_str(args, keys).ok_or_else(|| format!("missing required argument '{}'", keys[0]))
}

pub fn arg_u64(args: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|k| match args.get(*k) {
        Some(Value::Number(n)) => n.as_u64().or_else(|| n.as_f64().map(|f| f.max(0.0) as u64)),
        Some(Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    })
}

pub fn arg_bool(args: &Value, keys: &[&str]) -> bool {
    keys.iter().any(|k| match args.get(*k) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => matches!(s.to_lowercase().as_str(), "true" | "yes" | "1"),
        Some(Value::Number(n)) => n.as_u64() == Some(1),
        _ => false,
    })
}

pub const PATH_KEYS: &[&str] = &[
    "path",
    "file_path",
    "filepath",
    "file",
    "filename",
    "target_file",
];

/// Parse model-produced tool arguments, repairing common defects: code
/// fences, trailing commas, double-encoded JSON strings, single quotes and
/// truncated closing braces.
pub fn parse_args(raw: &str) -> Result<Value, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(json!({}));
    }
    let attempt = |s: &str| -> Option<Value> {
        let v: Value = serde_json::from_str(s).ok()?;
        match v {
            Value::Object(_) => Some(v),
            Value::String(inner) => serde_json::from_str::<Value>(&inner)
                .ok()
                .filter(Value::is_object),
            Value::Null => Some(json!({})),
            _ => None,
        }
    };
    if let Some(v) = attempt(trimmed) {
        return Ok(v);
    }
    let mut s = trimmed
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
        .to_string();
    if let (Some(a), Some(b)) = (s.find('{'), s.rfind('}')) {
        if a < b {
            s = s[a..=b].to_string();
        }
    }
    let no_trailing = regex::Regex::new(r",\s*([}\]])")
        .expect("regex")
        .replace_all(&s, "$1")
        .to_string();
    if let Some(v) = attempt(&no_trailing) {
        return Ok(v);
    }
    if !no_trailing.contains('"') && no_trailing.contains('\'') {
        if let Some(v) = attempt(&no_trailing.replace('\'', "\"")) {
            return Ok(v);
        }
    }
    // Close an unterminated string and any open braces/brackets (truncation).
    let mut repaired = no_trailing.clone();
    let mut in_str = false;
    let mut escaped = false;
    let mut stack = Vec::new();
    for c in repaired.chars() {
        if in_str {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => stack.push('}'),
            '[' => stack.push(']'),
            '}' | ']' => {
                stack.pop();
            }
            _ => {}
        }
    }
    if in_str {
        repaired.push('"');
    }
    while let Some(c) = stack.pop() {
        repaired.push(c);
    }
    attempt(&repaired).ok_or_else(|| {
        format!(
            "arguments are not valid JSON ({}). Re-issue the call with a valid JSON object.",
            serde_json::from_str::<Value>(trimmed)
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default()
        )
    })
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

pub async fn execute(name: &str, args: &Value, ctx: &ToolCtx<'_>) -> Result<String, String> {
    match name {
        "execute_command" => shell::execute_command(args, ctx).await,
        "read_file" => fs::read_file(args, ctx),
        "write_file" => fs::write_file(args, ctx, false),
        "append_file" => fs::write_file(args, ctx, true),
        "edit_file" => fs::edit_file(args, ctx),
        "list_dir" => fs::list_dir(args, ctx),
        "search_files" => {
            let args = args.clone();
            let sec_ok =
                arg_str(&args, &["path", "dir", "directory"]).unwrap_or_else(|| ".".into());
            ctx.security.check_read(&sec_ok)?;
            let max = ctx.max_output_chars;
            tokio::task::spawn_blocking(move || fs::search_files(&args, max))
                .await
                .map_err(|e| e.to_string())?
        }
        "find_files" => {
            let args = args.clone();
            let root = arg_str(&args, &["path", "dir", "directory"]).unwrap_or_else(|| ".".into());
            ctx.security.check_read(&root)?;
            tokio::task::spawn_blocking(move || fs::find_files(&args))
                .await
                .map_err(|e| e.to_string())?
        }
        "todo" => todo(args, ctx),
        "obsidian_search" => {
            let q = req_str(args, &["query", "q", "text"])?;
            let hits = ctx.memory.search_vault(&q, 40);
            if hits.is_empty() {
                Ok(format!("No matches for '{q}' in the Obsidian vault."))
            } else {
                Ok(format!(
                    "Found {} match(es):\n{}",
                    hits.len(),
                    hits.iter()
                        .map(|(p, l, t)| format!("{p}:{l}: {}", t.trim()))
                        .collect::<Vec<_>>()
                        .join("\n")
                ))
            }
        }
        "obsidian_read" => ctx
            .memory
            .read_note(&req_str(args, &["note_path", "path", "note", "filename"])?),
        "obsidian_write" => {
            let p = req_str(args, &["note_path", "path", "note", "filename"])?;
            let c = req_str(args, &["content", "text", "body"])?;
            ctx.memory.write_note(&p, &c, arg_bool(args, &["append"]))
        }
        "obsidian_list" => {
            let notes = ctx
                .memory
                .list_notes(arg_str(args, &["subfolder", "folder", "path"]).as_deref());
            if notes.is_empty() {
                Ok("No notes found in that location.".into())
            } else {
                let shown: Vec<_> = notes.iter().take(300).cloned().collect();
                Ok(format!(
                    "{} note(s){}:\n{}",
                    notes.len(),
                    if notes.len() > 300 {
                        " (first 300)"
                    } else {
                        ""
                    },
                    shown.join("\n")
                ))
            }
        }
        "obsidian_append" => {
            let m = req_str(args, &["memory", "content", "text", "fact", "note"])?;
            ctx.memory
                .append_memory(&crate::security::redact_secrets(&m))
        }
        "memory_search" => {
            let q = req_str(args, &["query", "q", "text"])?;
            let hits = ctx.memory.search_memories(&q, 12);
            if hits.is_empty() {
                Ok(format!("No stored memories match '{q}'."))
            } else {
                Ok(hits.join("\n"))
            }
        }
        "memory_forget" => {
            let m = req_str(args, &["match", "query", "text", "memory"])?;
            let n = ctx.memory.forget(&m)?;
            Ok(format!(
                "Removed {n} memory entr{}.",
                if n == 1 { "y" } else { "ies" }
            ))
        }
        "skill_list" => Ok(ctx
            .skills
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .summary()),
        "skill_read" => {
            let n = req_str(args, &["name", "skill_name", "skill"])?;
            let skills = ctx.skills.lock().unwrap_or_else(|e| e.into_inner());
            match skills.get(&n) {
                Some(s) => {
                    let file = arg_str(args, &["file", "path"]);
                    let part = args
                        .get("part")
                        .and_then(|p| {
                            p.as_u64()
                                .or_else(|| p.as_str().and_then(|x| x.parse().ok()))
                        })
                        .unwrap_or(1) as usize;
                    s.read(file.as_deref(), part)
                }
                None => {
                    let near: Vec<String> = skills
                        .suggest(&n, 4)
                        .iter()
                        .map(|s| s.name.clone())
                        .collect();
                    Err(format!(
                        "skill '{n}' not found. Similar: {}. Use skill_list.",
                        if near.is_empty() {
                            "none".into()
                        } else {
                            near.join(", ")
                        }
                    ))
                }
            }
        }
        "skill_create" => {
            let n = req_str(args, &["name", "skill_name"])?;
            let d =
                arg_str(args, &["description", "desc"]).unwrap_or_else(|| "Custom skill".into());
            let i = req_str(args, &["instructions", "content", "body"])?;
            let tags: Vec<String> = match args.get("tags") {
                Some(Value::Array(a)) => a
                    .iter()
                    .filter_map(|t| t.as_str().map(String::from))
                    .collect(),
                Some(Value::String(s)) => s
                    .split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect(),
                _ => vec![],
            };
            ctx.skills
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .create(&n, &d, &tags, &i)
        }
        "session_search" => {
            let q = req_str(args, &["query", "q", "text"])?;
            let dir = ctx.sessions_dir.clone();
            tokio::task::spawn_blocking(move || crate::session::search_sessions(&dir, &q, 20))
                .await
                .map_err(|e| e.to_string())?
        }
        "web_fetch" => web::web_fetch(args, ctx).await,
        other => Err(format!(
            "unknown tool '{other}'. Available tools are listed in your instructions."
        )),
    }
}

fn todo(args: &Value, ctx: &ToolCtx<'_>) -> Result<String, String> {
    let mut todos = ctx.todos.lock().unwrap_or_else(|e| e.into_inner());
    let incoming = match args.get("todos").or_else(|| args.get("items")) {
        Some(Value::Array(a)) => Some(a.clone()),
        Some(Value::String(s)) => serde_json::from_str::<Vec<Value>>(s).ok(),
        _ => None,
    };
    if let Some(items) = incoming {
        let mut next = Vec::new();
        for it in items {
            let content = match &it {
                Value::String(s) => s.clone(),
                _ => arg_str(&it, &["content", "task", "title", "text"]).unwrap_or_default(),
            };
            if content.trim().is_empty() {
                continue;
            }
            let status = arg_str(&it, &["status", "state"])
                .unwrap_or_else(|| "pending".into())
                .to_lowercase();
            let status = match status.as_str() {
                "done" | "complete" | "completed" | "finished" => "completed",
                "in_progress" | "in-progress" | "active" | "doing" | "started" => "in_progress",
                "cancelled" | "canceled" | "skipped" => "cancelled",
                _ => "pending",
            };
            next.push(TodoItem {
                content: content.trim().to_string(),
                status: status.into(),
            });
        }
        *todos = next;
    }
    Ok(render_todos(&todos))
}

pub fn render_todos(todos: &[TodoItem]) -> String {
    if todos.is_empty() {
        return "Task list is empty.".into();
    }
    let done = todos.iter().filter(|t| t.status == "completed").count();
    let mut s = format!("Task plan ({done}/{} done):\n", todos.len());
    for (i, t) in todos.iter().enumerate() {
        let mark = match t.status.as_str() {
            "completed" => "[x]",
            "in_progress" => "[>]",
            "cancelled" => "[-]",
            _ => "[ ]",
        };
        s.push_str(&format!("{mark} {}. {}\n", i + 1, t.content));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_repairs_common_defects() {
        assert_eq!(parse_args("").unwrap(), json!({}));
        assert_eq!(parse_args(r#"{"a":1,}"#).unwrap(), json!({"a":1}));
        assert_eq!(
            parse_args("```json\n{\"a\": 2}\n```").unwrap(),
            json!({"a":2})
        );
        assert_eq!(parse_args(r#""{\"a\":3}""#).unwrap(), json!({"a":3}));
        assert_eq!(
            parse_args(r#"{"path": "x.rs", "content": "fn main() {"#).unwrap()["path"],
            "x.rs"
        );
        assert_eq!(parse_args("{'a': 'b'}").unwrap(), json!({"a":"b"}));
        assert!(parse_args("not json at all").is_err());
    }

    #[test]
    fn arg_aliases() {
        let v = json!({"file_path": "a.txt", "limit": "5", "replace_all": "true"});
        assert_eq!(arg_str(&v, PATH_KEYS).unwrap(), "a.txt");
        assert_eq!(arg_u64(&v, &["limit"]), Some(5));
        assert!(arg_bool(&v, &["replace_all"]));
    }

    #[test]
    fn classes() {
        assert_eq!(class_of("write_file"), ToolClass::Mutating);
        assert_eq!(class_of("execute_command"), ToolClass::Mutating);
        assert_eq!(class_of("read_file"), ToolClass::ReadOnly);
        assert_eq!(class_of("web_fetch"), ToolClass::ReadOnly);
    }

    #[test]
    fn specs_are_valid() {
        let s = specs(&SpecOptions {
            allow_web: true,
            allow_delegate: true,
            save_sessions: true,
        });
        let mut names: Vec<_> = s.iter().map(|t| t.name).collect();
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(n, names.len(), "tool names unique");
        assert!(s.iter().all(|t| t.parameters["type"] == "object"));
    }
}

#[cfg(test)]
pub mod tests_support {
    use super::*;

    /// Owns everything a `ToolCtx` borrows, rooted in a temp directory.
    pub struct Harness {
        pub dir: tempfile::TempDir,
        pub security: SecurityGuard,
        pub memory: ObsidianMemory,
        pub skills: Mutex<SkillManager>,
        pub todos: Mutex<Vec<TodoItem>>,
        pub checkpoints: Checkpoints,
        pub http: reqwest::Client,
        pub interrupt: Interrupt,
    }

    impl Harness {
        pub fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let vault = dir.path().join("vault");
            let memory = ObsidianMemory::new(&vault);
            let skills = Mutex::new(SkillManager::new(&memory.skills_dir()));
            let checkpoints = Checkpoints::new(dir.path().join("checkpoints"));
            Self {
                security: SecurityGuard::new(&[]),
                memory,
                skills,
                todos: Mutex::new(Vec::new()),
                checkpoints,
                http: reqwest::Client::new(),
                interrupt: Interrupt::new(),
                dir,
            }
        }

        pub fn ctx(&self) -> ToolCtx<'_> {
            ToolCtx {
                security: &self.security,
                memory: &self.memory,
                skills: &self.skills,
                todos: &self.todos,
                checkpoints: &self.checkpoints,
                http: &self.http,
                sessions_dir: self.dir.path().join("sessions"),
                turn: 1,
                command_timeout_secs: 30,
                max_output_chars: 12_000,
                interrupt: &self.interrupt,
            }
        }
    }

    #[tokio::test]
    async fn dispatch_memory_todo_and_skills() {
        let h = Harness::new();
        let ctx = h.ctx();
        execute(
            "obsidian_append",
            &json!({"memory": "User's shell is zsh"}),
            &ctx,
        )
        .await
        .unwrap();
        assert!(
            execute("memory_search", &json!({"query": "which shell zsh"}), &ctx)
                .await
                .unwrap()
                .contains("zsh")
        );
        let t = execute("todo", &json!({"todos": [{"content": "a", "status": "done"}, {"content": "b", "status": "in_progress"}]}), &ctx).await.unwrap();
        assert!(t.contains("1/2 done") && t.contains("[>] 2. b"));
        assert!(
            execute("skill_read", &json!({"name": "git-workflow"}), &ctx)
                .await
                .unwrap()
                .contains("Skill: git-workflow")
        );
        let audit = execute("skill_read", &json!({"name": "qsl-security-audit"}), &ctx)
            .await
            .unwrap();
        assert!(audit.contains("part 1/") && audit.contains("HUNTING.md"));
        let page2 = execute(
            "skill_read",
            &json!({"name": "qsl-security-audit", "part": "2"}),
            &ctx,
        )
        .await
        .unwrap();
        assert!(page2.contains("part 2/"));
        assert!(execute(
            "skill_read",
            &json!({"name": "qsl-security-audit", "file": "RECONNAISSANCE.md"}),
            &ctx
        )
        .await
        .unwrap()
        .contains("RECONNAISSANCE.md"));
        assert!(execute(
            "skill_read",
            &json!({"name": "qsl-security-audit", "file": "../../../etc/passwd"}),
            &ctx
        )
        .await
        .is_err());
        assert!(
            execute("skill_read", &json!({"name": "no-such-skill-xyz"}), &ctx)
                .await
                .is_err()
        );
        assert!(execute(
            "obsidian_write",
            &json!({"note_path": "../x", "content": "y"}),
            &ctx
        )
        .await
        .is_err());
        assert!(execute("bogus", &json!({}), &ctx)
            .await
            .unwrap_err()
            .contains("unknown tool"));
    }
}
