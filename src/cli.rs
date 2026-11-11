//! Interactive REPL: banner, line editing (history, tab completion,
//! multi-line input), slash commands and Ctrl+C turn interruption.

use colored::*;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::DefaultHistory;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{Context, Editor, Helper};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::agent::Agent;
use crate::config::{detect_obsidian_vault, list_detected_vaults, ApprovalMode};
use crate::ui;
use crate::util::{expand_tilde, fmt_tokens, truncate_chars};

const COMMANDS: &[(&str, &str)] = &[
    ("/help", "Show this command manual"),
    ("/health", "Run the health & connectivity check"),
    ("/models", "List models on the active engine"),
    ("/model", "<name>  Switch the active model"),
    ("/provider", "<ollama|lmstudio|custom> [url]  Switch engine"),
    (
        "/approval",
        "<ask|always|strict|yolo>  Change approval mode",
    ),
    ("/vault", "[path|auto]  Show or switch the Obsidian vault"),
    ("/memory", "[query]  Show memories (or search them)"),
    ("/remember", "<fact>  Save a fact to long-term memory"),
    ("/forget", "<text>  Delete memories containing text"),
    ("/skills", "List all skills"),
    ("/skill", "<name>  Show a skill's instructions"),
    ("/use", "<name|off>  Pin a skill into the system prompt"),
    ("/todo", "Show the current task plan"),
    ("/sessions", "[rm <id>]  List (or delete) saved sessions"),
    ("/resume", "<id|#>  Resume a saved session"),
    ("/new", "Start a new session (alias /clear)"),
    ("/clear", "Start a new session"),
    ("/title", "<text>  Rename the current session"),
    ("/undo", "[n]  Remove the last n turns"),
    ("/retry", "Re-run the last prompt"),
    ("/rollback", "Undo file changes made in the last turn"),
    ("/checkpoints", "List file checkpoints in this session"),
    ("/compact", "Summarise older context now"),
    ("/export", "[md|json] [path]  Export the session"),
    ("/usage", "Token usage and session statistics"),
    ("/think", "<on|off>  Show or hide model reasoning"),
    ("/stream", "<on|off>  Toggle token streaming"),
    ("/tools", "List available tools"),
    ("/cd", "<dir>  Change the working directory"),
    ("/config", "Show the active configuration"),
    ("/about", "Company, licence and version information"),
    ("/exit", "Exit QSL Agent (alias /quit)"),
    ("/quit", "Exit QSL Agent"),
];

struct ReplHelper {
    skills: Vec<String>,
}

impl Completer for ReplHelper {
    type Candidate = Pair;
    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let head = &line[..pos];
        if !head.starts_with('/') {
            return Ok((pos, vec![]));
        }
        if let Some((cmd, arg)) = head.split_once(' ') {
            let opts: Vec<String> = match cmd {
                "/skill" | "/use" => {
                    let mut v = self.skills.clone();
                    if cmd == "/use" {
                        v.push("off".into());
                    }
                    v
                }
                "/approval" | "/mode" => ["ask", "always", "strict", "yolo"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                "/provider" => crate::frontier::ALL_PROVIDERS
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                "/export" => ["md", "json"].iter().map(|s| s.to_string()).collect(),
                "/think" | "/stream" => ["on", "off"].iter().map(|s| s.to_string()).collect(),
                _ => vec![],
            };
            let start = cmd.len() + 1;
            let pairs = opts
                .into_iter()
                .filter(|o| o.starts_with(arg.trim_start()))
                .map(|o| Pair {
                    display: o.clone(),
                    replacement: o,
                })
                .collect();
            return Ok((start, pairs));
        }
        let pairs = COMMANDS
            .iter()
            .filter(|(c, _)| c.starts_with(head))
            .map(|(c, _)| Pair {
                display: c.to_string(),
                replacement: format!("{c} "),
            })
            .collect();
        Ok((0, pairs))
    }
}

impl Hinter for ReplHelper {
    type Hint = String;
    fn hint(&self, line: &str, pos: usize, _ctx: &Context<'_>) -> Option<String> {
        if pos < line.len() || !line.starts_with('/') || line.contains(' ') || line.len() < 2 {
            return None;
        }
        COMMANDS
            .iter()
            .find(|(c, _)| c.starts_with(line) && *c != line)
            .map(|(c, _)| c[line.len()..].to_string())
    }
}

impl Highlighter for ReplHelper {
    fn highlight_hint<'h>(&self, hint: &'h str) -> std::borrow::Cow<'h, str> {
        std::borrow::Cow::Owned(hint.dimmed().to_string())
    }
}

