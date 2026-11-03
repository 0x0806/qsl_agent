use crate::agent::QslAgent;
use crate::approval::ApprovalMode;
use crate::config::Config;
use crate::ui::UI;
use colored::*;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::path::PathBuf;

pub async fn run_health_check(agent: &QslAgent) {
    println!();
    println!("{}", "╭── [SYSTEM HEALTH & DIAGNOSTIC] ───────────────────────────────────────╮".bold().bright_green());
    println!("│                                                                      │");
    
    // Check Engine connectivity
    print!("│  [1/4] Checking Local AI Provider ({}) ... ", agent.config.provider.cyan());
    match agent.client.health_check().await {
        Ok(_msg) => println!("{}", "ONLINE [OK]".green().bold()),
        Err(err) => println!("{} ({})", "OFFLINE [FAIL]".red().bold(), err),
    }

    // Check Loaded Models
    print!("│  [2/4] Querying available local models ... ");
    match agent.client.list_models().await {
        Ok(models) if !models.is_empty() => {
            println!("{} ({} found)", "DETECTED [OK]".green().bold(), models.len());
            for m in &models {
                let status = if *m == agent.config.model { "active" } else { "available" };
                println!("│        -> {} [{}]", m.yellow(), status.dimmed());
            }
        }
        Ok(_) => println!("{}", "EMPTY (0 models running) [FAIL]".yellow().bold()),
        Err(e) => println!("{} ({})", "ERROR [FAIL]".red().bold(), e),
    }

    // Check Obsidian Vault
    print!("│  [3/4] Validating Obsidian Vault at '{}' ... ", agent.memory.vault_path.display().to_string().magenta());
    if agent.memory.vault_path.exists() {
        let mem_file = agent.memory.memories_file().exists();
        let prof_file = agent.memory.profile_file().exists();
        if mem_file && prof_file {
            println!("{}", "CONNECTED & INITIALIZED [OK]".green().bold());
        } else {
            println!("{}", "CONNECTED (Pending First Note) [OK]".yellow().bold());
        }
    } else {
        println!("{}", "PATH NOT FOUND [FAIL]".red().bold());
    }

    // Check Skills Count
    let skills_count = agent.skills.skills.len();
    println!(
        "│  [4/5] Dynamic Skills Catalog: {} operational skills loaded [OK]",
        skills_count.to_string().green().bold()
    );

    // Organization Attribution
    println!(
        "│  [5/5] Origin: {} [OK]",
        "Quantum Secular Labs (https://quantumseclabs.com/)".cyan()
    );

    println!("│                                                                      │");
    println!("{}", "╰──────────────────────────────────────────────────────────────────────╯".bold().bright_green());
    println!();
}

