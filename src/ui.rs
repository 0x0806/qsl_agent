//! Terminal UI: the v0.2 box-drawing look (no emojis), extended with live
//! streaming, a spinner, status line and width-aware frames.

use colored::*;
use serde_json::Value;
use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use unicode_width::UnicodeWidthChar;

use crate::util::{terminal_width, truncate_chars};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

static QUIET: AtomicBool = AtomicBool::new(false);
/// Serialises writes from the spinner task and the main flow.
static OUT: Mutex<()> = Mutex::new(());

pub fn set_quiet(q: bool) {
    QUIET.store(q, Ordering::Relaxed);
}
pub fn quiet() -> bool {
    QUIET.load(Ordering::Relaxed)
}

/// Decide colour once: `--no-color`, `NO_COLOR`, or a non-TTY stdout disable it.
pub fn init_colors(force_off: bool) {
    let off =
        force_off || std::env::var_os("NO_COLOR").is_some() || !std::io::stdout().is_terminal();
    colored::control::set_override(!off);
}

fn width() -> usize {
    terminal_width().clamp(40, 110)
}

macro_rules! out {
    ($($arg:tt)*) => {{
        if !quiet() {
            let _g = OUT.lock().unwrap_or_else(|e| e.into_inner());
            println!($($arg)*);
        }
    }};
}

fn header(title: &str) -> String {
    let w = width();
    let lead = format!("╭── [{title}] ");
    let used = lead.chars().count();
    format!("{lead}{}", "─".repeat(w.saturating_sub(used)))
}

fn footer() -> String {
    format!("╰{}", "─".repeat(width().saturating_sub(1)))
}

fn divider() -> String {
    format!("├{}", "─".repeat(width().saturating_sub(1)))
}

/// Wrap `line` to `max` display columns.
fn wrap(line: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut col = 0;
    for word in line.split_inclusive(' ') {
        let ww: usize = word.chars().map(|c| c.width().unwrap_or(0)).sum();
        // A trailing space may hang past the margin; only the visible part must fit.
        let visible = ww - usize::from(word.ends_with(' '));
        if col + visible > max && col > 0 {
            out.push(cur.trim_end().to_string());
            cur.clear();
            col = 0;
        }
        if ww > max {
            for c in word.chars() {
                let cw = c.width().unwrap_or(0);
                if col + cw > max {
                    out.push(std::mem::take(&mut cur));
                    col = 0;
                }
                cur.push(c);
                col += cw;
            }
        } else {
            cur.push_str(word);
            col += ww;
        }
    }
    out.push(cur.trim_end().to_string());
    out
}

fn framed_lines(text: &str, style: impl Fn(&str) -> ColoredString) -> Vec<String> {
    let max = width().saturating_sub(4);
    let mut lines = Vec::new();
    for raw in text.lines() {
        for l in wrap(raw, max) {
            lines.push(format!("│  {}", style(&l)));
        }
    }
    lines
}

// ---------------------------------------------------------------------------
// Banner
// ---------------------------------------------------------------------------

pub struct BannerInfo<'a> {
    pub provider: &'a str,
    pub endpoint: &'a str,
    pub model: &'a str,
    pub context: u32,
    pub approval: &'a str,
    pub skills: usize,
    pub tools: usize,
    pub vault: &'a str,
    pub session: &'a str,
    pub resumed: usize,
}

