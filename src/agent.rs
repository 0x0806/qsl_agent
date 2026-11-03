use std::sync::Arc;
use std::time::Duration;
use futures::stream::{FuturesUnordered, StreamExt};
use serde::Serialize;
use tokio::sync::{mpsc, oneshot, Semaphore};
use tokio_util::sync::CancellationToken;
use tracing::{info, instrument};

use crate::config::AgentConfig;
use crate::context::ContextManager;
use crate::cost::CostTracker;
use crate::error::{AgentError, Result};
use crate::events::{Event, EventLog, ToolCallRec};
use crate::health::HealthMonitor;
use crate::memory::Memory;
use crate::provider::{ChatMessage, ChatRequest, Provider, StreamEvent, ToolCall};
use crate::security::SecurityGuard;
use crate::session::{ExportFormat, Session};
use crate::tools::{smart_clip, ToolContext, ToolOutcome, ToolRegistry};
use crate::turn_budget::TurnBudget;

pub enum AgentRequest {
    Chat { text: String, reply: oneshot::Sender<Result<String>> },
    Health { reply: oneshot::Sender<HealthSnapshot> },
    Stats { reply: oneshot::Sender<SessionStats> },
    Retry { reply: oneshot::Sender<Result<String>> },
    Undo { turns: usize, reply: oneshot::Sender<Result<()>> },
    Compact { reply: oneshot::Sender<Result<()>> },
    Export { format: ExportFormat, reply: oneshot::Sender<Result<String>> },
}

#[derive(Clone, Serialize)]
pub struct HealthSnapshot {
    pub session_id: String,
    pub idle_seconds: u64,
    pub steps: u64,
    pub stalled: bool,
    pub context_pct: f32,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_usd: f64,
}

#[derive(Clone, Serialize)]
pub struct SessionStats {
    pub session_id: String,
    pub turn_idx: u64,
    pub message_count: usize,
    pub context_tokens: usize,
    pub context_pct: f32,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_usd: f64,
    pub tools_available: Vec<&'static str>,
    pub snapshots_available: usize,
}

#[derive(Clone)]
pub struct AgentHandle {
    tx: mpsc::Sender<AgentRequest>,
    health: HealthMonitor,
}

impl AgentHandle {
    pub async fn chat(&self, text: String) -> Result<String> {
        let (tx, rx) = oneshot::channel();
        self.tx.send(AgentRequest::Chat { text, reply: tx }).await
            .map_err(|_| AgentError::Cancelled)?;
        rx.await.map_err(|_| AgentError::Cancelled)?
    }
    pub async fn health(&self) -> Result<HealthSnapshot> {
        let (tx, rx) = oneshot::channel();
        self.tx.send(AgentRequest::Health { reply: tx }).await
            .map_err(|_| AgentError::Cancelled)?;
        Ok(rx.await.map_err(|_| AgentError::Cancelled)?)
    }
    pub async fn stats(&self) -> Result<SessionStats> {
        let (tx, rx) = oneshot::channel();
        self.tx.send(AgentRequest::Stats { reply: tx }).await
            .map_err(|_| AgentError::Cancelled)?;
        Ok(rx.await.map_err(|_| AgentError::Cancelled)?)
    }
    pub fn health_monitor(&self) -> &HealthMonitor { &self.health }
}

pub struct Agent {
    provider: Arc<Provider>,
    tools: ToolRegistry,
    session: Session,
    memory: Arc<Memory>,
    #[allow(dead_code)]
    security: Arc<SecurityGuard>,
    events: Arc<EventLog>,
    cfg: AgentConfig,
    ctx: ContextManager,
    health: HealthMonitor,
    tool_ctx: ToolContext,
    cost: Arc<CostTracker>,
    rx: mpsc::Receiver<AgentRequest>,
    tool_semaphore: Arc<Semaphore>,
}

