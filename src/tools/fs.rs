//! File-system tools: read / write / append / edit / list / search / find.

use regex::RegexBuilder;
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

use super::{arg_bool, arg_str, arg_u64, req_str, ToolCtx, PATH_KEYS};
use crate::util::{floor_char_boundary, smart_clip};

const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    ".venv",
    "venv",
    "__pycache__",
    "dist",
    "build",
    ".cache",
    ".next",
    ".idea",
];
const MAX_SEARCH_FILE_BYTES: u64 = 2 * 1024 * 1024;

fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8192).any(|b| *b == 0)
}

pub fn read_file(args: &Value, ctx: &ToolCtx<'_>) -> Result<String, String> {
    let raw = req_str(args, PATH_KEYS)?;
    let path = ctx.security.check_read(&raw)?;
    if path.is_dir() {
        return Err(format!(
            "'{}' is a directory. Use list_dir instead.",
            path.display()
        ));
    }
    let bytes = fs::read(&path).map_err(|e| format!("cannot read '{}': {e}", path.display()))?;
    if is_binary(&bytes) {
        return Ok(format!(
            "'{}' is a binary file ({} bytes); not displayed.",
            path.display(),
            bytes.len()
        ));
    }
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    let total = lines.len();

    let start =
        arg_u64(args, &["start_line", "offset", "start", "line"]).map(|n| n.max(1) as usize);
    let end = arg_u64(args, &["end_line", "end"])
        .map(|n| n as usize)
        .or_else(|| {
            arg_u64(args, &["limit", "lines", "count"]).map(|l| start.unwrap_or(1) + l as usize - 1)
        });

    let budget = ctx.max_output_chars;
    if start.is_none() && end.is_none() && text.len() <= budget {
        return Ok(text.into_owned());
    }
    let from = start.unwrap_or(1).min(total.max(1));
    let to = end.unwrap_or(total).clamp(from, total.max(from));
    let mut out = String::new();
    let mut last = from.saturating_sub(1);
    for (i, line) in lines.iter().enumerate().take(to).skip(from - 1) {
        if out.len() + line.len() + 1 > budget {
            break;
        }
        out.push_str(line);
        out.push('\n');
        last = i + 1;
    }
    let mut header = format!(
        "[{} | lines {}-{} of {}]\n",
        path.display(),
        from,
        last,
        total
    );
    if last < to.min(total) {
        header.push_str(&format!(
            "[output truncated at {budget} chars; continue with start_line={}]\n",
            last + 1
        ));
    }
    Ok(header + &out)
}

/// Atomic write: temp file in the same directory, then rename. Existing file
/// permissions are preserved and the previous content is checkpointed.
fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;
    let perms = fs::metadata(path).ok().map(|m| m.permissions());
    let tmp = dir.join(format!(
        ".{}.qsl-tmp-{}",
        path.file_name()
            .map(|f| f.to_string_lossy())
            .unwrap_or_default(),
        crate::util::short_id()
    ));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    if let Some(p) = perms {
        let _ = fs::set_permissions(&tmp, p);
    }
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

pub fn write_file(args: &Value, ctx: &ToolCtx<'_>, append: bool) -> Result<String, String> {
    let raw = req_str(args, PATH_KEYS)?;
    let path = ctx.security.check_write(&raw)?;
    let content = req_str(args, &["content", "text", "data", "contents", "body"])?;
    if path.is_dir() {
        return Err(format!("'{}' is a directory", path.display()));
    }
    let existed = path.exists();
    ctx.checkpoints.snapshot(&path, ctx.turn);

    if append {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("cannot open '{}': {e}", path.display()))?;
        f.write_all(content.as_bytes()).map_err(|e| e.to_string())?;
        if !content.ends_with('\n') {
            f.write_all(b"\n").map_err(|e| e.to_string())?;
        }
        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        return Ok(format!(
            "Appended {} bytes ({} lines) to '{}' (total size {} bytes).",
            content.len(),
            content.lines().count(),
            path.display(),
            size
        ));
    }

    let old_lines = if existed {
        fs::read_to_string(&path)
            .map(|s| s.lines().count())
            .unwrap_or(0)
    } else {
        0
    };
    let mut body = content.clone();
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    atomic_write(&path, &body).map_err(|e| format!("failed to write '{}': {e}", path.display()))?;
    Ok(format!(
        "{} '{}' ({} bytes, {} lines{}).",
        if existed { "Overwrote" } else { "Created" },
        path.display(),
        body.len(),
        body.lines().count(),
        if existed {
            format!("; previously {old_lines} lines")
        } else {
            String::new()
        }
    ))
}