pub fn banner(b: &BannerInfo<'_>) {
    if quiet() {
        return;
    }
    let inner = width().saturating_sub(2).min(78);
    let sep = "─".repeat(inner);
    let row = |raw: &str, colored_text: String| {
        let pad = inner.saturating_sub(2).saturating_sub(raw.chars().count());
        out!(
            "{} {}{} {}",
            "│".bright_cyan(),
            colored_text,
            " ".repeat(pad),
            "│".bright_cyan()
        );
    };
    let kv = |k: &str, v: &str, paint: fn(&str) -> ColoredString| {
        let v = truncate_chars(v, inner.saturating_sub(14));
        row(
            &format!("{k:<10} {v}"),
            format!("{} {}", format!("{k:<10}").bold().white(), paint(&v)),
        );
    };

    out!();
    out!("{}", format!("╭{sep}╮").bright_cyan());
    let title = format!("QSL AGENT — AUTONOMOUS LOCAL INTELLIGENCE (v{VERSION})");
    row(&title, title.bold().bright_green().to_string());
    let badges = "[100% LOCAL]  [ZERO CLOUD]  [OBSIDIAN CORTEX]  [SECURITY GUARD]";
    row(
        badges,
        format!(
            "{}  {}  {}  {}",
            "[100% LOCAL]".green().bold(),
            "[ZERO CLOUD]".blue().bold(),
            "[OBSIDIAN CORTEX]".magenta().bold(),
            "[SECURITY GUARD]".red().bold()
        ),
    );
    out!("{}", format!("├{sep}┤").bright_cyan());
    kv(
        "Engine:",
        &format!("{} ({})", b.provider, b.endpoint),
        |s| s.cyan().bold(),
    );
    let model = if b.model.is_empty() {
        "<none detected — use /models>"
    } else {
        b.model
    };
    kv("Model:", model, |s| s.yellow().bold());
    kv(
        "Tier:",
        crate::provider::ModelTier::from_model_name(model).label(),
        |s| s.bright_cyan(),
    );
    kv("Context:", &format!("{} tokens", b.context), |s| {
        s.bright_cyan()
    });
    kv("Approval:", b.approval, |s| s.green().bold());
    kv(
        "Skills:",
        &format!("{} skills | {} tools", b.skills, b.tools),
        |s| s.magenta().bold(),
    );
    kv("Vault:", b.vault, |s| s.bright_black());
    let sess = if b.resumed > 0 {
        format!("{} (resumed, {} messages)", b.session, b.resumed)
    } else {
        b.session.to_string()
    };
    kv("Session:", &sess, |s| s.bright_black());
    kv(
        "Org:",
        "Quantum Secular Labs (https://quantumseclabs.com/)",
        |s| s.cyan(),
    );
    out!("{}", format!("├{sep}┤").bright_cyan());
    let tip = "Tip: /help for commands | Ctrl+C stops a turn | \\ or \"\"\" for multi-line";
    row(tip, format!("{} {}", "Tip:".yellow().bold(), &tip[5..]));
    out!("{}", format!("╰{sep}╯").bright_cyan());
    out!();
}

// ---------------------------------------------------------------------------
// Tool rendering
// ---------------------------------------------------------------------------

/// Structured key/value badges for tool arguments instead of raw JSON.
pub fn format_tool_details(name: &str, args: &Value) -> Vec<(String, String)> {
    let s = |keys: &[&str]| crate::tools::arg_str(args, keys);
    let mut d: Vec<(String, String)> = Vec::new();
    let mut push = |k: &str, v: String| d.push((k.to_string(), v));
    match name {
        "execute_command" => {
            if let Some(c) = s(&["command", "cmd"]) {
                push("Command", c);
            }
            if let Some(w) = s(&["workdir", "cwd"]) {
                push("Workdir", w);
            }
        }
        "write_file" | "append_file" => {
            if let Some(p) = s(crate::tools::PATH_KEYS) {
                push("Target", p);
            }
            if let Some(c) = s(&["content", "text", "data"]) {
                push(
                    if name == "append_file" {
                        "Appended"
                    } else {
                        "Payload"
                    },
                    format!("{} bytes ({} lines)", c.len(), c.lines().count()),
                );
            }
        }
        "read_file" => {
            if let Some(p) = s(crate::tools::PATH_KEYS) {
                push("Target", p);
            }
            let a = crate::tools::arg_u64(args, &["start_line", "offset"]);
            let b = crate::tools::arg_u64(args, &["end_line"]);
            if a.is_some() || b.is_some() {
                push(
                    "Range",
                    format!(
                        "lines {} to {}",
                        a.unwrap_or(1),
                        b.map(|x| x.to_string()).unwrap_or_else(|| "end".into())
                    ),
                );
            }
        }
        "edit_file" => {
            if let Some(p) = s(crate::tools::PATH_KEYS) {
                push("Target", p);
            }
            let t = s(&["target", "old_string", "old_str"])
                .map(|x| x.len())
                .unwrap_or(0);
            let r = s(&["replacement", "new_string", "new_str"])
                .map(|x| x.len())
                .unwrap_or(0);
            push("Diff", format!("replace {t} bytes with {r} bytes"));
        }
        "obsidian_write" | "obsidian_read" => {
            if let Some(p) = s(&["note_path", "path"]) {
                push("Note", p);
            }
            if let Some(c) = s(&["content"]) {
                push("Size", format!("{} bytes", c.len()));
            }
        }
        "obsidian_append" => {
            if let Some(m) = s(&["memory", "content", "text"]) {
                push("Memory", truncate_chars(&m, 90));
            }
        }
        "delegate_task" => {
            if let Some(t) = s(&["task"]) {
                push("Task", truncate_chars(&t, 120));
            }
        }
        "todo" => {
            let n = args
                .get("todos")
                .and_then(|t| t.as_array())
                .map(|a| a.len());
            push(
                "Plan",
                n.map(|n| format!("{n} item(s)"))
                    .unwrap_or_else(|| "read".into()),
            );
        }
        _ => {}
    }
    if d.is_empty() {
        if let Some(obj) = args.as_object() {
            for (k, v) in obj {
                let val = match v {
                    Value::String(s) => truncate_chars(s, 80),
                    other => truncate_chars(&other.to_string(), 80),
                };
                d.push((k.clone(), val));
            }
        }
    }
    d
}

