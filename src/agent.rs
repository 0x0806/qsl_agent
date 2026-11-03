use crate::approval::{ApprovalManager, ApprovalMode};
use crate::config::Config;
use crate::memory::ObsidianMemory;
use crate::provider::{ChatMessage, FunctionCall, LocalLlmClient, ToolCall};
use crate::skills::SkillManager;
use crate::tools::ToolRegistry;
use crate::ui::UI;
use chrono::Local;
use colored::*;
use regex::Regex;
use serde_json::Value;
use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::time::{sleep, Duration, Instant};

/// The core autonomous agent orchestrator implementing an adaptive ReAct (Reason+Act) loop
pub struct QslAgent {
    pub config: Config,
    pub memory: ObsidianMemory,
    pub skills: SkillManager,
    pub client: LocalLlmClient,
    pub approval: ApprovalManager,
    pub history: Vec<ChatMessage>,
}

impl QslAgent {
    pub fn new(config: Config) -> Self {
        let memory = ObsidianMemory::new(&config.obsidian_vault_path);
        let skills = SkillManager::new(&config.obsidian_vault_path);
        let endpoint = config.get_active_endpoint();
        let client = LocalLlmClient::new(&endpoint, &config.provider);
        let mode = if config.auto_approve_commands {
            ApprovalMode::Always
        } else {
            ApprovalMode::Ask
        };
        let approval = ApprovalManager::new(mode);

        Self {
            config,
            memory,
            skills,
            client,
            approval,
            history: Vec::new(),
        }
    }

    /// Build the comprehensive system prompt with context injection
    pub fn build_system_prompt(&self) -> String {
        let mem_context = self.memory.load_profile_and_memories();
        let current_time = Local::now().format("%Y-%m-%d %H:%M:%S %Z").to_string();
        let current_dir = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string());

        let tools = ToolRegistry::get_definitions();
        let mut tool_desc = String::new();
        for t in &tools {
            tool_desc.push_str(&format!(
                "- `{}`: {}\n",
                t.function.name, t.function.description,
            ));
        }

        let skills_summary = self.skills.list_skills_summary();