pub fn edit_file(args: &Value, ctx: &ToolCtx<'_>) -> Result<String, String> {
    let raw = req_str(args, PATH_KEYS)?;
    let path = ctx.security.check_write(&raw)?;
    let target = req_str(
        args,
        &[
            "target",
            "old_string",
            "old_str",
            "old_text",
            "search",
            "find",
            "old",
        ],
    )?;
    let replacement = arg_str(
        args,
        &[
            "replacement",
            "new_string",
            "new_str",
            "new_text",
            "replace",
            "new",
        ],
    )
    .unwrap_or_default();
    let replace_all = arg_bool(args, &["replace_all", "all"]);
    if target.is_empty() {
        return Err("target must not be empty (use write_file to create content)".into());
    }
    let content =
        fs::read_to_string(&path).map_err(|e| format!("cannot read '{}': {e}", path.display()))?;

    let count = content.matches(target.as_str()).count();
    let new_content = if count == 1 || (count > 1 && replace_all) {
        content.replace(target.as_str(), &replacement)
    } else if count > 1 {
        return Err(format!(
            "target occurs {count} times in '{}'. Add surrounding lines to make it unique, or set replace_all=true.",
            path.display()
        ));
    } else {
        match fuzzy_replace(&content, &target, &replacement) {
            Some(c) => c,
            None => {
                let hint = closest_line_hint(&content, &target);
                return Err(format!(
                    "target text not found in '{}'. Re-read the file and copy the exact text (whitespace matters).{hint}",
                    path.display()
                ));
            }
        }
    };
    ctx.checkpoints.snapshot(&path, ctx.turn);
    atomic_write(&path, &new_content)
        .map_err(|e| format!("failed to write '{}': {e}", path.display()))?;

    // Show the edited region so the model can verify without re-reading.
    let pos = new_content.find(replacement.as_str()).unwrap_or(0);
    let line_no = new_content[..pos].lines().count().max(1);
    let snippet: Vec<String> = new_content
        .lines()
        .enumerate()
        .skip(line_no.saturating_sub(3))
        .take(replacement.lines().count() + 5)
        .map(|(i, l)| format!("{:>5}| {l}", i + 1))
        .collect();
    Ok(format!(
        "Edited '{}' ({} replacement{}{}).\n{}",
        path.display(),
        count.max(1),
        if count > 1 { "s" } else { "" },
        if count == 0 {
            ", whitespace-tolerant match"
        } else {
            ""
        },
        snippet.join("\n")
    ))
}

/// Match `target` line-by-line ignoring leading/trailing whitespace; if
/// exactly one region matches, replace it (keeping the original indent).
fn fuzzy_replace(content: &str, target: &str, replacement: &str) -> Option<String> {
    let t_lines: Vec<&str> = target
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if t_lines.is_empty() {
        return None;
    }
    let c_lines: Vec<&str> = content.lines().collect();
    let mut hits = Vec::new();
    'outer: for start in 0..c_lines.len() {
        let mut ci = start;
        for tl in &t_lines {
            while ci < c_lines.len() && c_lines[ci].trim().is_empty() {
                ci += 1;
            }
            if ci >= c_lines.len() || c_lines[ci].trim() != *tl {
                continue 'outer;
            }
            ci += 1;
        }
        hits.push((start, ci));
    }
    if hits.len() != 1 {
        return None;
    }
    let (s, e) = hits[0];
    let indent: String = c_lines[s]
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect();
    let rep_indent: String = replacement
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect();
    let mut out: Vec<String> = c_lines[..s].iter().map(|l| l.to_string()).collect();
    for l in replacement.lines() {
        let stripped = l.strip_prefix(rep_indent.as_str()).unwrap_or(l);
        out.push(if l.trim().is_empty() {
            String::new()
        } else {
            format!("{indent}{stripped}")
        });
    }
    out.extend(c_lines[e..].iter().map(|l| l.to_string()));
    let mut joined = out.join("\n");
    if content.ends_with('\n') {
        joined.push('\n');
    }
    Some(joined)
}

fn closest_line_hint(content: &str, target: &str) -> String {
    let first = target
        .lines()
        .map(str::trim)
        .find(|l| l.len() >= 4)
        .unwrap_or("");
    if first.is_empty() {
        return String::new();
    }
    let key: String = first.chars().take(24).collect();
    content
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains(key.as_str()))
        .map(|(i, l)| {
            format!(
                " Closest line {}: `{}`",
                i + 1,
                crate::util::truncate_chars(l.trim(), 120)
            )
        })
        .unwrap_or_default()
}