pub fn render_tool_call(name: &str, args: &Value, note: Option<&str>) {
    if quiet() {
        return;
    }
    let mut lines = vec![header(&format!("ACTION: {name}")).bold().cyan().to_string()];
    for (k, v) in format_tool_details(name, args) {
        let first = v.lines().next().unwrap_or("").to_string();
        let more = v.lines().count().saturating_sub(1);
        let v = if more > 0 {
            format!("{first} (+{more} lines)")
        } else {
            first
        };
        lines.push(format!(
            "│  {:<12} {}",
            format!("{k}:").bold().white(),
            truncate_chars(&v, width().saturating_sub(18)).dimmed()
        ));
    }
    if let Some(n) = note {
        lines.push(format!("│  {}", n.yellow()));
    }
    lines.push(footer().bold().cyan().to_string());
    out!("{}", lines.join("\n"));
}

pub fn render_tool_result(name: &str, output: &str, elapsed_ms: u128) {
    if quiet() {
        return;
    }
    let trimmed = output.trim();
    let warn = trimmed.starts_with("Exit Code: ") && !trimmed.starts_with("Exit Code: 0");
    let total = trimmed.lines().count();
    let title = format!(
        "RESULT: {name}] [{}] [{:.1}s",
        if warn { "WARN" } else { "OK" },
        elapsed_ms as f64 / 1000.0
    );
    let paint = |s: String| {
        if warn {
            s.bold().yellow()
        } else {
            s.bold().green()
        }
    };
    let mut lines = vec![paint(header(&title)).to_string()];
    let max_lines = 6;
    for l in trimmed.lines().take(max_lines) {
        lines.push(format!(
            "│  {}",
            truncate_chars(l, width().saturating_sub(6)).bright_black()
        ));
    }
    if total > max_lines {
        lines.push(format!(
            "│  {}",
            format!(
                "... ({} more lines, {} bytes total)",
                total - max_lines,
                output.len()
            )
            .dimmed()
        ));
    }
    lines.push(paint(footer()).to_string());
    out!("{}", lines.join("\n"));
}

pub fn render_tool_error(name: &str, error: &str) {
    if quiet() {
        return;
    }
    let mut lines = vec![header(&format!("BLOCKED / ERROR: {name}"))
        .bold()
        .red()
        .to_string()];
    lines.extend(framed_lines(&truncate_chars(error.trim(), 1500), |l| {
        l.bright_red()
    }));
    lines.push(footer().bold().red().to_string());
    out!("{}", lines.join("\n"));
}

/// Final answer card (used when streaming is off, and for `-P` output).
pub fn render_final_response(content: &str) {
    let t = content.trim();
    if quiet() || t.is_empty() {
        return;
    }
    let mut lines = vec![
        String::new(),
        header("EXECUTIVE RESPONSE")
            .bold()
            .bright_green()
            .to_string(),
        "│".into(),
    ];
    let mut in_code = false;
    for raw in t.lines() {
        let fence = raw.trim_start().starts_with("```");
        for w in wrap(raw, width().saturating_sub(4)) {
            let styled = style_md_line(&w, in_code || fence);
            lines.push(format!("│  {styled}"));
        }
        if fence {
            in_code = !in_code;
        }
    }
    lines.push("│".into());
    lines.push(footer().bold().bright_green().to_string());
    out!("{}\n", lines.join("\n"));
}

