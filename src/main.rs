mod agent;
mod approval;
mod brand;
mod checkpoint;
mod cli;
mod config;
mod context;
mod frontier;
mod interrupt;
mod memory;
mod provider;
mod security;
mod server;
mod session;
mod sizing;
mod skills;
mod skills_builtin;
mod skills_bundle;
mod tools;
mod ui;
mod util;

use clap::Parser;
use std::io::{IsTerminal, Read};
use std::path::PathBuf;

use crate::config::{ApprovalMode, Config};

/// QSL Agent — fast, local-first autonomous AI agent (Ollama, LM Studio,
/// llama.cpp, vLLM or any OpenAI-compatible server) with Obsidian memory.
#[derive(Parser, Debug)]
#[command(
    name = "qsl-agent",
    version,
    long_version = concat!(env!("CARGO_PKG_VERSION"), "\nQuantum Secular Labs - https://quantumseclabs.com/\n(c) 2026 Quantum Secular Labs. All rights reserved.\nLicence: QSL Team Source-Available License v1.0 (2026); third-party notices: THIRD_PARTY_NOTICES.md"),
    author = "Quantum Secular Labs <https://quantumseclabs.com/>",
    about = "QSL Agent — fast, local-first autonomous AI agent with Obsidian memory",
    after_help = "Examples:\n  qsl-agent                                   interactive REPL\n  qsl-agent \"summarise this repo\"              one-shot task\n  git diff | qsl-agent -P - \"review this\"       read the prompt from stdin\n  qsl-agent -C                                continue the last session\n  qsl-agent --serve 127.0.0.1:8765            HTTP API"
)]
struct Args {
    /// Prompt words for a one-shot task (same as -P)
    #[arg(value_name = "PROMPT")]
    words: Vec<String>,

    /// Non-interactive task prompt ("-" reads stdin)
    #[arg(short = 'P', long)]
    prompt: Option<String>,

    /// Model name (e.g. qwen2.5-coder:7b)
    #[arg(short, long)]
    model: Option<String>,

    /// Provider: ollama, lmstudio, custom
    #[arg(short, long)]
    provider: Option<String>,

    /// Custom OpenAI-compatible endpoint (implies --provider custom)
    #[arg(short, long)]
    endpoint: Option<String>,

    /// API key for remote OpenAI-compatible endpoints
    #[arg(long, env = "QSL_API_KEY", hide_env_values = true)]
    api_key: Option<String>,

    /// Accept, for this run only, that a hosted (non-local) engine receives your prompts, files read and command output
    #[arg(long, env = "QSL_ACCEPT_CLOUD")]
    accept_cloud: bool,

    /// Path to the Obsidian vault
    #[arg(short, long)]
    vault: Option<PathBuf>,

    /// Approval mode: ask, always, strict, yolo
    #[arg(short = 'a', long)]
    approval: Option<String>,

    /// Shorthand for --approval yolo (disposable environments only)
    #[arg(long)]
    yolo: bool,

    /// Sampling temperature (0.0 - 2.0)
    #[arg(short, long)]
    temperature: Option<f32>,

    /// Context window in tokens (0 = auto-size for this model and machine)
    #[arg(short = 'c', long)]
    context_size: Option<u32>,

    /// Maximum ReAct steps per turn
    #[arg(long)]
    max_steps: Option<u32>,

    /// Use a specific config file
    #[arg(long, env = "QSL_CONFIG")]
    config: Option<PathBuf>,

    /// Continue the most recent session
    #[arg(short = 'C', long = "continue")]
    continue_last: bool,

    /// Resume a session by id, id prefix, or list number
    #[arg(short = 'r', long)]
    resume: Option<String>,

    /// Disable token streaming
    #[arg(long)]
    no_stream: bool,

    /// Disable colours
    #[arg(long)]
    no_color: bool,

    /// One-shot: print only the final answer
    #[arg(short, long)]
    quiet: bool,

    /// One-shot: print a JSON result object
    #[arg(long)]
    json: bool,

    /// Serve the HTTP API on ADDR (default 127.0.0.1:8765)
    #[arg(long, value_name = "ADDR", num_args = 0..=1, default_missing_value = "127.0.0.1:8765")]
    serve: Option<String>,

    /// List detected local models and exit
    #[arg(long)]
    list_models: bool,

    /// List saved sessions and exit
    #[arg(long)]
    list_sessions: bool,

    /// Print the active configuration and exit
    #[arg(long)]
    show_config: bool,

    /// Run the health check and exit (non-zero exit code on failure)
    #[arg(long)]
    health_check: bool,