pub fn list_dir(args: &Value, ctx: &ToolCtx<'_>) -> Result<String, String> {
    let raw = arg_str(args, &["path", "dir", "directory", "folder"]).unwrap_or_else(|| ".".into());
    let path = ctx.security.check_read(&raw)?;
    if !path.is_dir() {
        return Err(format!("'{}' is not a directory", path.display()));
    }
    let depth = arg_u64(args, &["depth", "max_depth"])
        .unwrap_or(1)
        .clamp(1, 3) as usize;
    let show_hidden = arg_bool(args, &["show_hidden", "all", "hidden"]);
    let mut lines = Vec::new();
    let mut count = 0usize;
    let mut walker = WalkDir::new(&path)
        .min_depth(1)
        .max_depth(depth)
        .sort_by_file_name()
        .into_iter();
    while let Some(entry) = walker.next() {
        let Ok(e) = entry else { continue };
        let name = e.file_name().to_string_lossy().to_string();
        let ft = e.file_type();
        if !show_hidden && name.starts_with('.') {
            if ft.is_dir() {
                walker.skip_current_dir();
            }
            continue;
        }
        // Show heavy dirs (target/, node_modules/) but do not descend.
        if ft.is_dir() && SKIP_DIRS.contains(&name.as_str()) {
            walker.skip_current_dir();
        }
        count += 1;
        if lines.len() >= 500 {
            continue;
        }
        let indent = "  ".repeat(e.depth() - 1);
        let line = if ft.is_symlink() {
            format!("{indent}[LINK] {name}")
        } else if ft.is_dir() {
            format!("{indent}[DIR ] {name}/")
        } else {
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            format!("{indent}[FILE] {name} ({})", human_size(size))
        };
        lines.push(line);
    }
    if lines.is_empty() {
        return Ok(format!("Directory '{}' is empty.", path.display()));
    }
    let more = if count > lines.len() {
        format!("\n... {} more entries", count - lines.len())
    } else {
        String::new()
    };
    Ok(format!("{}:\n{}{more}", path.display(), lines.join("\n")))
}

fn human_size(n: u64) -> String {
    match n {
        n if n >= 1 << 30 => format!("{:.1} GB", n as f64 / (1u64 << 30) as f64),
        n if n >= 1 << 20 => format!("{:.1} MB", n as f64 / (1u64 << 20) as f64),
        n if n >= 1 << 10 => format!("{:.1} KB", n as f64 / 1024.0),
        n => format!("{n} B"),
    }
}