fn style_md_line(line: &str, code: bool) -> ColoredString {
    let t = line.trim_start();
    if code {
        line.green()
    } else if t.starts_with('#') {
        line.bold().cyan()
    } else if t.starts_with("> ") {
        line.italic().dimmed()
    } else {
        line.normal()
    }
}

pub fn render_skills_table(skills: &[(String, String, Vec<String>)]) {
    if quiet() {
        return;
    }
    let w = width().max(80);
    let name_w = 24;
    let tag_w = 26;
    let desc_w = w.saturating_sub(name_w + tag_w + 10).max(20);
    let bar = |l: &str, m: &str, r: &str| {
        format!(
            "{l}{}{m}{}{m}{}{r}",
            "─".repeat(name_w + 2),
            "─".repeat(tag_w + 2),
            "─".repeat(desc_w + 2)
        )
    };
    let pad = |s: &str, n: usize| {
        let t = truncate_chars(s, n.saturating_sub(3).max(1));
        let t = if s.chars().count() <= n {
            s.to_string()
        } else {
            t
        };
        let len = t.chars().count();
        format!("{t}{}", " ".repeat(n.saturating_sub(len)))
    };
    let mut lines = vec![
        String::new(),
        bar("╭", "┬", "╮").bright_magenta().to_string(),
    ];
    lines.push(format!(
        "│ {} │ {} │ {} │",
        pad("Skill Name", name_w).bold().white(),
        pad("Tags", tag_w).bold().white(),
        pad("Description", desc_w).bold().white()
    ));
    lines.push(bar("├", "┼", "┤").bright_magenta().to_string());
    for (name, desc, tags) in skills {
        lines.push(format!(
            "│ {} │ {} │ {} │",
            pad(name, name_w).cyan().bold(),
            pad(&tags.join(", "), tag_w).dimmed(),
            pad(desc, desc_w)
        ));
    }
    lines.push(bar("╰", "┴", "╯").bright_magenta().to_string());
    out!("{}\n", lines.join("\n"));
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

pub fn info(msg: &str) {
    out!("  {} {}", "[OK]".green(), msg);
}
pub fn note(msg: &str) {
    out!("  {}", msg.dimmed());
}
pub fn warn(msg: &str) {
    if quiet() {
        eprintln!("warning: {msg}");
    } else {
        out!("  {} {}", "[WARN]".yellow().bold(), msg);
    }
}
pub fn error(msg: &str) {
    let _g = OUT.lock().unwrap_or_else(|e| e.into_inner());
    eprintln!("\n{} {}\n", "Agent Error:".bold().red(), msg);
}
pub fn plain(msg: &str) {
    out!("{msg}");
}
pub fn section(title: &str) {
    out!("\n{}", title.bold().cyan());
}

pub fn status_line(parts: &[String]) {
    out!("  {}", format!("[{}]", parts.join(" | ")).dimmed());
}

pub fn subagent_banner(task: &str, start: bool) {
    if start {
        out!(
            "{}\n│  {}\n{}",
            header("DELEGATE: sub-agent").bold().blue(),
            truncate_chars(task, 200).dimmed(),
            divider().blue()
        );
    } else {
        out!("{}", footer().bold().blue());
    }
}

// ---------------------------------------------------------------------------
// Prompts
// ---------------------------------------------------------------------------

/// Read one line from the user. Uses rustyline on a TTY (so Ctrl+C maps to
/// `None`), plain stdin otherwise. Returns `None` on EOF / interrupt.
pub fn ask_line(prompt: &str) -> Option<String> {
    let run = || {
        if std::io::stdin().is_terminal() {
            let mut ed = rustyline::DefaultEditor::new().ok()?;
            ed.readline(prompt).ok()
        } else {
            print!("{prompt}");
            let _ = std::io::stdout().flush();
            let mut s = String::new();
            match std::io::stdin().read_line(&mut s) {
                Ok(0) | Err(_) => None,
                Ok(_) => Some(s),
            }
        }
    };
    match tokio::runtime::Handle::try_current() {
        Ok(h) if h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(run)
        }
        _ => run(),
    }
}

pub enum ApprovalChoice {
    Once,
    Session,
    Deny,
    DenySession,
}

