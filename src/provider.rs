use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;
use futures::StreamExt;
use eventsource_stream::Eventsource;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{instrument, warn};

use crate::config::ProviderConfig;
use crate::error::{AgentError, Result};

static HTTP: std::sync::LazyLock<reqwest::Client> = std::sync::LazyLock::new(|| {
    reqwest::Client::builder()
        .pool_max_idle_per_host(16)
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_nodelay(true)
        .http2_adaptive_window(true)
        .build()
        .expect("reqwest client")
});

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip)]
    pub tokens: std::sync::OnceLock<u32>,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: Role::User, content: content.into(), tool_calls: None, tool_call_id: None, tokens: Default::default() }
    }
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: Role::System, content: content.into(), tool_calls: None, tool_call_id: None, tokens: Default::default() }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: Role::Assistant, content: content.into(), tool_calls: None, tool_call_id: None, tokens: Default::default() }
    }
    pub fn token_count(&self) -> u32 {
        *self.tokens.get_or_init(|| {
            tiktoken_rs::o200k_base()
                .map(|bpe| bpe.encode_with_special_tokens(&self.content).len() as u32)
                .unwrap_or_else(|_| (self.content.len() / 4) as u32)
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role { System, User, Assistant, Tool }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<serde_json::Value>,
    pub temperature: f32,
    pub stream: bool,
}

#[derive(Debug)]
pub enum StreamEvent {
    Text(String),
    ToolCall(ToolCall),
    Done { usage_in: u32, usage_out: u32 },
}

pub struct CircuitBreaker {
    failures: AtomicU32,
    open_until: AtomicU64,
}

impl CircuitBreaker {
    fn new() -> Self {
        Self { failures: AtomicU32::new(0), open_until: AtomicU64::new(0) }
    }
    fn check(&self, provider: &str) -> Result<()> {
        let open_until = self.open_until.load(Ordering::Relaxed);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        if open_until > now {
            return Err(AgentError::ProviderDegraded {
                provider: provider.into(),
                retry_after_s: open_until - now,
            });
        }
        Ok(())
    }
    fn record_success(&self) { self.failures.store(0, Ordering::Relaxed); }
    fn record_failure(&self) {
        let n = self.failures.fetch_add(1, Ordering::Relaxed) + 1;
        if n >= 5 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
            self.open_until.store(now + 60, Ordering::Relaxed);
            self.failures.store(0, Ordering::Relaxed);
            warn!("circuit breaker open for 60s after 5 failures");
            crate::metrics::circuit_open("provider");
        }
    }
}

pub struct Provider {
    cfg: ProviderConfig,
    breaker: CircuitBreaker,
    usage_in:  RwLock<u64>,
    usage_out: RwLock<u64>,
}

impl Provider {
    pub fn new(cfg: &crate::config::Config) -> Result<Self> {
        Ok(Self {
            cfg: cfg.provider.clone(),
            breaker: CircuitBreaker::new(),
            usage_in:  RwLock::new(0),
            usage_out: RwLock::new(0),
        })
    }

    pub async fn total_usage(&self) -> (u64, u64) {
        (*self.usage_in.read().await, *self.usage_out.read().await)
    }

    pub async fn probe(&self) -> Result<()> {
        let req = ChatRequest {
            messages: vec![ChatMessage::user("ping")],
            tools: vec![],
            temperature: 0.0,
            stream: false,
        };
        let _ = self.chat(req).await?;
        Ok(())
    }

    #[instrument(skip(self, req), fields(model = %self.cfg.model))]
    pub async fn chat_stream(
        &self,
        req: ChatRequest,
        tx: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<()> {
        self.breaker.check(&self.cfg.model)?;

        let body = serde_json::json!({
            "model": self.cfg.model,
            "messages": req.messages,
            "tools": if req.tools.is_empty() { serde_json::Value::Null } else { serde_json::json!(req.tools) },
            "temperature": req.temperature,
            "stream": true,
        });

        let url = format!("{}/v1/chat/completions", self.cfg.base_url.trim_end_matches('/'));
        let mut request = HTTP.post(&url).json(&body);
        if let Some(k) = &self.cfg.api_key {
            request = request.bearer_auth(k);
        }

        let resp = request.timeout(Duration::from_secs(self.cfg.request_timeout_s))
            .send().await
            .map_err(|source| {
                self.breaker.record_failure();
                AgentError::LlmUnreachable { provider: self.cfg.model.clone(), source }
            })?;

        if !resp.status().is_success() {
            self.breaker.record_failure();
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(AgentError::LlmStatus {
                provider: self.cfg.model.clone(), status, body,
            });
        }

        self.breaker.record_success();

        let mut stream = resp.bytes_stream().eventsource();
        let mut accum_tool: Option<ToolCall> = None;
        let mut tool_arg_buf = String::new();

        while let Some(evt) = stream.next().await {
            let evt = match evt {
                Ok(e) => e,
                Err(e) => { warn!(?e, "SSE read error"); break; }
            };
            if evt.data == "[DONE]" { break; }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&evt.data) else { continue };

            if let Some(delta) = v.pointer("/choices/0/delta") {
                if let Some(txt) = delta.get("content").and_then(|c| c.as_str()) {
                    if !txt.is_empty() {
                        let _ = tx.send(StreamEvent::Text(txt.to_string())).await;
                    }
                }
                if let Some(calls) = delta.get("tool_calls").and_then(|c| c.as_array()) {
                    for call in calls {
                        let name = call.pointer("/function/name").and_then(|n| n.as_str());
                        if let Some(name) = name {
                            let id = call.get("id").and_then(|i| i.as_str()).unwrap_or("call_0");
                            accum_tool = Some(ToolCall {
                                id: id.to_string(),
                                name: name.to_string(),
                                arguments: serde_json::Value::Null,
                            });
                            tool_arg_buf.clear();
                        }
                        if let Some(frag) = call.pointer("/function/arguments").and_then(|a| a.as_str()) {
                            tool_arg_buf.push_str(frag);
                            if tool_arg_buf.ends_with('}') {
                                if let Some(ref mut tc) = accum_tool {
                                    tc.arguments = serde_json::from_str(&tool_arg_buf)
                                        .unwrap_or(serde_json::Value::Null);
                                    let _ = tx.send(StreamEvent::ToolCall(tc.clone())).await;
                                    accum_tool = None;
                                    tool_arg_buf.clear();
                                }
                            }
                        }
                    }
                }
            }
            if let Some(usage) = v.get("usage") {
                let i = usage.get("prompt_tokens").and_then(|x| x.as_u64()).unwrap_or(0);
                let o = usage.get("completion_tokens").and_then(|x| x.as_u64()).unwrap_or(0);
                *self.usage_in.write().await  += i;
                *self.usage_out.write().await += o;
                let _ = tx.send(StreamEvent::Done {
                    usage_in: i as u32, usage_out: o as u32,
                }).await;
            }
        }
        Ok(())
    }

    pub async fn chat(&self, req: ChatRequest) -> Result<(ChatMessage, Vec<ToolCall>, u32, u32)> {
        use backoff::ExponentialBackoffBuilder;
        let backoff = ExponentialBackoffBuilder::new()
            .with_initial_interval(Duration::from_millis(500))
            .with_max_elapsed_time(Some(Duration::from_secs(30)))
            .with_randomization_factor(0.5)
            .build();

        backoff::future::retry(backoff, || async {
            self.chat_once(&req).await.map_err(|e| {
                if e.is_retryable() { backoff::Error::transient(e) }
                else { backoff::Error::permanent(e) }
            })
        }).await
    }

    async fn chat_once(&self, req: &ChatRequest) -> Result<(ChatMessage, Vec<ToolCall>, u32, u32)> {
        self.breaker.check(&self.cfg.model)?;
        let body = serde_json::json!({
            "model": self.cfg.model,
            "messages": req.messages,
            "tools": if req.tools.is_empty() { serde_json::Value::Null } else { serde_json::json!(req.tools) },
            "temperature": req.temperature,
            "stream": false,
        });
        let url = format!("{}/v1/chat/completions", self.cfg.base_url.trim_end_matches('/'));
        let mut r = HTTP.post(&url).json(&body);
        if let Some(k) = &self.cfg.api_key { r = r.bearer_auth(k); }

        let resp = r.timeout(Duration::from_secs(self.cfg.request_timeout_s))
            .send().await
            .map_err(|source| { self.breaker.record_failure();
                AgentError::LlmUnreachable { provider: self.cfg.model.clone(), source } })?;

        if !resp.status().is_success() {
            self.breaker.record_failure();
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(AgentError::LlmStatus { provider: self.cfg.model.clone(), status, body });
        }
        self.breaker.record_success();

        let v: serde_json::Value = resp.json().await
            .map_err(|e| AgentError::LlmStatus { provider: self.cfg.model.clone(), status: 200, body: e.to_string() })?;

        let msg = v.pointer("/choices/0/message").cloned().unwrap_or(serde_json::Value::Null);
        let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string();
        let tool_calls: Vec<ToolCall> = msg.get("tool_calls").and_then(|t| t.as_array())
            .map(|arr| arr.iter().filter_map(|c| {
                Some(ToolCall {
                    id: c.get("id")?.as_str()?.to_string(),
                    name: c.pointer("/function/name")?.as_str()?.to_string(),
                    arguments: c.pointer("/function/arguments")
                        .and_then(|a| a.as_str())
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or(serde_json::Value::Null),
                })
            }).collect()).unwrap_or_default();

        let usage_in  = v.pointer("/usage/prompt_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
        let usage_out = v.pointer("/usage/completion_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as u32;

        Ok((ChatMessage::assistant(content), tool_calls, usage_in, usage_out))
    }
}