    /// Verbose logging to <state_dir>/qsl-agent.log
    #[arg(long)]
    verbose: bool,
}

fn init_logging(state_dir: &std::path::Path, verbose: bool) {
    let _ = std::fs::create_dir_all(state_dir);
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(state_dir.join("qsl-agent.log"))
    else {
        return;
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(if verbose {
            "qsl_agent=debug,warn"
        } else {
            "qsl_agent=info,warn"
        })
    });
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_writer(std::sync::Mutex::new(file))
        .try_init();
}

fn apply_overrides(cfg: &mut Config, args: &Args) -> Result<(), String> {
    if let Some(p) = &args.provider {
        cfg.provider = p.to_lowercase();
    }
    if let Some(ep) = &args.endpoint {
        cfg.custom_url = Some(ep.clone());
        cfg.provider = "custom".into();
    }
    if let Some(m) = &args.model {
        cfg.model = m.clone();
    }
    if let Some(k) = &args.api_key {
        cfg.api_key = Some(k.clone());
    }
    if let Some(v) = &args.vault {
        cfg.obsidian_vault_path = v.clone();
    }
    if let Some(t) = args.temperature {
        cfg.temperature = t;
    }
    if let Some(c) = args.context_size {
        cfg.context_size = c;
    }
    if let Some(s) = args.max_steps {
        cfg.max_react_steps = s;
    }
    if args.no_stream {
        cfg.stream = false;
    }
    if let Some(a) = &args.approval {
        let mode = ApprovalMode::parse(a).ok_or_else(|| {
            format!("unknown approval mode '{a}'. Use: ask, always, strict, yolo")
        })?;
        cfg.approval_mode = Some(mode);
    }
    if args.yolo {
        cfg.approval_mode = Some(ApprovalMode::Yolo);
    }
    cfg.normalize();
    Ok(())
}