pub fn approval_prompt(
    tool: &str,
    args: &Value,
    risk: Option<&str>,
    allow_session: bool,
) -> ApprovalChoice {
    let mut lines = vec![
        String::new(),
        header("PERMISSION REVIEW").yellow().bold().to_string(),
    ];
    lines.push(format!(
        "│  {:<10} {}",
        "Tool:".bold().white(),
        tool.bold().cyan()
    ));
    for (k, v) in format_tool_details(tool, args) {
        for (i, l) in v.lines().take(12).enumerate() {
            let key = if i == 0 {
                format!("{k}:")
            } else {
                String::new()
            };
            lines.push(format!(
                "│  {:<10} {}",
                key.bold().white(),
                truncate_chars(l, width().saturating_sub(16))
            ));
        }
    }
    if let Some(r) = risk {
        lines.push(format!(
            "│  {:<10} {}",
            "Risk:".bold().red(),
            r.red().bold()
        ));
    }
    lines.push(divider().yellow().to_string());
    if allow_session {
        lines.push(format!(
            "│  {} Allow once  │  {} Always allow (session)  │  {} Deny  │  {} Deny (session)",
            "[y]".green().bold(),
            "[a]".cyan().bold(),
            "[n]".red().bold(),
            "[d]".red().bold()
        ));
    } else {
        lines.push(format!(
            "│  {} Allow once  │  {} Deny  │  {} Deny (session)",
            "[y]".green().bold(),
            "[n]".red().bold(),
            "[d]".red().bold()
        ));
    }
    lines.push(footer().yellow().to_string());
    out!("{}", lines.join("\n"));
    loop {
        let Some(ans) = ask_line(&format!(
            "{}",
            "  permission [y/a/n/d] (default n) > ".bold()
        )) else {
            return ApprovalChoice::Deny;
        };
        match ans.trim().to_lowercase().as_str() {
            "y" | "yes" => return ApprovalChoice::Once,
            "a" | "always" if allow_session => return ApprovalChoice::Session,
            "n" | "no" | "" => return ApprovalChoice::Deny,
            "d" => return ApprovalChoice::DenySession,
            _ => out!("  {}", "Please answer y, a, n or d.".yellow()),
        }
    }
}

pub enum BudgetChoice {
    Extend,
    Synthesize,
    Stop,
}

pub fn budget_prompt(used: usize, max: usize) -> BudgetChoice {
    let lines = [
        String::new(),
        header("STEP LIMIT REACHED").bold().yellow().to_string(),
        format!("│  Agent reached its step budget ({used}/{max} steps)."),
        divider().yellow().to_string(),
        format!(
            "│  {} Extend (+15 steps)  │  {} Force synthesis  │  {} Stop turn",
            "[c]".cyan().bold(),
            "[f]".green().bold(),
            "[s]".red().bold()
        ),
        footer().bold().yellow().to_string(),
    ];
    out!("{}", lines.join("\n"));
    match ask_line(&format!("{}", "  action [c/f/s] (default c) > ".bold()))
        .map(|s| s.trim().to_lowercase())
    {
        Some(s) if s == "f" || s.starts_with("syn") || s.starts_with("fin") => {
            BudgetChoice::Synthesize
        }
        Some(s) if s == "s" || s == "stop" => BudgetChoice::Stop,
        None => BudgetChoice::Stop,
        _ => BudgetChoice::Extend,
    }
}

// ---------------------------------------------------------------------------
// Spinner
// ---------------------------------------------------------------------------

struct SpinState {
    active: bool,
    label: String,
}

#[derive(Clone)]
pub struct Spinner {
    state: Arc<Mutex<SpinState>>,
}

impl Spinner {
    /// Starts a ticker on TTYs; on non-TTYs prints the label once.
    pub fn start(label: String) -> Self {
        let tty = std::io::stdout().is_terminal() && !quiet();
        let state = Arc::new(Mutex::new(SpinState {
            active: tty,
            label: label.clone(),
        }));
        if tty {
            let st = state.clone();
            let started = Instant::now();
            tokio::spawn(async move {
                const FRAMES: [&str; 4] = ["|", "/", "-", "\\"];
                let mut i = 0usize;
                loop {
                    {
                        let _g = OUT.lock().unwrap_or_else(|e| e.into_inner());
                        let s = st.lock().unwrap_or_else(|e| e.into_inner());
                        if !s.active {
                            break;
                        }
                        let line = format!(
                            "  {} ({}s) {}",
                            s.label,
                            started.elapsed().as_secs(),
                            FRAMES[i % 4]
                        );
                        print!(
                            "\r\x1b[2K{}",
                            truncate_chars(&line, width().saturating_sub(2)).dimmed()
                        );
                        let _ = std::io::stdout().flush();
                    }
                    i += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                }
            });
        } else if !quiet() {
            out!("  {label}");
        }
        Self { state }
    }