impl Validator for ReplHelper {
    fn validate(&self, ctx: &mut ValidationContext) -> rustyline::Result<ValidationResult> {
        let input = ctx.input();
        if input.ends_with('\\') || input.matches("\"\"\"").count() % 2 == 1 {
            Ok(ValidationResult::Incomplete)
        } else {
            Ok(ValidationResult::Valid(None))
        }
    }
}

impl Helper for ReplHelper {}

/// Join `\`-continued lines and strip `"""` fences from multi-line input.
pub fn normalize_input(raw: &str) -> String {
    let joined = raw.replace("\\\n", "\n");
    let joined = joined.trim_end_matches('\\');
    joined.replace("\"\"\"", "").trim().to_string()
}

pub async fn run_repl(mut agent: Agent) {
    print_banner(&agent, 0);

    let config = rustyline::Config::builder()
        .auto_add_history(false)
        .completion_type(rustyline::CompletionType::List)
        .build();
    let mut rl: Editor<ReplHelper, DefaultHistory> = match Editor::with_config(config) {
        Ok(e) => e,
        Err(e) => {
            ui::error(&format!("failed to initialise line editor: {e}"));
            return;
        }
    };
    let skill_names = |a: &Agent| {
        a.skills
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .skills
            .iter()
            .map(|s| s.name.clone())
            .collect::<Vec<_>>()
    };
    rl.set_helper(Some(ReplHelper {
        skills: skill_names(&agent),
    }));
    let history_file = agent.config.state_dir().join("repl_history.txt");
    let _ = std::fs::create_dir_all(agent.config.state_dir());
    let _ = rl.load_history(&history_file);

    // Ctrl+C during a turn interrupts it; twice within 2s force-quits.
    let busy = Arc::new(AtomicBool::new(false));
    {
        let busy = busy.clone();
        let intr = agent.interrupt.clone();
        tokio::spawn(async move {
            let mut last: Option<Instant> = None;
            loop {
                if tokio::signal::ctrl_c().await.is_err() {
                    return;
                }
                if !busy.load(Ordering::SeqCst) {
                    continue;
                }
                if last.is_some_and(|t| t.elapsed().as_secs_f32() < 2.0) {
                    eprintln!("\n{}", "Force quit.".red());
                    std::process::exit(130);
                }
                last = Some(Instant::now());
                intr.trigger();
                eprintln!(
                    "\n{}",
                    "  [Interrupting turn... press Ctrl+C again to force quit]".yellow()
                );
            }
        });
    }

    loop {
        let prompt = format!("{} ", crate::brand::accent("qsl-agent >").bold());
        let line = match rl.readline(&prompt) {
            Ok(l) => l,
            Err(ReadlineError::Interrupted) => {
                ui::note("(Interrupted. Type /exit to quit.)");
                continue;
            }
            Err(ReadlineError::Eof) => {
                ui::plain(&"Exiting QSL Agent. Goodbye!".green().to_string());
                break;
            }
            Err(e) => {
                ui::error(&format!("readline error: {e}"));
                break;
            }
        };
        let input = normalize_input(&line);
        if input.is_empty() {
            continue;
        }
        let _ = rl.add_history_entry(line.trim());

        if input.starts_with('/') && !input.starts_with("//") {
            busy.store(true, Ordering::SeqCst);
            let exit = handle_command(&input, &mut agent).await;
            busy.store(false, Ordering::SeqCst);
            if let Some(h) = rl.helper_mut() {
                h.skills = skill_names(&agent);
            }
            if exit {
                break;
            }
            continue;
        }
        let input = input
            .strip_prefix('/')
            .filter(|_| input.starts_with("//"))
            .map(String::from)
            .unwrap_or(input);
        busy.store(true, Ordering::SeqCst);
        run_and_render(&mut agent, &input).await;
        busy.store(false, Ordering::SeqCst);
    }
    agent.save_session();
    let _ = rl.save_history(&history_file);
}

async fn run_and_render(agent: &mut Agent, input: &str) {
    match agent.run_turn(input).await {
        Ok(outcome) => {
            if !agent.config.stream {
                ui::render_final_response(&outcome.text);
            }
            ui::status_line(&agent.status_parts(&outcome));
        }
        Err(e) if agent.interrupt.is_set() => {
            agent.interrupt.reset();
            ui::warn(&format!(
                "Turn interrupted ({e}). Partial results were kept in context."
            ));
        }
        Err(e) => ui::error(&e),
    }
}