fn read_prompt(args: &Args) -> Result<Option<String>, String> {
    let mut parts: Vec<String> = Vec::new();
    let mut want_stdin = false;
    match args.prompt.as_deref() {
        Some("-") => want_stdin = true,
        Some(p) => parts.push(p.to_string()),
        None => {}
    }
    if !args.words.is_empty() {
        if args.words.len() == 1 && args.words[0] == "-" {
            want_stdin = true;
        } else {
            parts.push(args.words.join(" "));
        }
    }
    if want_stdin || (!std::io::stdin().is_terminal() && !parts.is_empty() && args.serve.is_none())
    {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("failed to read stdin: {e}"))?;
        if !buf.trim().is_empty() {
            if parts.is_empty() {
                parts.push(buf.trim().to_string());
            } else {
                parts.push(format!("<stdin>\n{}\n</stdin>", buf.trim_end()));
            }
        }
    }
    Ok(if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    })
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let args = Args::parse();
    ui::init_colors(args.no_color);
    // Install tokio's SIGINT handler before any line editor exists, so it is
    // never saved/restored over: Ctrl+C must interrupt a turn (and kill its
    // child processes cleanly), not terminate the agent.
    #[cfg(unix)]
    let _sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt()).ok();

    let loaded = Config::load(args.config.as_deref());
    let mut cfg = loaded.config;
    if let Err(e) = apply_overrides(&mut cfg, &args) {
        eprintln!("error: {e}");
        std::process::exit(2);
    }
    init_logging(&cfg.state_dir(), args.verbose);
    if let Some(w) = &loaded.warning {
        ui::warn(w);
        tracing::warn!("{w}");
    }

    if args.show_config {
        println!("# {}\n{}", loaded.path.display(), cfg.display_toml());
        return;
    }

    let prompt = match read_prompt(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    };
    let one_shot = prompt.is_some();
    let interactive = !one_shot && args.serve.is_none() && std::io::stdin().is_terminal();
    if (one_shot && (args.quiet || args.json)) || args.serve.is_some() {
        ui::set_quiet(true);
    }

    let mut consent_accepted_now = false;
    if cfg.is_remote() && !cfg.has_cloud_consent() {
        let host = cfg.endpoint_host();
        if args.accept_cloud {
            cfg.session_consent.push(host);
        } else if interactive && std::io::stdout().is_terminal() {
            let (company, product, note) = frontier::preset(&cfg.provider).map_or(
                (host.clone(), "a remote endpoint".to_string(), String::new()),
                |p| {
                    (
                        p.company.to_string(),
                        p.product.to_string(),
                        p.note.to_string(),
                    )
                },
            );
            if !ui::cloud_notice(&frontier::disclaimer(&company, &host, &product, &note)) {
                eprintln!("error: not approved; nothing was sent. Use `--provider ollama` for a local engine.");
                std::process::exit(1);
            }
            cfg.cloud_consent.push(host);
            consent_accepted_now = true;
        } else {
            eprintln!(
                "error: the engine {host} is hosted, so your prompts, files and command output would be sent to it. Re-run with --accept-cloud (or QSL_ACCEPT_CLOUD=1) to accept that, or use a local engine."
            );
            std::process::exit(2);
        }
    }
    if let Some(p) = frontier::preset(&cfg.provider) {
        if cfg.resolved_api_key().is_none() {
            eprintln!(
                "error: no API key for {}. Set {} in the environment.",
                p.company, p.key_env
            );
            std::process::exit(2);
        }
    }

    let mut agent = agent::Agent::new(cfg, loaded.path.clone(), interactive);
    if consent_accepted_now {
        agent.persist_config();
    }

    if args.list_sessions {
        let list = agent.store.list(50);
        if list.is_empty() {
            println!("No saved sessions.");
        }
        for (i, m) in list.iter().enumerate() {
            println!(
                "{:>3}. {}  {}  {:>4} msgs  {}",
                i + 1,
                m.id,
                m.updated.format("%Y-%m-%d %H:%M"),
                m.messages,
                m.title
            );
        }
        return;
    }

    agent.init().await;

    if args.list_models {
        match agent.client.list_models().await {
            Ok(models) if models.is_empty() => {
                println!("No models found on {}.", agent.client.endpoint())
            }
            Ok(models) => {
                println!("Models on {}:", agent.client.endpoint());
                for m in models {
                    println!(
                        "  {} {m}",
                        if m == agent.config.model {
                            "●"
                        } else {
                            "○"
                        }
                    );
                }
            }
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    if args.health_check {
        let ok = cli::run_health_check(&agent).await;
        std::process::exit(if ok { 0 } else { 1 });
    }

    let resume_key = args.resume.clone().or_else(|| {
        if args.continue_last {
            agent.store.latest()
        } else {
            None
        }
    });
    let mut resumed = 0;
    if let Some(key) = resume_key {
        match agent.resume(&key) {
            Ok(n) => resumed = n,
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(2);
            }
        }
    }

    if let Some(addr) = &args.serve {
        let addr: std::net::SocketAddr = match addr.parse() {
            Ok(a) => a,
            Err(e) => {
                eprintln!("error: invalid --serve address '{addr}': {e}");
                std::process::exit(2);
            }
        };
        if let Err(e) = server::serve(agent, addr).await {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
        return;
    }

    if let Some(w) = agent.context_warning() {
        if one_shot {
            eprintln!("warning: context: {w}");
        }
    }

    if let Some(prompt) = prompt {
        // Ctrl+C during a one-shot run interrupts the turn cleanly.
        let intr = agent.interrupt.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                intr.trigger();
            }
        });
        let result = agent.run_turn(&prompt).await;
        match result {
            Ok(o) => {
                if args.json {
                    let v = serde_json::json!({
                        "ok": true,
                        "reply": o.text,
                        "session_id": agent.meta.id,
                        "model": agent.config.model,
                        "steps": o.steps,
                        "tool_calls": o.tool_calls,
                        "elapsed_ms": o.elapsed.as_millis() as u64,
                        "tokens_in": o.tokens_in,
                        "tokens_out": o.tokens_out,
                    });
                    println!("{v}");
                } else if args.quiet {
                    println!("{}", o.text.trim());
                } else {
                    if !agent.config.stream {
                        ui::render_final_response(&o.text);
                    }
                    ui::status_line(&agent.status_parts(&o));
                }
            }
            Err(e) => {
                let code = if agent.interrupt.is_set() { 130 } else { 1 };
                if args.json {
                    println!(
                        "{}",
                        serde_json::json!({ "ok": false, "error": e, "session_id": agent.meta.id })
                    );
                } else {
                    ui::error(&e);
                }
                std::process::exit(code);
            }
        }
        return;
    }

    if !std::io::stdin().is_terminal() {
        eprintln!("error: stdin is not a terminal. Pass a prompt (qsl-agent \"task\" or -P -) for non-interactive use.");
        std::process::exit(2);
    }
    if resumed > 0 {
        ui::info(&format!(
            "Resumed session {} ({} messages): {}",
            agent.meta.id, resumed, agent.meta.title
        ));
    }
    cli::run_repl(agent).await;
}