fn walk_files(root: &Path) -> impl Iterator<Item = DirEntry> {
    let root_owned = root.to_path_buf();
    WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(move |e| {
            if e.path() == root_owned {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            !(name.starts_with('.')
                || (e.file_type().is_dir() && SKIP_DIRS.contains(&name.as_ref())))
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
}

/// Convert a glob (`*`, `**`, `?`, `{a,b}`) to an anchored regex.
pub fn glob_to_regex(glob: &str) -> Result<regex::Regex, String> {
    let mut re = String::from("^");
    let chars: Vec<char> = glob.chars().collect();
    let mut i = 0;
    let mut in_brace = false;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '*' if chars.get(i + 1) == Some(&'*') => {
                i += 1;
                if chars.get(i + 1) == Some(&'/') {
                    i += 1;
                    re.push_str("(?:.*/)?");
                } else {
                    re.push_str(".*");
                }
            }
            '*' => re.push_str("[^/]*"),
            '?' => re.push_str("[^/]"),
            '{' => {
                in_brace = true;
                re.push_str("(?:");
            }
            '}' if in_brace => {
                in_brace = false;
                re.push(')');
            }
            ',' if in_brace => re.push('|'),
            c => re.push_str(&regex::escape(&c.to_string())),
        }
        i += 1;
    }
    re.push('$');
    regex::Regex::new(&re).map_err(|e| format!("invalid glob '{glob}': {e}"))
}

fn glob_matches(re: &regex::Regex, glob: &str, rel: &Path) -> bool {
    let rel_s = rel.to_string_lossy();
    if glob.contains('/') {
        re.is_match(&rel_s)
    } else {
        rel.file_name()
            .is_some_and(|f| re.is_match(&f.to_string_lossy()))
    }
}

pub fn search_files(args: &Value, max_output: usize) -> Result<String, String> {
    let pattern = req_str(args, &["pattern", "query", "regex", "text", "search"])?;
    let root = crate::util::expand_tilde(
        &arg_str(args, &["path", "dir", "directory"]).unwrap_or_else(|| ".".into()),
    );
    let max = arg_u64(args, &["max_results", "limit"])
        .unwrap_or(60)
        .clamp(1, 500) as usize;
    let re = RegexBuilder::new(&pattern)
        .case_insensitive(arg_bool(args, &["ignore_case", "case_insensitive", "i"]))
        .size_limit(1 << 22)
        .build()
        .or_else(|_| RegexBuilder::new(&regex::escape(&pattern)).build())
        .map_err(|e| e.to_string())?;
    let glob = arg_str(args, &["glob", "include", "file_pattern"]);
    let glob_re = glob.as_deref().map(glob_to_regex).transpose()?;

    let files: Box<dyn Iterator<Item = PathBuf>> = if root.is_file() {
        Box::new(std::iter::once(root.clone()))
    } else if root.is_dir() {
        Box::new(walk_files(&root).map(|e| e.into_path()))
    } else {
        return Err(format!("path '{}' does not exist", root.display()));
    };

    let mut out = Vec::new();
    let mut files_hit = 0;
    let mut truncated = false;
    for file in files {
        if let (Some(g), Some(gs)) = (&glob_re, &glob) {
            if !glob_matches(g, gs, file.strip_prefix(&root).unwrap_or(&file)) {
                continue;
            }
        }
        if fs::metadata(&file)
            .map(|m| m.len() > MAX_SEARCH_FILE_BYTES)
            .unwrap_or(true)
        {
            continue;
        }
        let Ok(bytes) = fs::read(&file) else { continue };
        if is_binary(&bytes) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        let mut hit = false;
        for (i, line) in text.lines().enumerate() {
            if re.is_match(line) {
                hit = true;
                let end = floor_char_boundary(line, 200);
                out.push(format!(
                    "{}:{}: {}",
                    file.display(),
                    i + 1,
                    line[..end].trim_end()
                ));
                if out.len() >= max {
                    truncated = true;
                    break;
                }
            }
        }
        if hit {
            files_hit += 1;
        }
        if truncated {
            break;
        }
    }
    if out.is_empty() {
        return Ok(format!(
            "No matches for /{pattern}/ under '{}'.",
            root.display()
        ));
    }
    let header = format!(
        "{} match(es) in {} file(s){}:\n",
        out.len(),
        files_hit,
        if truncated {
            " (limit reached; refine the pattern)"
        } else {
            ""
        }
    );
    Ok(smart_clip(&(header + &out.join("\n")), max_output))
}

pub fn find_files(args: &Value) -> Result<String, String> {
    let pattern = req_str(args, &["pattern", "glob", "name", "query"])?;
    let root = crate::util::expand_tilde(
        &arg_str(args, &["path", "dir", "directory"]).unwrap_or_else(|| ".".into()),
    );
    if !root.is_dir() {
        return Err(format!("'{}' is not a directory", root.display()));
    }
    let re = glob_to_regex(&pattern)?;
    let mut found = Vec::new();
    let mut total = 0;
    for e in walk_files(&root) {
        let rel = e.path().strip_prefix(&root).unwrap_or(e.path());
        if glob_matches(&re, &pattern, rel) {
            total += 1;
            if found.len() < 300 {
                found.push(e.path().display().to_string());
            }
        }
    }
    if found.is_empty() {
        return Ok(format!(
            "No files matching '{pattern}' under '{}'.",
            root.display()
        ));
    }
    found.sort();
    Ok(format!(
        "{total} file(s){}:\n{}",
        if total > found.len() {
            " (first 300)"
        } else {
            ""
        },
        found.join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::tests_support::Harness;
    use serde_json::json;

    #[test]
    fn write_read_append_edit_cycle() {
        let h = Harness::new();
        let ctx = h.ctx();
        let p = h.dir.path().join("sub/a.rs");
        let ps = p.to_string_lossy().to_string();
        assert!(write_file(
            &json!({"file_path": ps, "content": "fn main() {\n    println!(\"hi\");\n}"}),
            &ctx,
            false
        )
        .unwrap()
        .contains("Created"));
        assert!(
            write_file(&json!({"path": ps, "content": "// tail"}), &ctx, true)
                .unwrap()
                .contains("Appended")
        );
        let r = read_file(&json!({"path": ps}), &ctx).unwrap();
        assert!(r.ends_with("// tail\n"));
        let r = read_file(&json!({"path": ps, "start_line": 2, "end_line": 2}), &ctx).unwrap();
        assert!(r.contains("lines 2-2 of 4") && r.contains("println"));
        let e = edit_file(&json!({"path": ps, "old_string": "println!(\"hi\");", "new_string": "println!(\"bye\");"}), &ctx).unwrap();
        assert!(e.contains("bye"));
        assert!(edit_file(
            &json!({"path": ps, "target": "nope", "replacement": "x"}),
            &ctx
        )
        .is_err());
        // rollback through checkpoints restores the pre-turn state (file did not exist)
        h.checkpoints.rollback_last().unwrap();
        assert!(!p.exists());
    }

    #[test]
    fn edit_requires_unique_target() {
        let h = Harness::new();
        let ctx = h.ctx();
        let p = h.dir.path().join("dup.txt");
        fs::write(&p, "x = 1\nx = 1\n").unwrap();
        let ps = p.to_string_lossy();
        assert!(edit_file(
            &json!({"path": ps, "target": "x = 1", "replacement": "y"}),
            &ctx
        )
        .unwrap_err()
        .contains("2 times"));
        edit_file(
            &json!({"path": ps, "target": "x = 1", "replacement": "y", "replace_all": true}),
            &ctx,
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "y\ny\n");
    }

    #[test]
    fn fuzzy_whitespace_edit() {
        let content = "fn a() {\n        let x = 1;\n        let y = 2;\n}\n";
        let out = fuzzy_replace(
            content,
            "let x = 1;\nlet y = 2;",
            "let x = 10;\nlet y = 20;",
        )
        .unwrap();
        assert_eq!(
            out,
            "fn a() {\n        let x = 10;\n        let y = 20;\n}\n"
        );
    }

    #[test]
    fn read_handles_binary_and_utf8() {
        let h = Harness::new();
        let ctx = h.ctx();
        let b = h.dir.path().join("bin");
        fs::write(&b, [0u8, 1, 2, 3]).unwrap();
        assert!(read_file(&json!({"path": b.to_string_lossy()}), &ctx)
            .unwrap()
            .contains("binary"));
        let u = h.dir.path().join("u.txt");
        fs::write(&u, "ü".repeat(30_000)).unwrap();
        assert!(read_file(&json!({"path": u.to_string_lossy()}), &ctx).is_ok());
    }

    #[test]
    fn search_and_find() {
        let h = Harness::new();
        let root = h.dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("target")).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn alpha() {}\n// TODO: beta\n",
        )
        .unwrap();
        fs::write(root.join("target/junk.rs"), "TODO in build output\n").unwrap();
        fs::write(root.join("notes.md"), "todo list\n").unwrap();
        let r = search_files(
            &json!({"pattern": "TODO", "path": root.to_string_lossy()}),
            10_000,
        )
        .unwrap();
        assert!(r.contains("lib.rs:2") && !r.contains("junk"));
        let r = search_files(&json!({"pattern": "todo", "path": root.to_string_lossy(), "ignore_case": true, "glob": "*.md"}), 10_000).unwrap();
        assert!(r.contains("notes.md") && !r.contains("lib.rs"));
        let r = find_files(&json!({"pattern": "**/*.rs", "path": root.to_string_lossy()})).unwrap();
        assert!(r.contains("lib.rs") && !r.contains("junk"));
        let r = list_dir(
            &json!({"path": root.to_string_lossy(), "depth": 2}),
            &h.ctx(),
        )
        .unwrap();
        assert!(r.contains("[DIR ] src/") && r.contains("lib.rs"));
    }

    #[test]
    fn glob_conversion() {
        assert!(glob_to_regex("*.{rs,toml}").unwrap().is_match("Cargo.toml"));
        assert!(glob_to_regex("**/test_*.py")
            .unwrap()
            .is_match("a/b/test_x.py"));
        assert!(glob_to_regex("**/test_*.py").unwrap().is_match("test_x.py"));
        assert!(!glob_to_regex("*.rs").unwrap().is_match("src/a.rs"));
    }

    #[test]
    fn writes_into_system_dirs_blocked() {
        let h = Harness::new();
        assert!(write_file(
            &json!({"path": "/etc/qsl-test", "content": "x"}),
            &h.ctx(),
            false
        )
        .unwrap_err()
        .contains("SECURITY"));
    }
}