fn context_label(agent: &Agent) -> String {
    match &agent.ctx_plan {
        Some(p) => format!("{} tokens ({})", agent.num_ctx, p.reason),
        None => format!("{} tokens", agent.num_ctx),
    }
}

fn print_banner(agent: &Agent, resumed: usize) {
    let endpoint = agent.client.endpoint();
    let context = context_label(agent);
    let vault = agent.memory.vault_path.display().to_string();
    let approval = agent.approval.mode.describe();
    let skills = agent
        .skills
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .skills
        .len();
    let remote_host = agent
        .config
        .is_remote()
        .then(|| agent.config.endpoint_host());
    ui::banner(&ui::BannerInfo {
        provider: &agent.config.provider,
        endpoint: &endpoint,
        model: &agent.config.model,
        context: &context,
        approval,
        skills,
        tools: agent.tool_specs(0).len(),
        vault: &vault,
        remote_host: remote_host.as_deref(),
        session: &agent.meta.id,
        resumed,
    });
    if let Some(w) = agent.context_warning() {
        ui::warn(&format!("Context: {w}"));
    }
}

pub async fn run_health_check(agent: &Agent) -> bool {
    let mut ok = true;
    let w = 72;
    ui::plain(&format!(
        "\n{}",
        format!("╭── [SYSTEM HEALTH & DIAGNOSTIC] {}", "─".repeat(w - 32))
            .bold()
            .bright_green()
    ));
    let row = |label: &str, status: ColoredString, detail: String| {
        ui::plain(&format!("│  {label:<34} {status} {}", detail.dimmed()));
    };

    match agent.client.health_check().await {
        Ok(msg) => row(
            &format!("[1/7] Engine ({})", agent.config.provider),
            "ONLINE [OK]".green().bold(),
            msg,
        ),
        Err(e) => {
            ok = false;
            row(
                &format!("[1/7] Engine ({})", agent.config.provider),
                "OFFLINE [FAIL]".red().bold(),
                truncate_chars(&e.to_string(), 160),
            );
        }
    }
    ui::plain(&format!(
        "│        endpoint: {}",
        agent.client.endpoint().cyan()
    ));

    match agent.client.list_models().await {
        Ok(models) if !models.is_empty() => {
            let active = models.iter().any(|m| m == &agent.config.model);
            row(
                "[2/7] Models",
                if active {
                    "DETECTED [OK]".green().bold()
                } else {
                    "ACTIVE MODEL MISSING [WARN]".yellow().bold()
                },
                format!("{} available", models.len()),
            );
            if !active {
                ok = false;
            }
            for m in models.iter().take(15) {
                let tag = if *m == agent.config.model {
                    "active".green().to_string()
                } else {
                    "available".dimmed().to_string()
                };
                ui::plain(&format!("│        -> {} [{tag}]", m.yellow()));
            }
        }
        Ok(_) => {
            ok = false;
            row(
                "[2/7] Models",
                "EMPTY [FAIL]".yellow().bold(),
                "pull a model, e.g. `ollama pull qwen2.5-coder:7b`".into(),
            );
        }
        Err(e) => {
            ok = false;
            row(
                "[2/7] Models",
                "ERROR [FAIL]".red().bold(),
                truncate_chars(&e.to_string(), 120),
            );
        }
    }

    let native = if agent.client.native_tools() {
        "native tool calling"
    } else {
        "text tool protocol (fallback)"
    };
    match agent.context_warning() {
        None => row(
            "[3/7] Context / tools",
            "CONFIGURED [OK]".green().bold(),
            format!("{}, {native}", context_label(agent)),
        ),
        Some(w) => {
            ok = false;
            row(
                "[3/7] Context / tools",
                "TOO SMALL [WARN]".yellow().bold(),
                format!("{}, {native}", context_label(agent)),
            );
            ui::plain(&format!("│        {}", w.yellow()));
        }
    }

    let vault = &agent.memory.vault_path;
    if vault.is_dir() && agent.memory.memories_file().exists() {
        row(
            "[4/7] Obsidian vault",
            "CONNECTED [OK]".green().bold(),
            vault.display().to_string(),
        );
    } else {
        ok = false;
        row(
            "[4/7] Obsidian vault",
            "NOT WRITABLE [FAIL]".red().bold(),
            vault.display().to_string(),
        );
    }

    let n = agent
        .skills
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .skills
        .len();
    row(
        "[5/7] Skills",
        "LOADED [OK]".green().bold(),
        format!("{n} skills"),
    );

    let state = agent.config.state_dir();
    let probe = state.join(".write-test");
    let writable = std::fs::create_dir_all(&state)
        .and_then(|_| std::fs::write(&probe, b"ok"))
        .is_ok();
    let _ = std::fs::remove_file(&probe);
    if writable {
        row(
            "[6/7] State directory",
            "WRITABLE [OK]".green().bold(),
            state.display().to_string(),
        );
    } else {
        ok = false;
        row(
            "[6/7] State directory",
            "NOT WRITABLE [FAIL]".red().bold(),
            state.display().to_string(),
        );
    }

    let have = |bin: &str| {
        std::env::var_os("PATH")
            .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
    };
    let bins: Vec<String> = ["bash", "git", "rg", "curl"]
        .iter()
        .map(|b| format!("{b}:{}", if have(b) { "yes" } else { "no" }))
        .collect();
    row(
        "[7/7] Host tools",
        if have("bash") || have("sh") {
            "AVAILABLE [OK]".green().bold()
        } else {
            "NO SHELL [FAIL]".red().bold()
        },
        bins.join(" "),
    );

    ui::plain(&format!(
        "{}",
        format!("╰{}", "─".repeat(w)).bold().bright_green()
    ));
    ok
}