pub async fn run_repl(mut agent: QslAgent) {
    let mode_str = match agent.approval.mode {
        ApprovalMode::Ask => "Ask [Interactive Review: y/a/n]",
        ApprovalMode::Always => "Always Allow [Full Autonomous]",
        ApprovalMode::Strict => "Strict [Zero Bypass Prompting]",
    };

    UI::render_banner(
        &agent.config.provider,
        &agent.config.model,
        &agent.memory.vault_path.display().to_string(),
        mode_str,
        agent.skills.skills.len(),
    );

    let mut rl = match DefaultEditor::new() {
        Ok(editor) => editor,
        Err(e) => {
            eprintln!("Failed to initialize readline editor: {}", e);
            return;
        }
    };

    let history_file = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("qsl_agent_history.txt");
    let _ = rl.load_history(&history_file);

    loop {
        let prompt = format!("{} ", "qsl-agent >".bold().bright_cyan());
        let readline = rl.readline(&prompt);

        match readline {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(trimmed);

                // Handle slash commands
                if trimmed.starts_with('/') {
                    let handled = handle_slash_command(trimmed, &mut agent).await;
                    if handled {
                        break; // exit signal
                    }
                    continue;
                }

                // Run autonomous agent turn (interactive = true)
                match agent.run_turn(trimmed, true).await {
                    Ok(response) => {
                        println!("\n{}\n", response);
                    }
                    Err(err) => {
                        println!("\n{} {}\n", "Agent Error:".bold().red(), err);
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("{}", "\n(Interrupted. Type /exit or /quit to terminate)".dimmed());
            }
            Err(ReadlineError::Eof) => {
                println!("{}", "\nExiting QSL Agent. Goodbye!".green());
                break;
            }
            Err(err) => {
                eprintln!("Readline error: {:?}", err);
                break;
            }
        }
    }

    let _ = rl.save_history(&history_file);
}

async fn handle_slash_command(cmd: &str, agent: &mut QslAgent) -> bool {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    let command = parts[0].to_lowercase();

    match command.as_str() {
        "/exit" | "/quit" => {
            println!("{}", "Exiting QSL Agent. Have a productive day!".green());
            return true;
        }
        "/help" => {
            println!("\n{}", format!("QSL AGENT (v{}) — Developed by Quantum Secular Labs (https://quantumseclabs.com/)", UI::VERSION).bold().green());
            println!("{}\n", "100% Local Inference | Obsidian PKM Cortex | Zero Cloud Dependency".dimmed());
            println!("{}", "AVAILABLE COMMANDS & CONTROLS:".bold().cyan());
            println!("  {}            Show this detailed help manual", "/help".yellow().bold());
            println!("  {}          Run full health & connectivity check", "/health".yellow().bold());
            println!("  {}          List detected local models on active engine", "/models".yellow().bold());
            println!("  {}   Switch active model on the fly", "/model <name>".yellow().bold());
            println!("  {} Switch engine (ollama, lmstudio, custom)", "/provider <p>".yellow().bold());
            println!("  {}  Set security approval (ask, always, strict)", "/approval <m>".yellow().bold());
            println!("  {}   View or switch Obsidian vault location", "/vault [path]".yellow().bold());
            println!("  {}          Display loaded Obsidian profile & memories", "/memory".yellow().bold());
            println!("  {}  Persist custom fact or rule into Obsidian", "/remember <txt>".yellow().bold());
            println!("  {}          Display catalog of all loaded skills", "/skills".yellow().bold());
            println!("  {}   Read procedural rules for a specific skill", "/skill <name>".yellow().bold());
            println!("  {}           Clear current conversation context", "/clear".yellow().bold());
            println!("  {}          View current configuration TOML", "/config".yellow().bold());
            println!("  {}            Exit the QSL Agent\n", "/exit".yellow().bold());
        }
        "/health" => {
            run_health_check(agent).await;
        }
        "/models" => {
            println!("{}", "Scanning local AI engine for models...".dimmed());
            match agent.client.list_models().await {
                Ok(models) => {
                    println!("\n{}", "Detected Local Models:".bold().green());
                    if models.is_empty() {
                        println!("  No models currently running or loaded on provider endpoint.");
                    } else {
                        for m in &models {
                            if *m == agent.config.model {
                                println!("  {} {} (active model)", "●".green().bold(), m.bold());
                            } else {
                                println!("  {} {}", "○".dimmed(), m);
                            }
                        }
                    }
                    println!();
                }
                Err(e) => {
                    println!("{} {}", "Failed to list models:".bold().red(), e);
                }
            }
        }
        "/model" => {
            if parts.len() > 1 {
                let new_model = parts[1..].join(" ");
                agent.config.model = new_model.clone();
                let _ = agent.config.save();
                agent.reset_context();
                println!("{} Switched active model to '{}'", "[OK]".green(), new_model.bold());
            } else {
                println!("Current model: {}", agent.config.model.yellow());
                println!("Usage: /model <model_name>");
            }
        }
        "/provider" => {
            if parts.len() > 1 {
                let p = parts[1].to_lowercase();
                if ["ollama", "lmstudio", "custom"].contains(&p.as_str()) {
                    agent.config.provider = p.clone();
                    let _ = agent.config.save();
                    let ep = agent.config.get_active_endpoint();
                    agent.client = crate::provider::LocalLlmClient::new(&ep, &agent.config.provider);
                    agent.reset_context();
                    println!("{} Switched engine to '{}' ({})", "[OK]".green(), p.bold(), ep.cyan());
                } else {
                    println!("Unknown engine '{}'. Supported: ollama, lmstudio, custom", p.red());
                }
            } else {
                println!(
                    "Current engine: {} ({})",
                    agent.config.provider.yellow(),
                    agent.config.get_active_endpoint().cyan()
                );
            }
        }
        "/approval" | "/mode" => {
            if parts.len() > 1 {
                match parts[1].to_lowercase().as_str() {
                    "ask" => {
                        agent.approval.mode = ApprovalMode::Ask;
                        agent.approval.reset();
                        agent.config.auto_approve_commands = false;
                        let _ = agent.config.save();
                        println!("{} Approval mode set to 'Ask' (y=once, a=session, n=deny).", "[OK]".green());
                    }
                    "always" => {
                        agent.approval.mode = ApprovalMode::Always;
                        agent.config.auto_approve_commands = true;
                        let _ = agent.config.save();
                        println!("{} Approval mode set to 'Always' (full autonomous execution).", "[OK]".green());
                    }
                    "strict" => {
                        agent.approval.mode = ApprovalMode::Strict;
                        agent.approval.reset();
                        agent.config.auto_approve_commands = false;
                        let _ = agent.config.save();
                        println!("{} Approval mode set to 'Strict' (prompt every mutating call).", "[OK]".green());
                    }
                    _ => {
                        println!("Usage: /approval <ask | always | strict>");
                    }
                }
            } else {
                let current = match agent.approval.mode {
                    ApprovalMode::Ask => "Ask [Prompt: y/a/n]",
                    ApprovalMode::Always => "Always [Auto-approve safe actions]",
                    ApprovalMode::Strict => "Strict [Prompt on every mutating call]",
                };
                println!("Current approval mode: {}", current.yellow());
                println!("Usage: /approval <ask | always | strict>");
            }
        }
        "/vault" => {
            if parts.len() > 1 {
                let arg = parts[1..].join(" ");
                let new_path = if arg == "auto" || arg == "detect" {
                    let detected = Config::detect_obsidian_vault();
                    println!("{} Auto-detected Obsidian vault: {}", "[OK]".green(), detected.display().to_string().cyan());
                    detected
                } else {
                    PathBuf::from(arg)
                };
                agent.config.obsidian_vault_path = new_path.clone();
                let _ = agent.config.save();
                agent.memory = crate::memory::ObsidianMemory::new(&new_path);
                agent.skills.reload_skills(&new_path);
                agent.reset_context();
                println!("{} Obsidian vault updated to: {}", "[OK]".green(), new_path.display().to_string().magenta());
            } else {
                println!("Current Obsidian vault: {}", agent.memory.vault_path.display().to_string().magenta());
                let detected = Config::list_detected_vaults();
                if detected.len() > 1 {
                    println!("\nOther detected vaults on this system:");
                    for (i, v) in detected.iter().enumerate() {
                        if v != &agent.memory.vault_path {
                            println!("  [{}] {}", i + 1, v.display().to_string().cyan());
                        }
                    }
                    println!("To switch: /vault <path> or /vault auto");
                }
            }
        }
        "/memory" => {
            let content = agent.memory.load_profile_and_memories();
            if content.is_empty() {
                println!("Obsidian memory store is currently empty.");
            } else {
                println!("\n{}\n", content.cyan());
            }
        }
        "/remember" => {
            if parts.len() > 1 {
                let note = parts[1..].join(" ");
                match agent.memory.append_memory(&note) {
                    Ok(msg) => println!("{} {}", "[OK]".green(), msg),
                    Err(e) => println!("{} {}", "Error:".red(), e),
                }
            } else {
                println!("Usage: /remember <fact or preference to save>");
            }
        }
        "/skills" => {
            let list: Vec<(String, String, Vec<String>)> = agent
                .skills
                .skills
                .iter()
                .map(|s| (s.name.clone(), s.description.clone(), s.tags.clone()))
                .collect();
            UI::render_skills_table(&list);
        }
        "/skill" => {
            if parts.len() > 1 {
                let name = parts[1];
                if let Some(instructions) = agent.skills.get_skill_instructions(name) {
                    println!("\n{}\n", instructions.cyan());
                } else {
                    println!("Skill '{}' not found. Use '/skills' to list all available skills.", name.red());
                }
            } else {
                println!("Usage: /skill <skill_name>");
            }
        }
        "/clear" => {
            agent.reset_context();
            println!("{} Session context cleared.", "[OK]".green());
        }
        "/config" => {
            let serialized = toml::to_string_pretty(&agent.config).unwrap_or_default();
            println!("\n{}\n", serialized.dimmed());
        }
        _ => {
            println!("Unknown command '{}'. Type '/help' for available commands.", command.red());
        }
    }

    false
}