impl Agent {
    pub fn new(
        provider: Arc<Provider>,
        tools: ToolRegistry,
        session: Session,
        memory: Arc<Memory>,
        security: Arc<SecurityGuard>,
        events: Arc<EventLog>,
        cfg: AgentConfig,
        health: HealthMonitor,
        cost: Arc<CostTracker>,
        tier: crate::config::Tier,
    ) -> (Self, AgentHandle) {
        let (tx, rx) = mpsc::channel(64);
        let ctx = ContextManager::new(tier);
        let tool_ctx = ToolContext::new(
            security.clone(),
            cfg.max_concurrent_tools * 16 * 1024,
            cfg.tool_timeout_s,
        );
        let tool_semaphore = Arc::new(Semaphore::new(cfg.max_concurrent_tools));
        let handle = AgentHandle { tx, health: health.clone() };
        let agent = Self {
            provider, tools, session, memory, security, events, cfg,
            ctx, health, tool_ctx, cost, rx, tool_semaphore,
        };
        (agent, handle)
    }

    pub async fn run(mut self, shutdown: CancellationToken) -> Result<()> {
        use tokio::io::{AsyncBufReadExt, BufReader, stdin};
        let mut stdin = BufReader::new(stdin()).lines();

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => { info!("shutdown"); break; }
                Some(req) = self.rx.recv() => self.handle_request(req).await,
                line = stdin.next_line() => {
                    let Some(line) = line? else { break };
                    let line = line.trim();
                    if line.is_empty() { continue; }
                    if line == "/quit" { break; }
                    match self.handle_line(line).await {
                        Ok(reply) => if !reply.is_empty() { println!("{reply}"); },
                        Err(e) => eprintln!("error: {e}"),
                    }
                }
            }
        }
        self.memory.flush().await.ok();
        Ok(())
    }

    async fn handle_request(&mut self, req: AgentRequest) {
        match req {
            AgentRequest::Chat { text, reply } => {
                let r = self.handle_line(&text).await;
                let _ = reply.send(r);
            }
            AgentRequest::Health { reply } => { let _ = reply.send(self.snapshot_health().await); }
            AgentRequest::Stats { reply } => { let _ = reply.send(self.snapshot_stats().await); }
            AgentRequest::Retry { reply } => {
                let r = self.do_retry().await;
                let _ = reply.send(r);
            }
            AgentRequest::Undo { turns, reply } => {
                let r = self.session.undo(turns).await;
                let _ = reply.send(r);
            }
            AgentRequest::Compact { reply } => {
                let r = self.ctx.compact(
                    self.session.history_mut(), &self.memory, &self.provider).await;
                let _ = reply.send(r);
            }
            AgentRequest::Export { format, reply } => {
                let r = self.session.export(format).await;
                let _ = reply.send(r);
            }
        }
    }

    async fn handle_line(&mut self, line: &str) -> Result<String> {
        if let Some(cmd) = line.strip_prefix('/') {
            let parts: Vec<&str> = cmd.split_whitespace().collect();
            match parts.first().copied() {
                Some("health") => {
                    let h = self.snapshot_health().await;
                    return Ok(format!(
                        "idle={}s steps={} ctx={:.1}% cost=${:.4}",
                        h.idle_seconds, h.steps, h.context_pct, h.cost_usd));
                }
                Some("stats") => {
                    let st = self.snapshot_stats().await;
                    return Ok(serde_json::to_string_pretty(&st).unwrap_or_default());
                }
                Some("compact") => {
                    self.ctx.compact(self.session.history_mut(), &self.memory, &self.provider).await?;
                    return Ok("compacted".into());
                }
                Some("retry") => return self.do_retry().await,
                Some("undo") => {
                    let n = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1usize);
                    self.session.undo(n).await?;
                    return Ok(format!("undid {n} turn(s)"));
                }
                Some("export") => {
                    let fmt = match parts.get(1).copied() {
                        Some("md") | Some("markdown") => ExportFormat::Markdown,
                        _ => ExportFormat::Json,
                    };
                    return self.session.export(fmt).await;
                }
                Some("save") => {
                    let path = parts.get(1).copied().unwrap_or("session.json");
                    let body = self.session.export(ExportFormat::Json).await?;
                    tokio::fs::write(path, body).await?;
                    return Ok(format!("saved to {path}"));
                }
                Some(other) => return Ok(format!("unknown command: /{other}")),
                None => return Ok("empty command".into()),
            }
        }
        self.handle_turn(line.to_string()).await
    }

    async fn do_retry(&mut self) -> Result<String> {
        let Some(user) = self.session.last_user.clone() else {
            return Ok("nothing to retry".into());
        };
        let _ = self.session.snapshots.pop();
        self.session.history.pop();
        self.handle_turn(user).await
    }

    #[instrument(skip(self), fields(session.id = %self.session.id, turn.idx = self.session.turn_idx))]
    async fn handle_turn(&mut self, user_text: String) -> Result<String> {
        let trace_id = uuid::Uuid::new_v4().to_string();
        let budget = TurnBudget::new(Duration::from_secs(self.cfg.max_turn_seconds));
        self.session.append_user(user_text).await?;
        self.health.beat();

        let mut final_text = String::new();
        let turn_start = std::time::Instant::now();

        for step in 0..self.cfg.max_steps_per_turn {
            if budget.expired() {
                return Err(AgentError::TurnDeadline { seconds: self.cfg.max_turn_seconds });
            }
            if self.ctx.needs_compaction(self.session.history()) {
                self.ctx.compact(self.session.history_mut(), &self.memory, &self.provider).await?;
                self.events.append(Event::Compaction {
                    kept_msgs: self.session.history().len(),
                    tokens: ContextManager::tokens(self.session.history()),
                    at: chrono::Utc::now(),
                }).await?;
            }
            self.ctx.assert_well_formed(self.session.history())?;
            self.ctx.assert_fits(self.session.history())?;

            let util = self.ctx.utilization(self.session.history());
            metrics::gauge!("qsl_context_utilization_pct").set(util as f64 * 100.0);
            self.events.append(Event::Heartbeat {
                step, ctx_pct: util * 100.0, at: chrono::Utc::now(),
            }).await?;

            let req = ChatRequest {
                messages: self.session.history().to_vec(),
                tools: self.tools.schemas(),
                temperature: self.cfg.temperature,
                stream: true,
            };

            let (tx, mut rx) = mpsc::channel::<StreamEvent>(64);
            let provider = self.provider.clone();
            let producer = tokio::spawn(async move { provider.chat_stream(req, tx).await });

            let mut assistant_text = String::new();
            let mut tool_calls: Vec<ToolCall> = Vec::new();
            let mut in_flight: FuturesUnordered<_> = FuturesUnordered::new();
            let mut results: Vec<(ToolCall, ToolOutcome)> = Vec::new();
            let sem = self.tool_semaphore.clone();

            loop {
                tokio::select! {
                    Some(ev) = rx.recv() => match ev {
                        StreamEvent::Text(t) => { assistant_text.push_str(&t); }
                        StreamEvent::ToolCall(tc) => {
                            let tool = self.tools.get(&tc.name);
                            let tctx = self.tool_ctx.clone_for();
                            let tc_clone = tc.clone();
                            let sem = sem.clone();
                            tool_calls.push(tc.clone());
                            in_flight.push(async move {
                                let _permit = sem.acquire_owned().await.ok();
                                let out = match tool {
                                    Some(t) => {
                                        if let Err(e) = t.validate(&tc_clone.arguments) {
                                            Err(e)
                                        } else {
                                            let r = t.execute(tc_clone.arguments.clone(), &tctx).await;
                                            r.map(|mut o| {
                                                o.output = smart_clip(&o.output, tctx.max_output);
                                                o
                                            })
                                        }
                                    }
                                    None => Err(AgentError::ToolFailed {
                                        tool: tc_clone.name.clone(),
                                        reason: "unknown tool".into(),
                                    }),
                                };
                                (tc_clone, out)
                            });
                        }
                        StreamEvent::Done { usage_in, usage_out } => {
                            self.cost.record(usage_in, usage_out);
                        }
                    },
                    Some((tc, out)) = in_flight.next() => {
                        let outcome = out.unwrap_or_else(|e| ToolOutcome {
                            output: format!("error: {e}"), ok: false });
                        crate::metrics::tool_executed(&tc.name, outcome.ok, 0.0);
                        results.push((tc, outcome));
                    }
                    else => break,
                }
            }
            producer.await.map_err(|e| AgentError::ToolFailed {
                tool: "provider".into(), reason: e.to_string() })??;
            while let Some((tc, out)) = in_flight.next().await {
                let outcome = out.unwrap_or_else(|e| ToolOutcome {
                    output: format!("error: {e}"), ok: false });
                crate::metrics::tool_executed(&tc.name, outcome.ok, 0.0);
                results.push((tc, outcome));
            }

            let tcrec: Vec<ToolCallRec> = tool_calls.iter().map(|tc| ToolCallRec {
                id: tc.id.clone(), name: tc.name.clone(), args: tc.arguments.clone(),
            }).collect();
            self.session.append_assistant(assistant_text.clone(), tcrec).await?;

            for (tc, out) in &results {
                self.events.append(Event::ToolCall {
                    id: tc.id.clone(), name: tc.name.clone(),
                    args: tc.arguments.clone(), at: chrono::Utc::now(),
                }).await?;
                self.events.append(Event::ToolResult {
                    id: tc.id.clone(), ok: out.ok, output: out.output.clone(),
                    at: chrono::Utc::now(),
                }).await?;
                self.session.history_mut().push(ChatMessage {
                    role: crate::provider::Role::Tool,
                    content: out.output.clone(),
                    tool_calls: None,
                    tool_call_id: Some(tc.id.clone()),
                    tokens: Default::default(),
                });
            }

            crate::context::dedup_tool_results(self.session.history_mut());

            final_text = assistant_text.clone();
            self.session.checkpoint(&format!("step {step} trace {trace_id}")).await?;
            self.health.beat();

            if tool_calls.is_empty() { break; }
        }

        self.session.turn_idx += 1;
        self.session.maybe_rotate().await?;
        let (tin, tout) = self.cost.totals();
        crate::metrics::turn_completed(
            self.session.turn_idx,
            turn_start.elapsed().as_secs_f64() * 1000.0,
            tin as u32, tout as u32);
        Ok(final_text)
    }

    async fn snapshot_health(&self) -> HealthSnapshot {
        let (tin, tout) = self.cost.totals();
        HealthSnapshot {
            session_id: self.session.id.clone(),
            idle_seconds: self.health.idle_seconds(),
            steps: self.health.steps(),
            stalled: self.health.is_stalled(),
            context_pct: self.ctx.utilization(self.session.history()) * 100.0,
            tokens_in: tin,
            tokens_out: tout,
            cost_usd: self.cost.estimated_usd(),
        }
    }

    async fn snapshot_stats(&self) -> SessionStats {
        let (tin, tout) = self.cost.totals();
        SessionStats {
            session_id: self.session.id.clone(),
            turn_idx: self.session.turn_idx,
            message_count: self.session.history().len(),
            context_tokens: ContextManager::tokens(self.session.history()),
            context_pct: self.ctx.utilization(self.session.history()) * 100.0,
            tokens_in: tin,
            tokens_out: tout,
            cost_usd: self.cost.estimated_usd(),
            tools_available: self.tools.names(),
            snapshots_available: self.session.snapshots.depth(),
        }
    }
}