/// Handle a slash command. Returns true when the REPL should exit.
async fn handle_command(input: &str, agent: &mut Agent) -> bool {
    let (cmd, arg) = match input.split_once(char::is_whitespace) {
        Some((c, a)) => (c.to_lowercase(), a.trim().to_string()),
        None => (input.to_lowercase(), String::new()),
    };
    match cmd.as_str() {
        "/exit" | "/quit" | "/q" => {
            ui::plain(
                &"Exiting QSL Agent. Have a productive day!"
                    .green()
                    .to_string(),
            );
            return true;
        }
        "/about" => ui::about(),
        "/help" | "/?" => {
            ui::plain(&format!(
                "\n{} {}",
                crate::brand::ink("QSL Agent"),
                crate::brand::accent(&format!("v{}", ui::VERSION))
            ));
            ui::plain(&format!(
                "{}\n",
                format!("{} by {}", crate::brand::TAGLINE, crate::brand::COMPANY).dimmed()
            ));
            ui::plain(&crate::brand::accent("COMMANDS").bold().to_string());
            for (c, d) in COMMANDS
                .iter()
                .filter(|(c, _)| !matches!(*c, "/quit" | "/clear"))
            {
                let (args, desc) = match d.split_once("  ") {
                    Some((a, rest)) if a.starts_with('<') || a.starts_with('[') => (a, rest),
                    _ => ("", *d),
                };
                ui::plain(&format!(
                    "  {:<40} {}",
                    format!("{} {}", c.yellow().bold(), args.dimmed()),
                    desc
                ));
            }
            ui::plain(&format!(
                "\n{}\n  {}\n  {}\n  {}\n",
                "INPUT".bold().cyan(),
                "End a line with \\ or wrap text in \"\"\" for multi-line prompts. Start with // to send a literal '/'.",
                "Ctrl+C interrupts the running turn (twice = force quit). Tab completes commands.",
                "Approvals: [y] once, [a] session, [n] deny, [d] deny for session. Enter = deny."
            ));
        }
        "/health" => {
            run_health_check(agent).await;
        }
        "/models" => match agent.client.list_models().await {
            Ok(models) if models.is_empty() => {
                ui::warn("No models are loaded on the active engine.")
            }
            Ok(models) => {
                ui::section("Detected Local Models:");
                for m in &models {
                    if *m == agent.config.model {
                        ui::plain(&format!("  {} {} (active)", "●".green().bold(), m.bold()));
                    } else {
                        ui::plain(&format!("  {} {}", "○".dimmed(), m));
                    }
                }
            }
            Err(e) => ui::error(&format!("failed to list models: {e}")),
        },
        "/model" => {
            if arg.is_empty() {
                ui::plain(&format!(
                    "Current model: {}  (context {} tokens)\nUsage: /model <name>",
                    agent.config.model.yellow(),
                    agent.num_ctx
                ));
            } else {
                let models = agent.client.list_models().await.unwrap_or_default();
                let chosen = if models.is_empty() || models.contains(&arg) {
                    arg.clone()
                } else if let Some(m) = models.iter().find(|m| m.starts_with(&arg)) {
                    m.clone()
                } else {
                    ui::warn(&format!(
                        "'{arg}' is not in the engine's model list; using it anyway."
                    ));
                    arg.clone()
                };
                agent.set_model(&chosen).await;
                ui::info(&format!(
                    "Switched model to '{}' (context {} tokens). Conversation kept.",
                    chosen.bold(),
                    agent.num_ctx
                ));
                if let Some(w) = agent.context_warning() {
                    ui::warn(&format!("Context: {w}"));
                }
            }
        }
        "/provider" => {
            let mut parts = arg.split_whitespace();
            match parts.next().map(str::to_lowercase) {
                Some(p) if crate::frontier::ALL_PROVIDERS.contains(&p.as_str()) => {
                    switch_provider(agent, &p, parts.next()).await;
                }
                Some(other) => ui::warn(&format!(
                    "Unknown engine '{other}'. Supported: {}",
                    crate::frontier::ALL_PROVIDERS.join(", ")
                )),
                None => show_providers(agent),
            }
        }
        "/approval" | "/mode" => match ApprovalMode::parse(&arg) {
            Some(mode) => {
                agent.approval.set_mode(mode);
                agent.config.set_approval(mode);
                agent.persist_config();
                ui::info(&format!("Approval mode set to {}", mode.describe()));
                if mode == ApprovalMode::Yolo {
                    ui::warn("yolo approves everything except hard-blocked commands. Use only in disposable environments.");
                }
            }
            None => ui::plain(&format!(
                "Current approval mode: {}\nUsage: /approval <ask|always|strict|yolo>",
                agent.approval.mode.describe().yellow()
            )),
        },
        "/vault" => {
            if arg.is_empty() {
                ui::plain(&format!(
                    "Current Obsidian vault: {}",
                    agent.memory.vault_path.display().to_string().magenta()
                ));
                let others: Vec<PathBuf> = list_detected_vaults()
                    .into_iter()
                    .filter(|v| *v != agent.memory.vault_path)
                    .collect();
                if !others.is_empty() {
                    ui::plain("Other detected vaults:");
                    for v in others {
                        ui::plain(&format!("  - {}", v.display().to_string().cyan()));
                    }
                    ui::plain("Switch with /vault <path> or /vault auto");
                }
            } else {
                let path = if arg == "auto" || arg == "detect" {
                    detect_obsidian_vault()
                } else {
                    expand_tilde(&arg)
                };
                agent.set_vault(&path);
                ui::info(&format!(
                    "Obsidian vault set to {}",
                    path.display().to_string().magenta()
                ));
            }
        }
        "/memory" => {
            if arg.is_empty() {
                let content = agent.memory.load_profile_and_memories(8000);
                if content.trim().is_empty() {
                    ui::plain("Memory store is empty.");
                } else {
                    ui::plain(&format!("\n{}", content.cyan()));
                }
            } else {
                let hits = agent.memory.search_memories(&arg, 15);
                if hits.is_empty() {
                    ui::plain("No matching memories.");
                }
                for h in hits {
                    ui::plain(&format!("  {}", h.cyan()));
                }
            }
        }
        "/remember" => {
            if arg.is_empty() {
                ui::plain("Usage: /remember <fact or preference>");
            } else {
                match agent.memory.append_memory(&arg) {
                    Ok(m) => ui::info(&m),
                    Err(e) => ui::error(&e),
                }
            }
        }
        "/forget" => match agent.memory.forget(&arg) {
            Ok(n) => ui::info(&format!(
                "Removed {n} memory entr{}.",
                if n == 1 { "y" } else { "ies" }
            )),
            Err(e) => ui::warn(&e),
        },
        "/skills" => {
            let skills = agent.skills.lock().unwrap_or_else(|e| e.into_inner());
            let rows: Vec<(String, String, Vec<String>)> = skills
                .skills
                .iter()
                .map(|s| (s.name.clone(), s.description.clone(), s.tags.clone()))
                .collect();
            drop(skills);
            ui::render_skills_table(&rows);
        }
        "/skill" => {
            let skills = agent.skills.lock().unwrap_or_else(|e| e.into_inner());
            match skills.get(&arg) {
                Some(s) if !arg.is_empty() => ui::plain(&format!(
                    "\n{}\n{}\n\n{}\n",
                    s.name.bold().cyan(),
                    s.path.display().to_string().dimmed(),
                    s.instructions.cyan()
                )),
                _ => ui::plain("Usage: /skill <name>  (see /skills)"),
            }
        }
        "/use" => {
            if arg.is_empty() || arg == "off" || arg == "none" {
                agent.active_skill = None;
                agent.reset_system_prompt();
                ui::info("No skill pinned.");
            } else {
                let name = agent
                    .skills
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&arg)
                    .map(|s| s.name.clone());
                match name {
                    Some(n) => {
                        agent.active_skill = Some(n.clone());
                        agent.reset_system_prompt();
                        ui::info(&format!(
                            "Skill '{n}' pinned for this session (/use off to unpin)."
                        ));
                    }
                    None => ui::warn(&format!("Skill '{arg}' not found. See /skills.")),
                }
            }
        }
        "/todo" => {
            let todos = agent.todos.lock().unwrap_or_else(|e| e.into_inner());
            ui::plain(&crate::tools::render_todos(&todos));
        }
        "/sessions" if arg.starts_with("rm ") || arg.starts_with("delete ") => {
            let id = arg.split_whitespace().nth(1).unwrap_or("");
            if id == agent.meta.id {
                ui::warn("Cannot delete the current session; start /new first.");
            } else {
                match agent.store.load(id).and_then(|(m, _)| {
                    agent
                        .store
                        .delete(&m.id)
                        .map(|_| m.id)
                        .map_err(|e| e.to_string())
                }) {
                    Ok(deleted) => ui::info(&format!("Deleted session {deleted}.")),
                    Err(e) => ui::warn(&e),
                }
            }
        }
        "/sessions" => {
            let list = agent.store.list(20);
            if list.is_empty() {
                ui::plain("No saved sessions yet.");
            }
            for (i, m) in list.iter().enumerate() {
                let current = if m.id == agent.meta.id {
                    " (current)".green().to_string()
                } else {
                    String::new()
                };
                ui::plain(&format!(
                    "  {:>2}. {}  {}  {:>4} msgs  {}{}",
                    i + 1,
                    m.id.cyan(),
                    m.updated.format("%Y-%m-%d %H:%M"),
                    m.messages,
                    truncate_chars(&m.title, 50),
                    current
                ));
            }
            if !list.is_empty() {
                ui::note("Resume with /resume <number|id>");
            }
        }
        "/resume" => {
            let key = if arg.is_empty() {
                "1".to_string()
            } else {
                arg.clone()
            };
            match agent.resume(&key) {
                Ok(n) => {
                    ui::info(&format!(
                        "Resumed session {} ({} messages): {}",
                        agent.meta.id.cyan(),
                        n,
                        agent.meta.title
                    ));
                    if let Some(last) = agent.history.iter().rev().find(|m| {
                        m.role == crate::provider::Role::Assistant && !m.content.is_empty()
                    }) {
                        ui::note(&format!(
                            "Last reply: {}",
                            truncate_chars(last.content.lines().next().unwrap_or(""), 120)
                        ));
                    }
                }
                Err(e) => ui::warn(&e),
            }
        }
        "/new" | "/clear" | "/reset" => {
            agent.new_session();
            ui::info(&format!("Started a new session ({}).", agent.meta.id));
        }
        "/title" => {
            if arg.is_empty() {
                ui::plain(&format!("Title: {}", agent.meta.title));
            } else {
                agent.meta.title = arg.clone();
                agent.save_session();
                ui::info("Session renamed.");
            }
        }
        "/undo" => {
            let n = arg.parse().unwrap_or(1);
            match agent.undo(n) {
                Some(p) => ui::info(&format!(
                    "Removed {n} turn(s). Last removed prompt: {}",
                    truncate_chars(&p, 80)
                )),
                None => ui::warn("Nothing to undo."),
            }
        }
        "/retry" => match agent.undo(1) {
            Some(prompt) => {
                ui::note(&format!("Retrying: {}", truncate_chars(&prompt, 100)));
                run_and_render(agent, &prompt).await;
            }
            None => ui::warn("Nothing to retry."),
        },
        "/rollback" => match agent.checkpoints.rollback_last() {
            Ok(files) => {
                ui::info(&format!("Rolled back {} file(s):", files.len()));
                for f in files {
                    ui::plain(&format!("    {f}"));
                }
                ui::note("Shell commands are not tracked by checkpoints.");
            }
            Err(e) => ui::warn(&e),
        },
        "/checkpoints" => {
            let list = agent.checkpoints.list();
            if list.is_empty() {
                ui::plain("No file checkpoints in this session.");
            }
            for c in list {
                ui::plain(&format!(
                    "  turn {:>3}  {}  {}",
                    c.turn,
                    if c.backup.is_some() {
                        "modified"
                    } else {
                        "created "
                    },
                    c.path.display()
                ));
            }
        }
        "/compact" => {
            let msg = agent.compact_now().await;
            ui::info(&format!("Context compacted ({msg})."));
        }
        "/export" => {
            let mut parts = arg.split_whitespace();
            let fmt = parts.next().unwrap_or("md").to_lowercase();
            let body = if fmt == "json" {
                serde_json::to_string_pretty(
                    &serde_json::json!({"meta": agent.meta, "messages": agent.history}),
                )
                .unwrap_or_default()
            } else {
                crate::session::export_markdown(&agent.meta, &agent.history)
            };
            let path = parts.next().map(expand_tilde).unwrap_or_else(|| {
                PathBuf::from(format!(
                    "qsl-session-{}.{}",
                    agent.meta.id,
                    if fmt == "json" { "json" } else { "md" }
                ))
            });
            match std::fs::write(&path, body) {
                Ok(_) => ui::info(&format!("Exported session to {}", path.display())),
                Err(e) => ui::error(&format!("export failed: {e}")),
            }
        }
        "/usage" | "/stats" => {
            let s = agent.stats;
            ui::section("Session statistics");
            ui::plain(&format!(
                "  session      {}  ({})",
                agent.meta.id, agent.meta.title
            ));
            ui::plain(&format!(
                "  model        {}  ctx {} tokens",
                agent.config.model, agent.num_ctx
            ));
            ui::plain(&format!(
                "  turns        {}   tool calls {}   compactions {}",
                s.turns, s.tool_calls, s.compactions
            ));
            ui::plain(&format!(
                "  tokens       in {}  out {}  (engine-reported)",
                fmt_tokens(s.tokens_in as usize),
                fmt_tokens(s.tokens_out as usize)
            ));
            ui::plain(&format!(
                "  context      ~{} tokens in {} messages",
                fmt_tokens(agent.conversation_tokens()),
                agent.history.len()
            ));
        }
        "/think" => {
            agent.config.show_reasoning = arg != "off";
            ui::info(&format!(
                "Reasoning display {}.",
                if agent.config.show_reasoning {
                    "on"
                } else {
                    "off"
                }
            ));
        }
        "/stream" => {
            agent.config.stream = arg != "off";
            ui::info(&format!(
                "Streaming {}.",
                if agent.config.stream { "on" } else { "off" }
            ));
        }
        "/tools" => {
            ui::section("Available tools");
            for t in agent.tool_specs(0) {
                let kind = if crate::tools::class_of(t.name) == crate::tools::ToolClass::Mutating {
                    "mutating".yellow()
                } else {
                    "read-only".green()
                };
                ui::plain(&format!(
                    "  {:<18} [{}] {}",
                    t.name.cyan(),
                    kind,
                    truncate_chars(t.description.lines().next().unwrap_or(""), 80)
                ));
            }
            if !agent.client.native_tools() {
                ui::note("Engine lacks native tool calling; using the <tool_call> text protocol.");
            }
        }
        "/cd" => {
            let target = if arg.is_empty() {
                dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
            } else {
                expand_tilde(&arg)
            };
            match std::env::set_current_dir(&target) {
                Ok(_) => {
                    agent.reset_system_prompt();
                    ui::info(&format!(
                        "Working directory: {}",
                        std::env::current_dir()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default()
                    ));
                }
                Err(e) => ui::warn(&format!("cannot cd to {}: {e}", target.display())),
            }
        }
        "/config" => ui::plain(&format!(
            "\n# {}\n{}",
            agent.config_path.display(),
            agent.config.display_toml().dimmed()
        )),
        _ => {
            let close: Vec<&str> = COMMANDS
                .iter()
                .map(|(c, _)| *c)
                .filter(|c| c.starts_with(&cmd[..cmd.len().min(3)]))
                .collect();
            ui::warn(&format!(
                "Unknown command '{cmd}'.{} Type /help.",
                if close.is_empty() {
                    String::new()
                } else {
                    format!(" Did you mean {}?", close.join(", "))
                }
            ));
        }
    }
    false
}

