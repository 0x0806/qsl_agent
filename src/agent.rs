//! The agent: an adaptive ReAct (reason + act) loop over a local LLM.

use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use crate::approval::{ApprovalManager, Decision};
use crate::checkpoint::Checkpoints;
use crate::config::Config;
use crate::context;
use crate::interrupt::Interrupt;
use crate::memory::ObsidianMemory;
use crate::provider::{
    ChatOptions, Delta, LlmClient, Message, ProviderError, Role, ToolCall, ToolSpec,
};
use crate::security::{CommandRisk, SecurityGuard};
use crate::session::{SessionMeta, SessionStore};
use crate::skills::SkillManager;
use crate::tools::{self, class_of, TodoItem, ToolClass, ToolCtx};
use crate::ui::{self, Spinner, StreamPrinter};
use crate::util::{fmt_tokens, smart_clip, truncate_chars};

const PROJECT_FILES: &[&str] = &["AGENTS.md", "QSL.md", ".qsl/instructions.md", "CLAUDE.md"];
const SUBAGENT_MAX_STEPS: usize = 20;

#[derive(Default, Clone, Copy)]
pub struct Stats {
    pub turns: u64,
    pub tool_calls: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub last_prompt_tokens: u64,
    pub compactions: u64,
}

pub struct TurnOutcome {
    pub text: String,
    pub steps: usize,
    pub tool_calls: usize,
    pub elapsed: Duration,
    pub tokens_in: u64,
    pub tokens_out: u64,
}

#[derive(Clone, Copy)]
struct LoopOpts {
    max_steps: usize,
    depth: u8,
}

pub struct Agent {
    pub config: Config,
    pub config_path: PathBuf,
    pub client: LlmClient,
    pub memory: ObsidianMemory,
    pub skills: Mutex<SkillManager>,
    pub security: SecurityGuard,
    pub approval: ApprovalManager,
    pub store: SessionStore,
    pub meta: SessionMeta,
    pub history: Vec<Message>,
    system_base: String,
    pub summary: Option<String>,
    pub active_skill: Option<String>,
    pub todos: Mutex<Vec<TodoItem>>,
    pub checkpoints: Checkpoints,
    http: reqwest::Client,
    pub interrupt: Interrupt,
    pub num_ctx: u32,
    /// How `num_ctx` was chosen (and any warning), for banner / health check.
    pub ctx_plan: Option<crate::sizing::Plan>,
    pub stats: Stats,
    turn_counter: u64,
    pub interactive: bool,
}

