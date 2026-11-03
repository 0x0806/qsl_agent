//! Small, dependency-free helpers shared across modules.
//!
//! Every string slicing helper here is UTF-8 safe: the old builds sliced with
//! raw byte indices (`&s[..15_000]`) which panics on multi-byte characters and,
//! with `panic = "abort"`, killed the whole process.

use std::path::{Component, Path, PathBuf};

/// Largest byte index `<= idx` that lies on a char boundary.
pub fn floor_char_boundary(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    let mut i = idx;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Smallest byte index `>= idx` that lies on a char boundary.
pub fn ceil_char_boundary(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    let mut i = idx;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Truncate to at most `max_chars` characters, appending an ellipsis marker.
pub fn truncate_chars(s: &str, max_chars: usize) -> String {
    match s.char_indices().nth(max_chars) {
        None => s.to_string(),
        Some((byte_idx, _)) => format!("{}...", &s[..byte_idx]),
    }
}

/// Keep the head and tail of a long text, eliding the middle. Used for tool
/// output so the model sees both the start and the (usually important) end,
/// e.g. compiler errors or test summaries.
pub fn smart_clip(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    if max_bytes < 256 {
        let end = floor_char_boundary(s, max_bytes);
        return format!("{}...", &s[..end]);
    }
    let head = floor_char_boundary(s, max_bytes * 6 / 10);
    let tail_len = max_bytes.saturating_sub(head + 80);
    let tail_start = ceil_char_boundary(s, s.len().saturating_sub(tail_len));
    let elided_lines = s[head..tail_start].lines().count();
    format!(
        "{}\n\n[... {} bytes / ~{} lines elided ...]\n\n{}",
        &s[..head],
        tail_start - head,
        elided_lines,
        &s[tail_start..]
    )
}

/// Compact human formatting for token counts: 950, 4.1k, 1.2M.
pub fn fmt_tokens(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Expand a leading `~` to the user's home directory.
pub fn expand_tilde(p: &str) -> PathBuf {
    if p == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(p));
    }
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(p)
}

/// Lexically normalise a path (resolve `.` and `..`) without touching the
/// filesystem. Relative paths are resolved against the current directory.
pub fn normalize_path(p: &Path) -> PathBuf {
    let base = if p.is_absolute() {
        PathBuf::new()
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
    };
    let mut out = PathBuf::new();
    for comp in base.components().chain(p.components()) {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolve symlinks for the longest existing ancestor of `p`, then re-attach
/// the non-existent remainder. This defeats `ln -s /etc x; write x/passwd`
/// style escapes while still allowing paths that do not exist yet.
pub fn resolve_path(p: &Path) -> PathBuf {
    let normalized = normalize_path(p);
    let mut existing = normalized.clone();
    let mut remainder: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(canon) = existing.canonicalize() {
            let mut out = canon;
            for part in remainder.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (
            existing.file_name().map(|f| f.to_os_string()),
            existing.parent(),
        ) {
            (Some(name), Some(parent)) => {
                remainder.push(name);
                existing = parent.to_path_buf();
            }
            _ => return normalized,
        }
    }
}

/// Short random identifier for tool calls / sessions.
pub fn short_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..12].to_string()
}

/// Current terminal width in columns (falls back to `$COLUMNS`, then 100).
pub fn terminal_width() -> usize {
    #[cfg(unix)]
    {
        // SAFETY: TIOCGWINSZ only writes into the provided, properly sized struct.
        unsafe {
            let mut ws: libc::winsize = std::mem::zeroed();
            if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 {
                return ws.ws_col as usize;
            }
        }
    }
    std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.parse().ok())
        .unwrap_or(100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_is_utf8_safe() {
        let s = "é".repeat(5000);
        let out = smart_clip(&s, 1000);
        assert!(out.len() < 1200);
        assert!(out.contains("elided"));
        let t = truncate_chars("héllo wörld", 4);
        assert_eq!(t, "héll...");
    }

    #[test]
    fn clip_keeps_head_and_tail() {
        let s: String = (0..2000).map(|i| format!("line {i}\n")).collect();
        let out = smart_clip(&s, 2000);
        assert!(out.starts_with("line 0"));
        assert!(out.trim_end().ends_with("line 1999"));
    }

    #[test]
    fn normalizes_parent_components() {
        let p = normalize_path(Path::new("/etc/../etc/./shadow"));
        assert_eq!(p, PathBuf::from("/etc/shadow"));
    }

    #[test]
    fn resolves_nonexistent_tail() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a/b/c.txt");
        let r = resolve_path(&p);
        assert!(r.ends_with("a/b/c.txt"));
        assert!(r.starts_with(dir.path().canonicalize().unwrap()));
    }

    #[test]
    fn token_formatting() {
        assert_eq!(fmt_tokens(950), "950");
        assert_eq!(fmt_tokens(4100), "4.1k");
        assert_eq!(fmt_tokens(1_200_000), "1.2M");
    }
}