fn show_providers(agent: &Agent) {
    let host = agent.config.endpoint_host();
    ui::plain(&format!(
        "Current engine: {} ({})\n  {}",
        agent.config.provider.yellow(),
        agent.client.endpoint().cyan(),
        if agent.config.is_remote() {
            format!("[hosted: data is sent to {host}]")
                .yellow()
                .bold()
                .to_string()
        } else {
            "[local: data stays on this machine]".dimmed().to_string()
        }
    ));
    ui::plain(&format!(
        "\n{}",
        "Local engines (your data stays on your machine)".bold()
    ));
    ui::plain(
        "  ollama | lmstudio | custom <url>   (custom may point at a local or remote server)",
    );
    ui::plain(&format!(
        "\n{}",
        "Hosted frontier models (your data is sent to the company named; you must accept a notice)"
            .bold()
    ));
    for p in crate::frontier::PRESETS {
        let key = if std::env::var(p.key_env).is_ok_and(|k| !k.trim().is_empty()) {
            "key found".green().to_string()
        } else {
            format!("export {} first", p.key_env).dimmed().to_string()
        };
        ui::plain(&format!(
            "  {:<11} {} - {}  [{}]",
            p.id.bold(),
            p.product,
            p.company,
            key
        ));
    }
    ui::plain("\nSwitch with /provider <name>, then /models and /model <name>. Back to local: /provider ollama");
}