impl Agent {
    pub fn new(config: Config, config_path: PathBuf, interactive: bool) -> Self {
        let memory = ObsidianMemory::new(&config.obsidian_vault_path);
        let skills = Mutex::new(SkillManager::new(&memory.skills_dir()));
        let mut security = SecurityGuard::new(&config.allowed_roots);
        security.protect(&config_path);
        let state = config.state_dir();
        let meta = SessionMeta::new(&config.model);
        let checkpoints = Checkpoints::new(state.join("checkpoints").join(&meta.id));
        let http = reqwest::Client::builder()
            .user_agent(concat!(
                "Mozilla/5.0 (compatible; qsl-agent/",
                env!("CARGO_PKG_VERSION"),
                ")"
            ))
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::limited(8))
            .build()
            .unwrap_or_default();
        let mut agent = Self {
            client: LlmClient::new(&config),
            approval: ApprovalManager::new(config.effective_approval()),
            store: SessionStore::new(state.join("sessions"), config.save_sessions),
            num_ctx: if config.context_size == 0 {
                config.min_context_size
            } else {
                config.context_size.max(config.min_context_size)
            },
            ctx_plan: None,
            config,
            config_path,
            memory,
            skills,
            security,
            meta,
            history: Vec::new(),
            system_base: String::new(),
            summary: None,
            active_skill: None,
            todos: Mutex::new(Vec::new()),
            checkpoints,
            http,
            interrupt: Interrupt::new(),
            stats: Stats::default(),
            turn_counter: 0,
            interactive,
        };
        agent.reset_system_prompt();
        agent
    }

    /// Network-dependent setup: model auto-detection and context sizing.
    pub async fn init(&mut self) {
        if self.config.model.trim().is_empty() {
            if let Some(m) = self.client.auto_detect_model().await {
                self.config.model = m;
                self.meta.model = self.config.model.clone();
            }
        }
        self.detect_context().await;
        self.reset_system_prompt();
    }

    /// Choose the context window for this model on this machine.
    async fn detect_context(&mut self) {
        let limits = crate::sizing::Limits {
            requested: self.config.context_size,
            min: self.config.min_context_size,
            max: self.config.max_context_size,
        };
        let plan = if self.client.api == crate::config::ApiStyle::Ollama {
            let meta = self.client.model_meta(&self.config.model).await;
            crate::sizing::plan(&limits, &meta, crate::sizing::available_memory_bytes())
        } else {
            let detected = self.client.server_context(&self.config.model).await;
            crate::sizing::plan_server(&limits, &self.config.provider, detected)
        };
        tracing::info!(num_ctx = plan.num_ctx, reason = %plan.reason, "context window planned");
        self.num_ctx = plan.num_ctx;
        self.ctx_plan = Some(plan);
    }

    /// Warning to show the user when the context is below the agentic minimum.
    pub fn context_warning(&self) -> Option<&str> {
        self.ctx_plan.as_ref().and_then(|p| p.warning.as_deref())
    }

    // -----------------------------------------------------------------------
    // System prompt (kept stable within a session so the engine can reuse
    // its KV cache; only compaction summaries / pinned skills change it)
    // -----------------------------------------------------------------------

    fn project_context() -> String {
        let Ok(cwd) = std::env::current_dir() else {
            return String::new();
        };
        let mut out = String::new();
        for name in PROJECT_FILES {
            let p = cwd.join(name);
            if let Ok(text) = std::fs::read_to_string(&p) {
                if text.trim().is_empty() || out.len() > 12_000 {
                    continue;
                }
                out.push_str(&format!(
                    "#### {name}\n{}\n\n",
                    smart_clip(text.trim(), 6000)
                ));
            }
        }
        if out.is_empty() {
            out
        } else {
            format!("### Project Instructions (from the working directory; follow them)\n{out}")
        }
    }

    fn build_system_base(&self) -> String {
        let now = chrono::Local::now();
        let cwd = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| ".".into());
        let small_ctx = self.num_ctx <= 8192;
        let skills = {
            let mgr = self.skills.lock().unwrap_or_else(|e| e.into_inner());
            if small_ctx {
                mgr.summary_compact()
            } else {
                mgr.summary()
            }
        };
        let project = if self.config.project_context {
            Self::project_context()
        } else {
            String::new()
        };
        let memories = self
            .memory
            .load_profile_and_memories(if small_ctx { 1200 } else { 3500 });
        format!(
            "You are QSL Agent — a fast, local-first autonomous AI agent built in Rust (v{ver}) by Quantum Secular Labs.\n\
             You run on the user's machine with a local model. You can act on the system through tools; your long-term \
             memory and skills live in the user's Obsidian vault.\n\n\
             ### Environment\n\
             - Date: {date}\n\
             - OS: {os} | Shell: bash | User: {user}\n\
             - Working directory: {cwd}\n\
             - Obsidian vault: {vault}\n\
             - Model: {model} via {provider}\n\
             - Approval mode: {mode}\n\n\
             {project}{memories}\
             ### Skills (load full instructions with `skill_read` before specialised work)\n{skills}\n\
             ### Operating Rules\n\
             1. Be direct, concise and actionable. No filler.\n\
             2. Use tools to get real information. Never invent file contents, command output, paths or results.\n\
             3. For multi-step work, write a plan with `todo` first and keep it updated (exactly one item in_progress).\n\
             4. Read before editing. Prefer `edit_file` for targeted changes; use `search_files`/`find_files` instead of guessing paths.\n\
             5. Verify your work (build, test, run) after changes. If something fails, read the error and fix the cause; never repeat an identical failing call.\n\
             6. Independent read-only lookups can be issued together in one step; they run in parallel.\n\
             7. Save durable facts (user preferences, environment details, project conventions) with `obsidian_append`. Never store secrets.\n\
             8. After solving a non-trivial, reusable workflow, save it with `skill_create`.\n\
             9. The Security Guard blocks destructive commands and the user may deny actions. Never try to bypass it; adapt your plan instead.\n\
             10. Keep each `write_file` under ~200 lines; continue large files with `append_file`.\n\
             11. For big self-contained research or analysis subtasks you may use `delegate_task`.\n\
             12. When the task is complete, stop calling tools and answer with a short summary of what you did and any next steps.\n",
            ver = env!("CARGO_PKG_VERSION"),
            date = now.format("%Y-%m-%d (%A)"),
            os = std::env::consts::OS,
            user = std::env::var("USER").unwrap_or_else(|_| "user".into()),
            vault = self.memory.vault_path.display(),
            model = self.config.model,
            provider = self.config.provider,
            mode = self.approval.mode,
        )
    }

    fn compose_system(&self) -> String {
        let mut s = self.system_base.clone();
        if let Some(name) = &self.active_skill {
            if let Some(skill) = self
                .skills
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(name)
            {
                s.push_str(&format!(
                    "\n### Active Skill: {} (follow this procedure)\n{}\n",
                    skill.name, skill.instructions
                ));
            }
        }
        if let Some(sum) = &self.summary {
            s.push_str(&format!(
                "\n### Summary of Earlier Conversation (older turns were compacted)\n{sum}\n"
            ));
        }
        s
    }

    /// Rebuild the frozen system prompt (new session, model/vault change).
    pub fn reset_system_prompt(&mut self) {
        self.system_base = self.build_system_base();
        self.refresh_system();
    }

    fn refresh_system(&mut self) {
        let sys = Message::system(self.compose_system());
        match self.history.first_mut() {
            Some(m) if m.role == Role::System => *m = sys,
            _ => self.history.insert(0, sys),
        }
    }

    pub fn tool_specs(&self, depth: u8) -> Vec<ToolSpec> {
        tools::specs(&tools::SpecOptions {
            allow_web: self.config.allow_web,
            allow_delegate: depth == 0,
            save_sessions: self.config.save_sessions,
        })
    }

    // -----------------------------------------------------------------------
    // Session management
    // -----------------------------------------------------------------------

    pub fn new_session(&mut self) {
        self.save_session();
        self.meta = SessionMeta::new(&self.config.model);
        self.checkpoints = Checkpoints::new(
            self.config
                .state_dir()
                .join("checkpoints")
                .join(&self.meta.id),
        );
        self.history.clear();
        self.summary = None;
        self.todos.lock().unwrap_or_else(|e| e.into_inner()).clear();
        self.stats = Stats::default();
        self.reset_system_prompt();
    }

    pub fn resume(&mut self, key: &str) -> Result<usize, String> {
        let (meta, msgs) = self.store.load(key)?;
        self.save_session();
        self.meta = meta;
        self.checkpoints = Checkpoints::new(
            self.config
                .state_dir()
                .join("checkpoints")
                .join(&self.meta.id),
        );
        self.summary = self.meta.summary.clone();
        self.history = msgs;
        self.history.retain(|m| m.role != Role::System);
        context::sanitize(&mut self.history);
        self.reset_system_prompt();
        self.stats = Stats {
            tokens_in: self.meta.tokens_in,
            tokens_out: self.meta.tokens_out,
            ..Stats::default()
        };
        Ok(self.history.len() - 1)
    }

    pub fn save_session(&mut self) {
        if let Err(e) = self.store.save(&mut self.meta, &self.history) {
            tracing::warn!(error = %e, "failed to save session");
        }
    }

    /// Remove the last `n` user turns. Returns the most recent removed prompt.
    pub fn undo(&mut self, n: usize) -> Option<String> {
        let mut removed = None;
        for _ in 0..n.max(1) {
            let idx = self.history.iter().rposition(|m| m.role == Role::User)?;
            removed = Some(self.history[idx].content.clone());
            self.history.truncate(idx);
        }
        self.save_session();
        removed
    }

    pub fn conversation_tokens(&self) -> usize {
        context::estimate(&self.history)
    }

    // -----------------------------------------------------------------------
    // Turn execution
    // -----------------------------------------------------------------------

    pub async fn run_turn(&mut self, input: &str) -> Result<TurnOutcome, String> {
        if self.config.model.trim().is_empty() {
            if let Some(m) = self.client.auto_detect_model().await {
                self.config.model = m;
                self.meta.model = self.config.model.clone();
                self.reset_system_prompt();
            } else {
                return Err("no model configured and none detected. Pull one (e.g. `ollama pull qwen2.5-coder:7b`) or set `model` / use /model <name>.".into());
            }
        }
        self.interrupt.reset();
        self.turn_counter += 1;
        let started = Instant::now();
        let (in0, out0, tools0) = (
            self.stats.tokens_in,
            self.stats.tokens_out,
            self.stats.tool_calls,
        );

        context::sanitize(&mut self.history);
        self.refresh_system();
        self.history.push(Message::user(input));

        let mut history = std::mem::take(&mut self.history);
        let opts = LoopOpts {
            max_steps: self.config.max_react_steps as usize,
            depth: 0,
        };
        let result = self.react(&mut history, opts).await;
        self.history = history;
        self.stats.turns += 1;

        let (text, steps) = match result {
            Ok(v) => v,
            Err(e) => {
                context::sanitize(&mut self.history);
                if self.interrupt.is_set() {
                    self.history
                        .push(Message::assistant("[turn interrupted by user]"));
                }
                self.save_session();
                return Err(e);
            }
        };
        self.save_session();
        let tool_list: Vec<String> = self
            .history
            .iter()
            .rev()
            .take_while(|m| m.role != Role::User)
            .flat_map(|m| m.tool_calls.iter().map(|t| t.name.clone()))
            .collect();
        let log = format!(
            "**User:** {}\n\n**Agent:**\n{}{}",
            input.trim(),
            text.trim(),
            if tool_list.is_empty() {
                String::new()
            } else {
                format!(
                    "\n\n_Tools: {}_",
                    tool_list.into_iter().rev().collect::<Vec<_>>().join(", ")
                )
            }
        );
        if let Err(e) = self
            .memory
            .append_daily_log(&crate::security::redact_secrets(&log))
        {
            tracing::warn!(error = %e, "daily log failed");
        }
        Ok(TurnOutcome {
            text,
            steps,
            tool_calls: (self.stats.tool_calls - tools0) as usize,
            elapsed: started.elapsed(),
            tokens_in: self.stats.tokens_in - in0,
            tokens_out: self.stats.tokens_out - out0,
        })
    }

    fn step_label(step: usize) -> &'static str {
        match step {
            1 => "Analyzing prompt & planning...",
            2 => "Processing tool results & deciding next action...",
            3 => "Executing follow-up operation...",
            _ => "Synthesizing output & evaluating completion...",
        }
    }

    /// Messages actually sent: when the engine lacks native tool support the
    /// tool protocol is described in the system prompt instead.
    fn prepare_messages(
        &self,
        history: &[Message],
        specs: &[ToolSpec],
        with_tools: bool,
    ) -> Vec<Message> {
        let mut msgs = history.to_vec();
        if with_tools && !self.client.native_tools() {
            if let Some(sys) = msgs.first_mut().filter(|m| m.role == Role::System) {
                let schemas: Vec<Value> = specs.iter().map(|s| json!({"name": s.name, "description": s.description, "parameters": s.parameters})).collect();
                sys.content.push_str(&format!(
                    "\n### Tool Calling Protocol\nTo call a tool, output exactly:\n<tool_call>\n{{\"name\": \"<tool name>\", \"arguments\": {{...}}}}\n</tool_call>\n\
                     You may output several <tool_call> blocks, then stop and wait: results arrive as <tool_response>. \
                     When no tool is needed, answer normally without any <tool_call>.\nAvailable tools:\n{}\n",
                    schemas.iter().map(|s| s.to_string()).collect::<Vec<_>>().join("\n")
                ));
            }
        }
        msgs
    }

    async fn call_model(
        &mut self,
        history: &[Message],
        specs: &[ToolSpec],
        with_tools: bool,
        label: String,
    ) -> Result<(crate::provider::ChatResponse, StreamPrinter), ProviderError> {
        let msgs = self.prepare_messages(history, specs, with_tools);
        let opts = ChatOptions {
            temperature: self.config.temperature,
            max_tokens: self.output_reserve(),
            num_ctx: self.num_ctx,
            tools: if with_tools { Some(specs) } else { None },
            stream: self.config.stream,
        };
        let spinner = Spinner::start(label);
        let mut printer = StreamPrinter::new(spinner, self.config.show_reasoning);
        let res = {
            let mut on_delta = |d: Delta<'_>| match d {
                Delta::Content(t) => printer.on_content(t),
                Delta::Reasoning(t) => printer.on_reasoning(t),
                Delta::ToolCall(n) => printer.on_tool_call(n),
            };
            self.client
                .chat(
                    &self.config.model,
                    &msgs,
                    &opts,
                    &self.interrupt,
                    &mut on_delta,
                )
                .await
        };
        printer.finish();
        let r = res?;
        if let Some(u) = r.usage {
            self.stats.tokens_in += u.prompt_tokens;
            self.stats.tokens_out += u.completion_tokens;
            self.stats.last_prompt_tokens = u.prompt_tokens;
            self.meta.tokens_in += u.prompt_tokens;
            self.meta.tokens_out += u.completion_tokens;
        }
        Ok((r, printer))
    }

    async fn react(
        &mut self,
        history: &mut Vec<Message>,
        opts: LoopOpts,
    ) -> Result<(String, usize), String> {
        let specs = self.tool_specs(opts.depth);
        let mut max_steps = opts.max_steps;
        let mut step = 0usize;
        let mut nudges = 0;
        let mut recoveries = 0;
        let mut auto_extended = false;
        let mut overflow_retry = false;
        let mut signatures: HashMap<String, usize> = HashMap::new();

        loop {
            if self.interrupt.is_set() {
                return Err("interrupted".into());
            }
            step += 1;
            if step > max_steps {
                if self.interactive && opts.depth == 0 {
                    match ui::budget_prompt(step - 1, max_steps) {
                        ui::BudgetChoice::Extend => {
                            max_steps += 15;
                            ui::info(&format!(
                                "Extended step budget by +15 (new limit: {max_steps})."
                            ));
                        }
                        ui::BudgetChoice::Synthesize => {
                            return self.synthesize(history, step).await
                        }
                        ui::BudgetChoice::Stop => {
                            let msg = "Turn halted by user at the step limit.".to_string();
                            history.push(Message::assistant(&msg));
                            return Ok((msg, step - 1));
                        }
                    }
                } else if !auto_extended && opts.depth == 0 {
                    auto_extended = true;
                    max_steps += 15;
                    ui::note(&format!("[step budget auto-extended to {max_steps}]"));
                } else {
                    ui::note("[maximum step budget reached; synthesizing final response]");
                    return self.synthesize(history, step).await;
                }
            }

            self.maybe_compact(history, opts.depth, false).await;
            context::sanitize(history);

            if max_steps > 6 && step == max_steps - 2 {
                if let Some(last) = history.last_mut().filter(|m| m.role == Role::Tool) {
                    last.content.push_str(&format!(
                        "\n\n[SYSTEM NOTE] Step budget nearly exhausted (step {step} of {max_steps}). Finish the remaining essential actions and give your final answer."
                    ));
                }
            }

            let label = format!("[Step {step}/{max_steps}] {}", Self::step_label(step));
            let (resp, printer) = match self.call_model(history, &specs, true, label).await {
                Ok(v) => v,
                Err(ProviderError::Interrupted) => return Err("interrupted".into()),
                Err(e) if e.truncated_tool_json() && recoveries < 2 => {
                    recoveries += 1;
                    ui::render_tool_error(
                        "engine_truncation",
                        "Tool-call arguments exceeded the engine's output limit (truncated JSON). Engaging self-healing recovery...",
                    );
                    history.push(Message::user(
                        "[SYSTEM NOTE] Your previous tool call was cut off by the inference engine's output limit (truncated JSON). \
                         Retry with smaller pieces: write at most ~150 lines per write_file call and continue with append_file.",
                    ));
                    continue;
                }
                Err(e) if e.context_overflow() && !overflow_retry => {
                    overflow_retry = true;
                    ui::warn(
                        "Engine reported a context overflow; compacting history and retrying.",
                    );
                    self.maybe_compact(history, opts.depth, true).await;
                    step -= 1;
                    continue;
                }
                Err(e) => return Err(e.to_string()),
            };
            let (_, mut clean) = ui::extract_thought_and_response(&resp.content);
            let mut calls = resp.tool_calls.clone();
            if calls.is_empty() {
                let known: Vec<&str> = specs.iter().map(|s| s.name).collect();
                let (parsed, rest) = parse_text_tool_calls(&clean, &known);
                if !parsed.is_empty() {
                    calls = parsed;
                    clean = rest;
                }
            }

            if calls.is_empty() {
                // Reasoning models sometimes put the whole answer in their
                // thinking channel and leave the visible reply empty.
                let reasoning = if resp.reasoning.trim().is_empty() {
                    printer.think_text.trim().to_string()
                } else {
                    resp.reasoning.trim().to_string()
                };
                if clean.is_empty() && nudges < 2 {
                    nudges += 1;
                    history.push(Message::user(if reasoning.is_empty() {
                        "[SYSTEM NOTE] Your last reply was empty. Continue the task: call the next tool, or give your final answer based on the results above."
                    } else {
                        "[SYSTEM NOTE] You only produced internal reasoning. Now write the final answer for the user as normal visible text (or call the next tool if work remains)."
                    }));
                    continue;
                }
                if resp.finish_reason.as_deref() == Some("length") {
                    clean.push_str("\n\n[response truncated by the max_tokens limit]");
                }
                let text = if !clean.is_empty() {
                    clean
                } else if !reasoning.is_empty() {
                    reasoning
                } else {
                    "Task completed.".to_string()
                };
                history.push(Message::assistant(&text));
                return Ok((text, step));
            }

            let mut assistant = Message::assistant(clean);
            assistant.tool_calls = calls.clone();
            history.push(assistant);
            self.execute_tool_calls(history, &calls, &mut signatures, opts)
                .await;
            if opts.depth == 0 {
                self.store.save(&mut self.meta, history).ok();
            }
        }
    }

    async fn synthesize(
        &mut self,
        history: &mut Vec<Message>,
        step: usize,
    ) -> Result<(String, usize), String> {
        context::sanitize(history);
        history.push(Message::user(
            "Step budget reached. Stop using tools. Synthesize everything gathered so far into your final, complete answer now, \
             including what remains unfinished.",
        ));
        let (resp, _) = self
            .call_model(history, &[], false, "Synthesizing final response...".into())
            .await
            .map_err(|e| e.to_string())?;
        let (_, clean) = ui::extract_thought_and_response(&resp.content);
        let text = if clean.is_empty() {
            "Task concluded after reaching the step budget.".to_string()
        } else {
            clean
        };
        history.push(Message::assistant(&text));
        Ok((text, step))
    }

    async fn execute_tool_calls(
        &mut self,
        history: &mut Vec<Message>,
        calls: &[ToolCall],
        signatures: &mut HashMap<String, usize>,
        opts: LoopOpts,
    ) {
        let parallel = self.config.parallel_tools
            && calls.len() > 1
            && calls
                .iter()
                .all(|c| class_of(&c.name) == ToolClass::ReadOnly && c.name != "delegate_task");
        let mut parsed: Vec<(ToolCall, Result<Value, String>, usize)> = Vec::new();
        for c in calls {
            let args = tools::parse_args(&c.arguments);
            let sig = format!(
                "{}:{}",
                c.name,
                args.as_ref()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|_| c.arguments.clone())
            );
            let n = signatures.entry(sig).or_insert(0);
            *n += 1;
            parsed.push((c.clone(), args, *n));
        }
        if !ui::quiet() && calls.len() > 1 {
            ui::note(&format!(
                "[ACTION] Executing {} tool calls{}",
                calls.len(),
                if parallel { " in parallel" } else { "" }
            ));
        }

        let mut outputs: Vec<Option<String>> = vec![None; parsed.len()];
        // Pre-flight: argument errors, repetition guard, security and approval.
        let mut runnable: Vec<usize> = Vec::new();
        for (i, (call, args, repeats)) in parsed.iter().enumerate() {
            let args = match args {
                Ok(a) => a,
                Err(e) => {
                    ui::render_tool_error(&call.name, e);
                    outputs[i] = Some(format!("ERROR: {e}"));
                    continue;
                }
            };
            if *repeats >= 3 {
                let msg = format!("[BLOCKED] identical '{}' call issued {} times this turn. Do not repeat it; use the earlier result or change approach.", call.name, repeats);
                ui::render_tool_error(&call.name, &msg);
                outputs[i] = Some(msg);
                continue;
            }
            let mut risk_note: Option<String> = None;
            if class_of(&call.name) == ToolClass::Mutating {
                if call.name == "execute_command" {
                    let cmd =
                        tools::arg_str(args, &["command", "cmd", "script"]).unwrap_or_default();
                    match self.security.classify_command(&cmd) {
                        CommandRisk::Deny(reason) => {
                            ui::render_tool_call(&call.name, args, None);
                            let msg = format!("[SECURITY BLOCKED] {reason}. This command is never allowed; use a safer alternative.");
                            ui::render_tool_error(&call.name, &msg);
                            outputs[i] = Some(msg);
                            continue;
                        }
                        CommandRisk::Ask(reason) => risk_note = Some(reason),
                        CommandRisk::Allow => {}
                    }
                }
                let interactive = self.interactive;
                // The permission review card already shows the call details.
                if !parallel
                    && !self
                        .approval
                        .will_prompt(&call.name, risk_note.is_some(), interactive)
                {
                    ui::render_tool_call(
                        &call.name,
                        args,
                        risk_note
                            .as_deref()
                            .map(|r| format!("[DANGEROUS] {r}"))
                            .as_deref(),
                    );
                }
                if let Decision::Denied(reason) =
                    self.approval
                        .check(&call.name, args, risk_note.as_deref(), interactive)
                {
                    ui::render_tool_error(&call.name, &format!("DENIED: {reason}"));
                    outputs[i] = Some(format!("DENIED: {reason}"));
                    continue;
                }
            } else if !parallel {
                ui::render_tool_call(&call.name, args, None);
            }
            runnable.push(i);
        }

        if parallel && !runnable.is_empty() {
            for &i in &runnable {
                if let Ok(a) = &parsed[i].1 {
                    ui::render_tool_call(&parsed[i].0.name, a, None);
                }
            }
            let ctx = self.tool_ctx();
            let futs = runnable.iter().map(|&i| {
                let (call, args, _) = &parsed[i];
                let args = args.as_ref().cloned().unwrap_or_default();
                let ctx = &ctx;
                async move {
                    let t = Instant::now();
                    let r = tools::execute(&call.name, &args, ctx).await;
                    (i, r, t.elapsed())
                }
            });
            let results = futures_util::future::join_all(futs).await;
            for (i, r, dt) in results {
                outputs[i] = Some(self.finish_tool_output(&parsed[i].0.name, r, dt, parsed[i].2));
            }
        } else {
            for &i in &runnable {
                if self.interrupt.is_set() {
                    break;
                }
                let (call, args, repeats) = &parsed[i];
                let args = args.as_ref().cloned().unwrap_or_default();
                let t = Instant::now();
                let r = if call.name == "delegate_task" {
                    Box::pin(self.run_subagent(&args, opts.depth)).await
                } else {
                    let ctx = self.tool_ctx();
                    tools::execute(&call.name, &args, &ctx).await
                };
                outputs[i] = Some(self.finish_tool_output(&call.name, r, t.elapsed(), *repeats));
            }
        }

        for (i, (call, _, _)) in parsed.iter().enumerate() {
            let content = outputs[i]
                .take()
                .unwrap_or_else(|| "[not executed: turn interrupted]".into());
            history.push(Message::tool(&call.id, &call.name, content));
        }
        self.stats.tool_calls += calls.len() as u64;
    }

    fn finish_tool_output(
        &self,
        name: &str,
        r: Result<String, String>,
        dt: Duration,
        repeats: usize,
    ) -> String {
        let mut out = match r {
            Ok(out) => {
                ui::render_tool_result(
                    name,
                    &crate::security::redact_secrets(&out),
                    dt.as_millis(),
                );
                smart_clip(&out, self.config.max_tool_output_chars)
            }
            Err(e) => {
                ui::render_tool_error(name, &e);
                format!("ERROR: {e}")
            }
        };
        if repeats == 2 {
            out.push_str("\n\n[SYSTEM NOTE] You already made this exact call earlier in this turn. Do not repeat it again; move on to the next distinct step or conclude.");
        }
        out
    }

    fn tool_ctx(&self) -> ToolCtx<'_> {
        ToolCtx {
            security: &self.security,
            memory: &self.memory,
            skills: &self.skills,
            todos: &self.todos,
            checkpoints: &self.checkpoints,
            http: &self.http,
            sessions_dir: self.store.dir.clone(),
            turn: self.turn_counter,
            command_timeout_secs: self.config.command_timeout_secs,
            max_output_chars: self.config.max_tool_output_chars,
            interrupt: &self.interrupt,
        }
    }

    async fn run_subagent(&mut self, args: &Value, depth: u8) -> Result<String, String> {
        if depth > 0 {
            return Err("sub-agents cannot delegate further".into());
        }
        let task = tools::req_str(args, &["task", "prompt", "goal"])?;
        let extra = tools::arg_str(args, &["context", "details"]).unwrap_or_default();
        ui::subagent_banner(&task, true);
        let system = format!(
            "You are a focused sub-agent of QSL Agent. Complete the delegated task using tools, then reply with a concise, \
             factual report (findings, file paths, commands and results). Do not ask questions; make reasonable assumptions \
             and state them.\nWorking directory: {}\nDate: {}",
            std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default(),
            chrono::Local::now().format("%Y-%m-%d")
        );
        let mut hist = vec![
            Message::system(system),
            Message::user(if extra.is_empty() {
                task.clone()
            } else {
                format!("{task}\n\nContext:\n{extra}")
            }),
        ];
        let r = Box::pin(self.react(
            &mut hist,
            LoopOpts {
                max_steps: SUBAGENT_MAX_STEPS,
                depth: depth + 1,
            },
        ))
        .await;
        ui::subagent_banner("", false);
        r.map(|(text, steps)| format!("[sub-agent report after {steps} step(s)]\n{text}"))
    }

    // -----------------------------------------------------------------------
    // Context compaction
    // -----------------------------------------------------------------------

    /// Tokens kept free for the model's reply (`num_predict` / `max_tokens`).
    fn output_reserve(&self) -> u32 {
        self.config.max_tokens.min(self.num_ctx / 4).max(512)
    }

    fn context_budget(&self) -> usize {
        (self.num_ctx as usize)
            .saturating_sub(self.output_reserve() as usize)
            .max(1024)
    }

    fn tools_overhead(&self) -> usize {
        self.tool_specs(0)
            .iter()
            .map(|s| (s.description.len() + s.parameters.to_string().len()) / 4 + 10)
            .sum()
    }

    /// Summarise older turns when the conversation nears the context limit.
    async fn maybe_compact(&mut self, history: &mut Vec<Message>, depth: u8, force: bool) {
        // Fixed overhead (system prompt + tool schemas) is not compactable, so
        // thresholds apply to the space left for the conversation itself.
        let budget = self.context_budget();
        let overhead = self.tools_overhead() + history.first().map_or(0, Message::estimated_tokens);
        let available = budget.saturating_sub(overhead).max(512);
        let conversation = context::estimate(history.get(1..).unwrap_or(&[]));
        let used = conversation + overhead;
        let limit = (available as f32 * self.config.compaction_threshold) as usize;
        if conversation <= limit && !force {
            return;
        }
        if depth > 0 {
            context::clip_tool_outputs(history, 2000, 2);
            return;
        }
        // Keep ~40% of the available space verbatim: enough recent context,
        // with headroom before the next compaction (hysteresis).
        let keep_tail = (available as f32 * 0.40) as usize;
        // Only completed turns are summarised; the current request and its
        // tool traffic stay verbatim (old tool output is clipped instead).
        let current_turn = history
            .iter()
            .rposition(|m| m.role == Role::User)
            .unwrap_or(history.len());
        let split = context::compaction_split(&history[..current_turn.max(1)], keep_tail)
            .or_else(|| (force && current_turn > 1).then_some(current_turn));
        let Some(split) = split else {
            let saved = context::clip_tool_outputs(history, 1500, 2);
            if saved > 0 {
                ui::note(&format!(
                    "[context] clipped {} of old tool output to stay within the window",
                    fmt_tokens(saved / 4)
                ));
            }
            return;
        };
        let before = used;
        let middle: Vec<Message> = history[1..split].to_vec();
        let mut transcript = String::new();
        if let Some(prev) = &self.summary {
            transcript.push_str(&format!("[previous summary]\n{prev}\n\n"));
        }
        transcript.push_str(&context::transcript_for_summary(&middle));
        let transcript = smart_clip(&transcript, (budget * 3).max(8000));

        ui::note(&format!(
            "[context] window {}% full; summarising {} older messages...",
            used * 100 / budget.max(1),
            middle.len()
        ));
        let req = vec![
            Message::system(context::SUMMARY_PROMPT),
            Message::user(transcript),
        ];
        let saved_stream = self.config.stream;
        let saved_reasoning = self.config.show_reasoning;
        self.config.stream = false;
        self.config.show_reasoning = false;
        let was_quiet = ui::quiet();
        ui::set_quiet(true);
        let summary = self
            .call_model(&req, &[], false, "Compacting context...".into())
            .await;
        ui::set_quiet(was_quiet);
        self.config.stream = saved_stream;
        self.config.show_reasoning = saved_reasoning;

        let mut summary_text = match summary {
            Ok((r, _)) => {
                let (_, clean) = ui::extract_thought_and_response(&r.content);
                if clean.trim().is_empty() {
                    fallback_summary(&middle)
                } else {
                    clean
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "summarisation failed; using extractive fallback");
                fallback_summary(&middle)
            }
        };
        if !history[split..].iter().any(|m| m.role == Role::User) {
            if let Some(u) = middle.iter().rev().find(|m| m.role == Role::User) {
                summary_text.push_str(&format!(
                    "\n\nCurrent user request (verbatim): {}",
                    smart_clip(&u.content, 3000)
                ));
            }
        }
        history.drain(1..split);
        self.summary = Some(truncate_chars(&summary_text, 6000));
        self.meta.summary = self.summary.clone();
        let sys = self.compose_system();
        if let Some(first) = history.first_mut() {
            first.content = sys;
        }
        self.stats.compactions += 1;
        let after = context::estimate(history) + self.tools_overhead();
        ui::note(&format!(
            "[context] compacted {} -> {} tokens ({} older messages summarised)",
            fmt_tokens(before),
            fmt_tokens(after),
            middle.len()
        ));
    }

    pub async fn compact_now(&mut self) -> String {
        let mut history = std::mem::take(&mut self.history);
        let before = context::estimate(&history);
        self.maybe_compact(&mut history, 0, true).await;
        self.history = history;
        self.save_session();
        format!(
            "{} -> {} tokens",
            fmt_tokens(before),
            fmt_tokens(self.conversation_tokens())
        )
    }

    // -----------------------------------------------------------------------
    // Runtime reconfiguration
    // -----------------------------------------------------------------------

    pub fn persist_config(&self) {
        if let Err(e) = self.config.save(&self.config_path) {
            ui::warn(&format!("could not save config: {e}"));
        }
    }

    pub async fn set_model(&mut self, model: &str) {
        self.config.model = model.to_string();
        self.meta.model = model.to_string();
        self.client = LlmClient::new(&self.config);
        self.detect_context().await;
        self.reset_system_prompt();
        self.persist_config();
    }

    pub async fn rebuild_client(&mut self) {
        self.client = LlmClient::new(&self.config);
        self.detect_context().await;
        self.reset_system_prompt();
    }

    pub fn set_vault(&mut self, path: &Path) {
        self.config.obsidian_vault_path = path.to_path_buf();
        self.memory = ObsidianMemory::new(path);
        self.skills
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set_vault_dir(&self.memory.skills_dir());
        self.reset_system_prompt();
        self.persist_config();
    }

    pub fn status_parts(&self, o: &TurnOutcome) -> Vec<String> {
        let ctx_used = if self.stats.last_prompt_tokens > 0 {
            self.stats.last_prompt_tokens as usize
        } else {
            self.conversation_tokens()
        };
        let pct = ctx_used as f64 * 100.0 / self.num_ctx.max(1) as f64;
        let mut parts = vec![
            format!("{:.1}s", o.elapsed.as_secs_f64()),
            format!("{} step{}", o.steps, if o.steps == 1 { "" } else { "s" }),
            format!(
                "{} tool{}",
                o.tool_calls,
                if o.tool_calls == 1 { "" } else { "s" }
            ),
            format!(
                "ctx {}/{} {:.0}%",
                fmt_tokens(ctx_used),
                fmt_tokens(self.num_ctx as usize),
                pct
            ),
        ];
        if o.tokens_in + o.tokens_out > 0 {
            let tps = if o.elapsed.as_secs_f64() > 0.0 {
                o.tokens_out as f64 / o.elapsed.as_secs_f64()
            } else {
                0.0
            };
            parts.push(format!(
                "in {} out {} ({:.0} tok/s)",
                fmt_tokens(o.tokens_in as usize),
                fmt_tokens(o.tokens_out as usize),
                tps
            ));
        }
        parts
    }
}