    pub fn set_label(&self, label: String) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        s.label = label;
    }

    pub fn is_active(&self) -> bool {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).active
    }

    /// Stop and erase the spinner line. Safe to call repeatedly.
    pub fn stop(&self) {
        let _g = OUT.lock().unwrap_or_else(|e| e.into_inner());
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if s.active {
            s.active = false;
            print!("\r\x1b[2K");
            let _ = std::io::stdout().flush();
        }
    }
}

// ---------------------------------------------------------------------------
// Live stream rendering
// ---------------------------------------------------------------------------

#[derive(PartialEq, Clone, Copy)]
enum Frame {
    None,
    Reasoning,
    Content,
}

#[derive(PartialEq, Clone, Copy)]
enum LineStyle {
    Pending,
    Normal,
    Heading,
    Code,
    Fence,
    Quote,
}

/// Renders streamed reasoning/content inside frames as tokens arrive, and
/// splits inline `<think>...</think>` blocks out of the content stream.
pub struct StreamPrinter {
    spinner: Spinner,
    show_reasoning: bool,
    frame: Frame,
    col: usize,
    max_col: usize,
    in_think: bool,
    tag_buf: String,
    line_buf: String,
    line_style: LineStyle,
    in_code: bool,
    /// Current word, held back so lines break between words.
    word: String,
    word_w: usize,
    /// Newlines not yet printed (dropped if the frame closes first).
    pending_nl: usize,
    pub printed_content: bool,
    pub content: String,
    pub think_text: String,
}

impl StreamPrinter {
    pub fn new(spinner: Spinner, show_reasoning: bool) -> Self {
        Self {
            spinner,
            show_reasoning,
            frame: Frame::None,
            col: 0,
            max_col: width().saturating_sub(4),
            in_think: false,
            tag_buf: String::new(),
            line_buf: String::new(),
            line_style: LineStyle::Pending,
            in_code: false,
            word: String::new(),
            word_w: 0,
            pending_nl: 0,
            printed_content: false,
            content: String::new(),
            think_text: String::new(),
        }
    }

    fn open(&mut self, f: Frame) {
        if self.frame == f {
            return;
        }
        self.close();
        self.spinner.stop();
        let h = match f {
            Frame::Reasoning => header("REASONING & COGNITIVE PLANNING")
                .bold()
                .bright_magenta(),
            Frame::Content => header("RESPONSE").bold().bright_green(),
            Frame::None => return,
        };
        print!("{h}\n│  ");
        self.frame = f;
        self.col = 0;
        self.pending_nl = 0;
        self.line_style = LineStyle::Pending;
    }

    /// Close the current frame (if any).
    pub fn close(&mut self) {
        if self.frame == Frame::None {
            return;
        }
        if self.frame == Frame::Content {
            self.flush_line_buf();
        }
        self.flush_word();
        self.pending_nl = 0;
        let f = match self.frame {
            Frame::Reasoning => footer().bold().bright_magenta(),
            _ => footer().bold().bright_green(),
        };
        print!("\n{f}\n");
        let _ = std::io::stdout().flush();
        self.frame = Frame::None;
    }

    fn newline(&mut self) {
        // At most one blank line in a row; emitted lazily.
        self.pending_nl = (self.pending_nl + 1).min(2);
        self.col = 0;
    }

    fn emit_pending_newlines(&mut self) {
        for _ in 0..self.pending_nl {
            print!("\n│  ");
        }
        self.pending_nl = 0;
    }

    fn emit_reasoning(&mut self, text: &str) {
        self.think_text.push_str(text);
        if !self.show_reasoning || quiet() {
            if self.spinner.is_active() {
                self.spinner.set_label("Reasoning...".into());
            }
            return;
        }
        // `open` stops the spinner (which takes OUT), so lock only afterwards.
        self.open(Frame::Reasoning);
        let _g = OUT.lock().unwrap_or_else(|e| e.into_inner());
        for c in text.chars() {
            if c == '\n' {
                self.flush_word();
                self.newline();
            } else {
                self.put(c);
            }
        }
        let _ = std::io::stdout().flush();
    }