/// Switch engine. A hosted target needs an API key and an explicit "yes" to the
/// data-sharing notice; the acceptance is remembered per host in the config.
async fn switch_provider(agent: &mut Agent, id: &str, url: Option<&str>) {
    let previous = agent.config.clone();
    agent.config.provider = id.to_string();
    if let Some(url) = url {
        match id {
            "ollama" => agent.config.ollama_url = url.into(),
            "lmstudio" => agent.config.lmstudio_url = url.into(),
            "custom" => agent.config.custom_url = Some(url.into()),
            _ => ui::warn("Hosted presets have a fixed address; the URL was ignored (use `custom` for others)."),
        }
    }
    let preset = crate::frontier::preset(id);
    if agent.config.is_remote() {
        if preset.is_some() && agent.config.resolved_api_key().is_none() {
            let env = preset.map_or("QSL_API_KEY", |p| p.key_env);
            ui::warn(&format!(
                "No API key found. Exit, run `export {env}=...` (keys are read from the environment and never saved), and start again."
            ));
            agent.config = previous;
            return;
        }
        if !agent.config.has_cloud_consent() {
            let host = agent.config.endpoint_host();
            let (company, product, note) = preset.map_or(
                (
                    host.clone(),
                    "a custom remote endpoint".to_string(),
                    String::new(),
                ),
                |p| {
                    (
                        p.company.to_string(),
                        p.product.to_string(),
                        p.note.to_string(),
                    )
                },
            );
            let lines = crate::frontier::disclaimer(&company, &host, &product, &note);
            if !ui::cloud_notice(&lines) {
                agent.config = previous;
                ui::info("Cancelled. Nothing was sent; the engine was not changed.");
                return;
            }
            agent.config.cloud_consent.push(host);
        }
    }
    agent.rebuild_client().await;
    agent.persist_config();
    if agent.config.is_remote() {
        ui::warn(&format!(
            "Hosted engine active: prompts, file contents and command output now go to {}.",
            agent.config.endpoint_host()
        ));
        ui::note("Pick a model with /models and /model <name>. Set context_size in the config if the model's window is larger than the assumed one.");
    } else {
        ui::info("Local engine active: your data stays on this machine.");
    }
    ui::info(&format!(
        "Switched engine to '{}' ({})",
        id.bold(),
        agent.client.endpoint().cyan()
    ));
    if agent
        .client
        .list_models()
        .await
        .map(|m| !m.contains(&agent.config.model))
        .unwrap_or(true)
    {
        ui::warn("The current model may not exist on this engine; use /models and /model <name>.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiline_normalization() {
        assert_eq!(
            normalize_input("line one\\\nline two"),
            "line one\nline two"
        );
        assert_eq!(
            normalize_input("\"\"\"\nfn main() {}\n\"\"\""),
            "fn main() {}"
        );
        assert_eq!(normalize_input("  hi  "), "hi");
    }

    #[test]
    fn completion_of_commands() {
        let h = ReplHelper {
            skills: vec!["rust-cargo".into(), "refactor".into()],
        };
        let hist = DefaultHistory::new();
        let ctx = Context::new(&hist);
        let (_, c) = h.complete("/mo", 3, &ctx).unwrap();
        let names: Vec<_> = c.iter().map(|p| p.display.clone()).collect();
        assert!(names.contains(&"/model".to_string()) && names.contains(&"/models".to_string()));
        let (start, c) = h.complete("/use ru", 7, &ctx).unwrap();
        assert_eq!(start, 5);
        assert_eq!(c[0].replacement, "rust-cargo");
    }
}