        format!(
            "You are QSL Agent — an ultra-lightweight, rapid autonomous AI agent built in Rust (v0.2.0, 2026).\n\
             You run 100% locally with zero cloud dependencies, zero external accounts, and complete privacy.\n\
             Your persistent memory and skills are stored directly inside an Obsidian vault.\n\n\
             ### System & Environment Context:\n\
             - Current Date/Time: {}\n\
             - Working Directory: {}\n\
             - Obsidian Vault: {}\n\
             - Local AI Provider: {} (Model: {})\n\
             - Approval Mode: {}\n\n\
             {}\
             ### Available Skills (use `skill_read` to load detailed procedures):\n\
             {}\n\n\
             ### Available Tools:\n\
             {}\n\n\
             ### Core Operating Rules:\n\
             1. Be direct, concise, and actionable. Avoid unnecessary preamble.\n\
             2. Use tools to gather real information — never fabricate file contents, command output, or data.\n\
             3. If you learn important facts or user preferences, persist them with `obsidian_append`.\n\
             4. For specialized domains, load the relevant skill with `skill_read` first.\n\
             5. Security Guard actively blocks destructive commands. Do not attempt to bypass it.\n\
             6. Tool efficiency: execute tasks in the fewest, most direct tool calls. For file creation, verify content formatting and word count in memory before calling `write_file`, avoiding redundant verification commands (like `wc -w`) unless explicitly requested.\n\
             7. When executing multi-step tasks, reason through each step before acting.\n\
             8. After completing a task, give a concise summary of what was done.\n",
            current_time,
            current_dir,
            self.memory.vault_path.display(),
            self.config.provider,
            self.config.model,
            self.approval.mode,
            mem_context,
            skills_summary,
            tool_desc
        )
    }

    /// Extract tool calls from text content (fallback for models without native tool calling)
    pub fn extract_text_tool_calls(&self, content: &str) -> Vec<ToolCall> {
        let mut calls = Vec::new();
        let re = Regex::new(r"(?s)<tool_call>(.*?)</tool_call>").unwrap();

        for (idx, cap) in re.captures_iter(content).enumerate() {
            let json_str = cap[1].trim();
            if let Ok(val) = serde_json::from_str::<Value>(json_str) {
                if let Some(name) = val.get("name").and_then(|n| n.as_str()) {
                    let args = val
                        .get("arguments")
                        .map(|a| {
                            if a.is_string() {
                                a.as_str().unwrap().to_string()
                            } else {
                                a.to_string()
                            }
                        })
                        .unwrap_or_else(|| "{}".to_string());

                    calls.push(ToolCall {
                        id: format!("call_text_{}", idx),
                        tool_type: "function".to_string(),
                        function: FunctionCall {
                            name: name.to_string(),
                            arguments: args,
                        },
                    });
                }
            }
        }
        calls
    }

    /// Parse and extract reasoning thought blocks (e.g. <think>...</think>) and clean response text
    pub fn extract_thought_and_response(text: &str) -> (Option<String>, String) {
        if text.contains("<think>") && text.contains("</think>") {
            let re = Regex::new(r"(?s)<think>(.*?)</think>").unwrap();
            let mut thoughts = Vec::new();
            for cap in re.captures_iter(text) {
                thoughts.push(cap[1].trim().to_string());
            }
            let cleaned = re.replace_all(text, "").trim().to_string();
            let combined_thought = thoughts.join("\n\n");
            let opt_thought = if combined_thought.is_empty() {
                None
            } else {
                Some(combined_thought)
            };
            (opt_thought, cleaned)
        } else if text.contains("</think>") {
            let parts: Vec<&str> = text.split("</think>").collect();
            let thought = parts[..parts.len() - 1].join("\n").replace("<think>", "").trim().to_string();
            let cleaned = parts.last().unwrap_or(&"").trim().to_string();
            let opt_thought = if thought.is_empty() { None } else { Some(thought) };
            (opt_thought, cleaned)
        } else if text.starts_with("<think>") {
            let thought = text.replace("<think>", "").trim().to_string();
            (Some(thought), String::new())
        } else {
            (None, text.trim().to_string())
        }
    }

    /// Backward-compatible clean model output
    #[allow(dead_code)]
    pub fn clean_model_output(text: &str) -> String {
        Self::extract_thought_and_response(text).1
    }

    /// Run a single autonomous turn: send user input, handle tool calls in an adaptive ReAct loop
    pub async fn run_turn(
        &mut self,
        user_input: &str,
        is_interactive: bool,
    ) -> Result<String, String> {
        // Initialize system prompt on first message
        if self.history.is_empty() {
            let sys_prompt = self.build_system_prompt();
            self.history.push(ChatMessage {
                role: "system".to_string(),
                content: Some(sys_prompt),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            });
        }

        // Add user message
        self.history.push(ChatMessage {
            role: "user".to_string(),
            content: Some(user_input.to_string()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });

        let tool_defs = ToolRegistry::get_definitions();
        let mut max_steps = self.config.max_react_steps as usize;
        let timeout_secs = self.config.command_timeout_secs;
        let mut step = 0;
        let mut nudge_count = 0;
        let mut executed_signatures: Vec<String> = Vec::new();

        loop {
            step += 1;

            // Approaching limit directive: inject warning when 3 steps remain
            if step == max_steps.saturating_sub(3) && step > 3 {
                self.history.push(ChatMessage {
                    role: "system".to_string(),
                    content: Some(format!(
                        "[SYSTEM DIRECTIVE] Approaching step budget limit (step {} of {}). Conclude remaining tool operations immediately and synthesize your final comprehensive response.",
                        step, max_steps
                    )),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                });
            }

            // Adaptive step budget resolution when limit is exceeded
            if step > max_steps {
                if is_interactive {
                    println!();
                    println!("{}", "╭── [STEP LIMIT REACHED] ───────────────────────────────────────────╮".bold().yellow());
                    println!(
                        "│  Agent reached step budget limit ({}/{} steps).                   │",
                        step - 1,
                        max_steps
                    );
                    println!("├───────────────────────────────────────────────────────────────────┤");
                    println!(
                        "│  {} Extend (+15 steps)  │  {} Force synthesis  │  {} Stop turn    │",
                        "[c]".cyan().bold(),
                        "[f]".green().bold(),
                        "[s]".red().bold()
                    );
                    println!("{}", "╰───────────────────────────────────────────────────────────────────╯".bold().yellow());
                    print!("{}", "  action [c/f/s] > ".bold());
                    let _ = io::stdout().flush();

                    let mut choice = String::new();
                    let _ = io::stdin().read_line(&mut choice);
                    match choice.trim().to_lowercase().as_str() {
                        "c" | "continue" | "" => {
                            max_steps += 15;
                            println!(
                                "  {} Extended step budget by +15 (new limit: {} steps).",
                                "[OK]".green(),
                                max_steps
                            );
                        }
                        "f" | "finalize" | "synthesize" => {
                            println!("  {} Requesting final synthesis without tools...", "[WAIT]".yellow());
                            self.history.push(ChatMessage {
                                role: "user".to_string(),
                                content: Some(
                                    "Conclude all operations now. Synthesize all gathered data and provide your comprehensive final response. Do not invoke any further tools.".to_string(),
                                ),
                                tool_calls: None,
                                tool_call_id: None,
                                name: None,
                            });

                            let final_chat = self
                                .client
                                .chat(
                                    &self.config.model,
                                    &self.history,
                                    None,
                                    self.config.temperature,
                                    self.config.max_tokens,
                                    self.config.context_size,
                                )
                                .await?;

                            let raw_text = final_chat.content.unwrap_or_default();
                            let (thought, clean_text) = Self::extract_thought_and_response(&raw_text);
                            if let Some(th) = thought {
                                UI::render_reasoning(&th);
                            }

                            let final_text = if clean_text.is_empty() {
                                "Task concluded.".to_string()
                            } else {
                                clean_text
                            };

                            self.history.push(ChatMessage {
                                role: "assistant".to_string(),
                                content: Some(final_text.clone()),
                                tool_calls: None,
                                tool_call_id: None,
                                name: None,
                            });

                            let log_entry = format!("**User:** {}\n\n**Agent:**\n{}", user_input, final_text);
                            let _ = self.memory.append_daily_log(&log_entry);
                            return Ok(final_text);
                        }
                        _ => {
                            return Ok("Turn execution halted by user at step limit.".to_string());
                        }
                    }
                } else {
                    // Non-interactive mode: auto-extend once up to 50 steps
                    if max_steps < 50 {
                        max_steps += 15;
                        println!("  [Step budget dynamically auto-extended to {} steps]", max_steps);
                    } else {
                        // Beyond 50 steps: force final synthesis without crashing
                        println!("  [Maximum step budget reached; synthesizing final response]");
                        self.history.push(ChatMessage {
                            role: "user".to_string(),
                            content: Some(
                                "Step budget reached. Synthesize all findings and collected information into your comprehensive final answer now. Do not call any tools.".to_string(),
                            ),
                            tool_calls: None,
                            tool_call_id: None,
                            name: None,
                        });

                        let final_chat = self
                            .client
                            .chat(
                                &self.config.model,
                                &self.history,
                                None,
                                self.config.temperature,
                                self.config.max_tokens,
                                self.config.context_size,
                            )
                            .await?;

                        let raw_text = final_chat.content.unwrap_or_default();
                        let (thought, clean_text) = Self::extract_thought_and_response(&raw_text);
                        if let Some(th) = thought {
                            UI::render_reasoning(&th);
                        }

                        let final_text = if clean_text.is_empty() {
                            "Task concluded after reaching maximum step budget.".to_string()
                        } else {
                            clean_text
                        };

                        self.history.push(ChatMessage {
                            role: "assistant".to_string(),
                            content: Some(final_text.clone()),
                            tool_calls: None,
                            tool_call_id: None,
                            name: None,
                        });

                        let log_entry = format!("**User:** {}\n\n**Agent:**\n{}", user_input, final_text);
                        let _ = self.memory.append_daily_log(&log_entry);
                        return Ok(final_text);
                    }
                }
            }

            let step_label = match step {
                1 => "Analyzing prompt & cognitive planning...",
                2 => "Processing tool results & determining next action...",
                3 => "Executing follow-up operation...",
                _ => "Synthesizing output & evaluating completion...",
            };

            let start_time = Instant::now();
            let is_term = io::stdout().is_terminal();

            // Dynamic live ticker while waiting for local inference
            let done = Arc::new(AtomicBool::new(false));
            let done_clone = done.clone();
            let ticker_step = step;
            let ticker_max = max_steps;
            let ticker_label = step_label;

            let ticker_handle = tokio::spawn(async move {
                if !is_term {
                    return;
                }
                let frames = ["|", "/", "-", "\\"];
                let mut idx = 0;
                while !done_clone.load(Ordering::Relaxed) {
                    let elapsed = start_time.elapsed().as_secs();
                    let frame = frames[idx % frames.len()];
                    print!(
                        "\r  [Step {}/{}] {} ({}s) {}  ",
                        ticker_step, ticker_max, ticker_label, elapsed, frame
                    );
                    let _ = io::stdout().flush();
                    sleep(Duration::from_millis(200)).await;
                    idx += 1;
                }
            });

            if !is_term {
                println!("  [Step {}/{}] {}", step, max_steps, step_label);
            }

            let chat_result = self
                .client
                .chat(
                    &self.config.model,
                    &self.history,
                    Some(&tool_defs),
                    self.config.temperature,
                    self.config.max_tokens,
                    self.config.context_size,
                )
                .await;

            done.store(true, Ordering::Relaxed);
            let _ = ticker_handle.await;

            if is_term {
                let duration_secs = start_time.elapsed().as_secs_f32();
                print!("\r\x1b[2K"); // erase ticker line cleanly
                println!(
                    "{}",
                    format!("  [Step {}/{}] Inference completed in {:.1}s", step, max_steps, duration_secs).dimmed()
                );
                let _ = io::stdout().flush();
            }

            let response_msg = chat_result?;

            // Extract thought blocks (e.g. <think>...</think>) and render them
            if let Some(content) = &response_msg.content {
                let (thought, _) = Self::extract_thought_and_response(content);
                if let Some(th) = thought {
                    UI::render_reasoning(&th);
                }
            }

            // Check for tool calls (native or text-based fallback)
            let mut tool_calls = response_msg.tool_calls.clone().unwrap_or_default();
            if tool_calls.is_empty() {
                if let Some(content) = &response_msg.content {
                    tool_calls = self.extract_text_tool_calls(content);
                }
            }

            // No tool calls → final text response
            if tool_calls.is_empty() {
                let raw_text = response_msg.content.unwrap_or_default();
                let (_, clean_text) = Self::extract_thought_and_response(&raw_text);

                // If response is empty after tool calls, nudge the model (max 2 attempts)
                if clean_text.is_empty() && step > 1 && nudge_count < 2 {
                    nudge_count += 1;
                    self.history.push(ChatMessage {
                        role: "user".to_string(),
                        content: Some(
                            "Please provide a concise answer or summary based on the results above."
                                .to_string(),
                        ),
                        tool_calls: None,
                        tool_call_id: None,
                        name: None,
                    });
                    continue;
                }

                let final_text = if clean_text.is_empty() {
                    "Task completed.".to_string()
                } else {
                    clean_text
                };

                self.history.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: Some(final_text.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                });

                // Log to daily journal
                let log_entry = format!("**User:** {}\n\n**Agent:**\n{}", user_input, final_text);
                let _ = self.memory.append_daily_log(&log_entry);

                return Ok(final_text);
            }

            // Loop repetition detector: prevent models from spinning on the exact same tool calls
            for call in &tool_calls {
                let sig = format!("{}:{}", call.function.name, call.function.arguments.trim());
                let repetition_count = executed_signatures.iter().rev().take(3).filter(|s| *s == &sig).count();
                if repetition_count >= 1 {
                    self.history.push(ChatMessage {
                        role: "system".to_string(),
                        content: Some(format!(
                            "[SYSTEM DIRECTIVE] Repeated tool action detected for '{}'. You have already executed this action with identical parameters. Do NOT repeat it. Conclude this task or transition to the next distinct action now.",
                            call.function.name
                        )),
                        tool_calls: None,
                        tool_call_id: None,
                        name: None,
                    });
                }
                executed_signatures.push(sig);
            }

            // Add assistant message with tool calls
            self.history.push(ChatMessage {
                role: "assistant".to_string(),
                content: response_msg.content.clone(),
                tool_calls: Some(tool_calls.clone()),
                tool_call_id: None,
                name: None,
            });

            // Working indication before executing tools
            println!(
                "  {} Executing {} autonomous tool action(s)...",
                "[ACTION]".bold().cyan(),
                tool_calls.len()
            );

            // Execute each tool call
            for call in tool_calls {
                UI::render_tool_call(&call.function.name, &call.function.arguments);

                // Permission gate
                let approval_result = self.approval.check_approval(
                    &call.function.name,
                    &call.function.arguments,
                    is_interactive,
                );

                let output_str = match approval_result {
                    Ok(()) => {
                        let result = ToolRegistry::execute(
                            &call.function.name,
                            &call.function.arguments,
                            &self.memory,
                            &mut self.skills,
                            timeout_secs,
                        )
                        .await;

                        match result {
                            Ok(out) => {
                                UI::render_tool_result(&call.function.name, &out);
                                out
                            }
                            Err(err) => {
                                UI::render_tool_error(&call.function.name, &err);
                                format!("ERROR: {}", err)
                            }
                        }
                    }
                    Err(permission_err) => {
                        UI::render_tool_error(&call.function.name, &permission_err);
                        format!("DENIED: {}", permission_err)
                    }
                };

                self.history.push(ChatMessage {
                    role: "tool".to_string(),
                    content: Some(output_str),
                    tool_calls: None,
                    tool_call_id: Some(call.id),
                    name: Some(call.function.name),
                });
            }
        }
    }

    /// Clear conversation history (keeps system prompt on next turn)
    pub fn reset_context(&mut self) {
        self.history.clear();
    }

    /// Trim history to keep context size manageable
    #[allow(dead_code)]
    pub fn trim_history(&mut self, max_messages: usize) {
        if self.history.len() > max_messages {
            let system_msg = self.history.first().cloned();
            let keep_count = max_messages - 1;
            let start = self.history.len() - keep_count;
            let mut trimmed = Vec::with_capacity(max_messages);
            if let Some(sys) = system_msg {
                trimmed.push(sys);
            }
            trimmed.extend_from_slice(&self.history[start..]);
            self.history = trimmed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_thought_and_response_with_tags() {
        let input = "<think>\nAnalyzing the user's codebase structure.\nFound 5 files.\n</think>\nHere is the summary of your project.";
        let (thought, clean) = QslAgent::extract_thought_and_response(input);
        assert!(thought.is_some());
        assert_eq!(
            thought.unwrap(),
            "Analyzing the user's codebase structure.\nFound 5 files."
        );
        assert_eq!(clean, "Here is the summary of your project.");
    }

    #[test]
    fn test_extract_thought_and_response_no_tags() {
        let input = "Direct response without any reasoning block.";
        let (thought, clean) = QslAgent::extract_thought_and_response(input);
        assert!(thought.is_none());
        assert_eq!(clean, "Direct response without any reasoning block.");
    }

    #[test]
    fn test_extract_thought_unclosed_tag() {
        let input = "<think>\nThinking in progress...";
        let (thought, clean) = QslAgent::extract_thought_and_response(input);
        assert!(thought.is_some());
        assert_eq!(clean, "");
    }
}