    fn emit_content(&mut self, text: &str) {
        self.content.push_str(text);
        if quiet() {
            return;
        }
        if !self.printed_content && text.trim().is_empty() {
            return;
        }
        self.printed_content = true;
        self.open(Frame::Content);
        let _g = OUT.lock().unwrap_or_else(|e| e.into_inner());
        for c in text.chars() {
            if self.line_style == LineStyle::Pending {
                if c == '\n' {
                    let buffered = std::mem::take(&mut self.line_buf);
                    self.decide_style(&buffered);
                    self.print_chars(&buffered);
                    self.end_line();
                    continue;
                }
                self.line_buf.push(c);
                if self.line_buf.chars().count() >= 4 {
                    let buffered = std::mem::take(&mut self.line_buf);
                    self.decide_style(&buffered);
                    self.print_chars(&buffered);
                }
                continue;
            }
            if c == '\n' {
                self.end_line();
            } else {
                self.print_chars(&c.to_string());
            }
        }
        let _ = std::io::stdout().flush();
    }

    fn decide_style(&mut self, start: &str) {
        let t = start.trim_start();
        self.line_style = if t.starts_with("```") {
            LineStyle::Fence
        } else if self.in_code {
            LineStyle::Code
        } else if t.starts_with('#') {
            LineStyle::Heading
        } else if t.starts_with("> ") {
            LineStyle::Quote
        } else {
            LineStyle::Normal
        };
    }

    fn end_line(&mut self) {
        self.flush_word();
        if self.line_style == LineStyle::Fence {
            self.in_code = !self.in_code;
        }
        self.line_style = LineStyle::Pending;
        self.newline();
    }

    fn flush_line_buf(&mut self) {
        if !self.line_buf.is_empty() {
            let buffered = std::mem::take(&mut self.line_buf);
            self.decide_style(&buffered);
            self.print_chars(&buffered);
        }
    }

    fn print_chars(&mut self, s: &str) {
        for c in s.chars() {
            self.put(c);
        }
    }

    fn styled(&self, piece: &str) -> ColoredString {
        if self.frame == Frame::Reasoning {
            return piece.bright_black();
        }
        match self.line_style {
            LineStyle::Heading => piece.bold().cyan(),
            LineStyle::Code | LineStyle::Fence => piece.green(),
            LineStyle::Quote => piece.dimmed(),
            _ => piece.normal(),
        }
    }

    /// Emit one non-newline char: spaces flush the pending word, other
    /// chars accumulate so a word is never split across lines.
    fn put(&mut self, c: char) {
        if c == ' ' || c == '\t' {
            self.flush_word();
            if self.col + 1 > self.max_col {
                self.newline();
            } else {
                self.emit_pending_newlines();
                print!("{}", self.styled(" "));
                self.col += 1;
            }
            return;
        }
        self.word.push(c);
        self.word_w += c.width().unwrap_or(0);
        if self.word_w >= self.max_col {
            self.flush_word();
        }
    }

    fn flush_word(&mut self) {
        if self.word.is_empty() {
            return;
        }
        if self.col > 0 && self.col + self.word_w > self.max_col {
            self.newline();
        }
        let word = std::mem::take(&mut self.word);
        self.word_w = 0;
        for c in word.chars() {
            let cw = c.width().unwrap_or(0);
            if self.col + cw > self.max_col {
                self.newline();
            }
            self.emit_pending_newlines();
            print!("{}", self.styled(&c.to_string()));
            self.col += cw;
        }
    }

    pub fn on_reasoning(&mut self, text: &str) {
        self.emit_reasoning(text);
    }

    /// Content may contain `<think>` blocks split across chunks.
    pub fn on_content(&mut self, text: &str) {
        const OPEN: &str = "<think>";
        const CLOSE: &str = "</think>";
        self.tag_buf.push_str(text);
        loop {
            let tag = if self.in_think { CLOSE } else { OPEN };
            if let Some(pos) = self.tag_buf.find(tag) {
                let before: String = self.tag_buf[..pos].to_string();
                self.tag_buf.drain(..pos + tag.len());
                if self.in_think {
                    self.emit_reasoning(&before);
                } else {
                    self.emit_content(&before);
                }
                self.in_think = !self.in_think;
                continue;
            }
            // Keep a possible partial tag at the end for the next chunk.
            let keep = (1..tag.len())
                .rev()
                .find(|&n| self.tag_buf.ends_with(&tag[..n]))
                .unwrap_or(0);
            let split = self.tag_buf.len() - keep;
            let ready: String = self.tag_buf[..split].to_string();
            self.tag_buf.drain(..split);
            if !ready.is_empty() {
                if self.in_think {
                    self.emit_reasoning(&ready);
                } else {
                    self.emit_content(&ready);
                }
            }
            break;
        }
    }

