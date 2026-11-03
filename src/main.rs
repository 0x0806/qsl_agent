mod agent;
mod approval;
mod cli;
mod config;
mod memory;
mod provider;
mod skills;
mod tools;
mod ui;

use agent::QslAgent;
use clap::Parser;
use config::Config;
use std::path::PathBuf;

/// QSL Agent — Ultra-lightweight autonomous AI agent (2026)
/// 100% local inference with Ollama, LM Studio, or GGUF models.
/// Persistent memory via Obsidian vault. Zero cloud. Zero accounts.
#[derive(Parser, Debug)]
#[command(
    name = "qsl-agent",
    about = "QSL Agent — Ultra-lightweight, rapid Rust autonomous AI agent for local Ollama, LM Studio, GGUF, and Obsidian memory (2026)",
    version = env!("CARGO_PKG_VERSION"),
    author = "Quantum Secular Labs <https://quantumseclabs.com/>"
)]
struct Args {
    /// Non-interactive task prompt to execute immediately
    #[arg(short = 'P', long)]
    prompt: Option<String>,

    /// Model name (e.g. huihui-qwen35-9b-256k:latest, qwen2.5-coder)
    #[arg(short, long)]
    model: Option<String>,

    /// Provider: ollama, lmstudio, or custom
    #[arg(short, long)]
    provider: Option<String>,

    /// Custom endpoint URL (e.g. http://127.0.0.1:11434, http://127.0.0.1:1234/v1)
    #[arg(short, long)]
    endpoint: Option<String>,

    /// Path to Obsidian Vault (defaults to auto-detected local vault or ~/Documents/Obsidian_Vault)
    #[arg(short, long)]
    vault: Option<PathBuf>,

    /// Approval mode: ask, always, strict
    #[arg(short = 'a', long)]
    approval: Option<String>,

    /// Generation temperature (0.0 to 1.0)
    #[arg(short, long)]
    temperature: Option<f32>,

    /// Context window size for local model (tokens)
    #[arg(short = 'c', long)]
    context_size: Option<u32>,

    /// List detected local models and exit
    #[arg(long)]
    list_models: bool,

    /// Show current configuration and exit
    #[arg(long)]
    show_config: bool,

    /// Run health check on local AI provider and exit
    #[arg(long)]
    health_check: bool,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let mut config = Config::load();

    // Apply CLI overrides
    if let Some(m) = args.model {
        config.model = m;
    }
    if let Some(p) = args.provider {
        config.provider = p;
    }
    if let Some(ep) = args.endpoint {
        config.custom_url = Some(ep);
        config.provider = "custom".to_string();
    }
    if let Some(v) = args.vault {
        config.obsidian_vault_path = v;
    }
    if let Some(t) = args.temperature {
        config.temperature = t.clamp(0.0, 2.0);
    }
    if let Some(ctx) = args.context_size {
        config.context_size = ctx;
    }
    if let Some(appr) = args.approval {
        match appr.to_lowercase().as_str() {
            "always" => config.auto_approve_commands = true,
            "ask" | "strict" => config.auto_approve_commands = false,
            _ => {
                eprintln!("Unknown approval mode '{}'. Use: ask, always, strict", appr);
                std::process::exit(1);
            }
        }
    }

    let mut agent = QslAgent::new(config);

    // Auto-detect running local models if model field is empty or default
    if let Some(detected) = agent.client.auto_detect_model().await {
        if agent.config.model.is_empty() {
            println!("  Auto-detected model: {}", detected);
            agent.config.model = detected;
        }
    }

    // Handle one-shot flag commands
    if args.list_models {
        match agent.client.list_models().await {
            Ok(models) => {
                if models.is_empty() {
                    println!("No models found on current provider endpoint.");
                } else {
                    println!("Detected local models:");
                    for m in &models {
                        if *m == agent.config.model {
                            println!("  ● {} (active)", m);
                        } else {
                            println!("  ○ {}", m);
                        }
                    }
                }
            }
            Err(e) => eprintln!("Error querying models: {}", e),
        }
        return;
    }

    if args.show_config {
        let serialized = toml::to_string_pretty(&agent.config).unwrap_or_default();
        println!("{}", serialized);
        return;
    }

    if args.health_check {
        cli::run_health_check(&agent).await;
        return;
    }

    if let Some(prompt_text) = args.prompt {
        match agent.run_turn(&prompt_text, false).await {
            Ok(ans) => println!("{}", ans),
            Err(err) => {
                eprintln!("Error: {}", err);
                std::process::exit(1);
            }
        }
    } else {
        cli::run_repl(agent).await;
    }
}