/// Deterministic summary used when the model cannot summarise.
fn fallback_summary(messages: &[Message]) -> String {
    let mut s = String::from("(extractive summary)\n");
    for m in messages {
        match m.role {
            Role::User => s.push_str(&format!(
                "- User asked: {}\n",
                truncate_chars(m.content.trim(), 300)
            )),
            Role::Assistant => {
                for tc in &m.tool_calls {
                    s.push_str(&format!(
                        "- Called {} {}\n",
                        tc.name,
                        truncate_chars(&tc.arguments, 160)
                    ));
                }
                if !m.content.trim().is_empty() && m.tool_calls.is_empty() {
                    s.push_str(&format!(
                        "- Agent answered: {}\n",
                        truncate_chars(m.content.trim(), 300)
                    ));
                }
            }
            Role::Tool if m.content.starts_with("ERROR") || m.content.starts_with("DENIED") => {
                s.push_str(&format!(
                    "- {} failed: {}\n",
                    m.name.clone().unwrap_or_default(),
                    truncate_chars(&m.content, 200)
                ));
            }
            _ => {}
        }
    }
    truncate_chars(&s, 5000)
}

/// Extract tool calls written as text (models without native tool calling,
/// or models that "print" their call): `<tool_call>{json}</tool_call>`,
/// `<function=name>{json}</function>`, or a bare / fenced JSON object whose
/// `name` is a known tool. Returns the calls and the remaining text.
pub fn parse_text_tool_calls(content: &str, known: &[&str]) -> (Vec<ToolCall>, String) {
    static TAGGED: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)<tool_call>\s*(.*?)\s*(?:</tool_call>|$)").expect("re"));
    static FUNC: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?s)<function=([\w\-.]+)>\s*(.*?)\s*</function>").expect("re")
    });
    static FENCED: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)```(?:json|tool_call)?\s*(\{.*?\})\s*```").expect("re"));

    let mut calls = Vec::new();
    let to_call = |name: &str, args: Option<&Value>| -> Option<ToolCall> {
        let name = name.trim();
        if !known.contains(&name) {
            return None;
        }
        let arguments = match args {
            Some(Value::String(s)) => s.clone(),
            Some(v) if !v.is_null() => v.to_string(),
            _ => "{}".into(),
        };
        Some(ToolCall {
            id: format!("call_{}", crate::util::short_id()),
            name: name.to_string(),
            arguments,
        })
    };
    let from_json = |raw: &str| -> Vec<ToolCall> {
        let Ok(v) = tools::parse_args(raw) else {
            // parse_args only yields objects; arrays of calls are handled here
            return serde_json::from_str::<Vec<Value>>(raw)
                .unwrap_or_default()
                .iter()
                .filter_map(|o| {
                    to_call(
                        o["name"].as_str().unwrap_or(""),
                        o.get("arguments").or(o.get("parameters")),
                    )
                })
                .collect();
        };
        let name = v["name"]
            .as_str()
            .or(v["function"]["name"].as_str())
            .or(v["tool"].as_str())
            .unwrap_or("");
        let args = v
            .get("arguments")
            .or(v.get("parameters"))
            .or(v["function"].get("arguments"));
        to_call(name, args).into_iter().collect()
    };

    let mut rest = content.to_string();
    if TAGGED.is_match(content) {
        for cap in TAGGED.captures_iter(content) {
            calls.extend(from_json(&cap[1]));
        }
        rest = TAGGED.replace_all(content, "").to_string();
    } else if FUNC.is_match(content) {
        for cap in FUNC.captures_iter(content) {
            let args = tools::parse_args(&cap[2]).ok();
            calls.extend(to_call(&cap[1], args.as_ref()));
        }
        rest = FUNC.replace_all(content, "").to_string();
    } else {
        for cap in FENCED.captures_iter(content) {
            let found = from_json(&cap[1]);
            if !found.is_empty() {
                calls.extend(found);
                rest = rest.replace(&cap[0], "");
            }
        }
        let trimmed = content.trim();
        if calls.is_empty() && trimmed.starts_with('{') && trimmed.ends_with('}') {
            let found = from_json(trimmed);
            if !found.is_empty() {
                calls.extend(found);
                rest.clear();
            }
        }
    }
    (calls, rest.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KNOWN: &[&str] = &["read_file", "list_dir", "write_file"];

    #[test]
    fn parses_hermes_tool_call_blocks() {
        let (calls, rest) = parse_text_tool_calls(
            "Let me check.\n<tool_call>\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"Cargo.toml\"}}\n</tool_call>\n<tool_call>{\"name\":\"list_dir\",\"arguments\":{}}</tool_call>",
            KNOWN,
        );
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "read_file");
        assert!(calls[0].arguments.contains("Cargo.toml"));
        assert_eq!(rest, "Let me check.");
    }

    #[test]
    fn parses_unclosed_and_llama_formats() {
        let (calls, _) = parse_text_tool_calls(
            "<tool_call>{\"name\": \"list_dir\", \"arguments\": {\"path\": \".\"}}",
            KNOWN,
        );
        assert_eq!(calls.len(), 1);
        let (calls, _) =
            parse_text_tool_calls("<function=read_file>{\"path\": \"a\"}</function>", KNOWN);
        assert_eq!(calls[0].name, "read_file");
    }

    #[test]
    fn parses_bare_and_fenced_json_only_for_known_tools() {
        let (calls, rest) = parse_text_tool_calls(
            "{\"name\": \"list_dir\", \"parameters\": {\"path\": \"src\"}}",
            KNOWN,
        );
        assert_eq!(calls.len(), 1);
        assert!(rest.is_empty());
        let (calls, _) = parse_text_tool_calls("```json\n{\"name\": \"write_file\", \"arguments\": {\"path\": \"x\", \"content\": \"y\"}}\n```", KNOWN);
        assert_eq!(calls[0].name, "write_file");
        let (calls, rest) = parse_text_tool_calls(
            "Here is JSON: ```json\n{\"name\": \"Alice\", \"age\": 3}\n```",
            KNOWN,
        );
        assert!(calls.is_empty());
        assert!(rest.contains("Alice"));
    }

    #[test]
    fn fallback_summary_lists_requests_and_calls() {
        let mut a = Message::assistant("");
        a.tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "read_file".into(),
            arguments: "{\"path\":\"x\"}".into(),
        }];
        let s = fallback_summary(&[
            Message::user("fix the build"),
            a,
            Message::tool("1", "read_file", "ERROR: nope"),
        ]);
        assert!(
            s.contains("fix the build") && s.contains("Called read_file") && s.contains("failed")
        );
    }
}