    pub fn on_tool_call(&mut self, name: &str) {
        self.close();
        if self.spinner.is_active() {
            self.spinner
                .set_label(format!("Preparing tool call: {name}..."));
        } else if !quiet() {
            let s = Spinner::start(format!("Preparing tool call: {name}..."));
            self.spinner = s;
        }
    }

    /// Flush buffered partial tags and close frames.
    pub fn finish(&mut self) {
        let rest = std::mem::take(&mut self.tag_buf);
        if !rest.is_empty() {
            if self.in_think {
                self.emit_reasoning(&rest);
            } else {
                self.emit_content(&rest);
            }
        }
        self.spinner.stop();
        self.close();
    }
}

/// Split `<think>...</think>` reasoning from a complete response.
pub fn extract_thought_and_response(text: &str) -> (Option<String>, String) {
    if let Some(end) = text.find("</think>") {
        let (head, tail) = text.split_at(end);
        let thought = head.replace("<think>", "").trim().to_string();
        let rest = tail["</think>".len()..].to_string();
        let (more, clean) = extract_thought_and_response(&rest);
        let combined = match more {
            Some(m) if !thought.is_empty() => Some(format!("{thought}\n\n{m}")),
            Some(m) => Some(m),
            None if !thought.is_empty() => Some(thought),
            None => None,
        };
        return (combined, clean);
    }
    if let Some(start) = text.find("<think>") {
        let thought = text[start + 7..].trim().to_string();
        let clean = text[..start].trim().to_string();
        return ((!thought.is_empty()).then_some(thought), clean);
    }
    (None, text.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_think_blocks() {
        let (t, c) = extract_thought_and_response("<think>\nplan\n</think>\nAnswer.");
        assert_eq!(t.as_deref(), Some("plan"));
        assert_eq!(c, "Answer.");
        let (t, c) = extract_thought_and_response("implicit reasoning</think>Answer");
        assert_eq!(t.as_deref(), Some("implicit reasoning"));
        assert_eq!(c, "Answer");
        let (t, c) = extract_thought_and_response("<think>unfinished");
        assert_eq!(t.as_deref(), Some("unfinished"));
        assert_eq!(c, "");
        let (t, c) = extract_thought_and_response("plain");
        assert!(t.is_none());
        assert_eq!(c, "plain");
    }

    #[test]
    fn stream_splits_think_tags_across_chunks() {
        set_quiet(true);
        let mut p = StreamPrinter::new(
            Spinner {
                state: Arc::new(Mutex::new(SpinState {
                    active: false,
                    label: String::new(),
                })),
            },
            true,
        );
        for chunk in ["Hel", "lo <thi", "nk>secret", " plan</th", "ink> world"] {
            p.on_content(chunk);
        }
        p.finish();
        assert_eq!(p.content, "Hello  world");
        assert_eq!(p.think_text, "secret plan");
        set_quiet(false);
    }

    #[test]
    fn tool_details_are_structured() {
        let d = format_tool_details(
            "write_file",
            &serde_json::json!({"path": "src/test.rs", "content": "a\nb\n"}),
        );
        assert_eq!(d[0], ("Target".to_string(), "src/test.rs".to_string()));
        assert!(d[1].1.contains("2 lines"));
        let d = format_tool_details(
            "execute_command",
            &serde_json::json!({"command": "cargo test --lib"}),
        );
        assert_eq!(d[0].1, "cargo test --lib");
        let d = format_tool_details(
            "read_file",
            &serde_json::json!({"path": "Cargo.toml", "start_line": 10, "end_line": 60}),
        );
        assert_eq!(d[1].1, "lines 10 to 60");
    }

    #[test]
    fn wrapping_respects_width() {
        let w = wrap("aaaa bbbb cccc dddd", 9);
        assert_eq!(w, vec!["aaaa bbbb", "cccc dddd"]);
        assert!(wrap(&"x".repeat(25), 10)
            .iter()
            .all(|l| l.chars().count() <= 10));
    }
}
