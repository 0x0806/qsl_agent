//! Local LLM client.
//!
//! Speaks two wire protocols:
//! * OpenAI-compatible `/v1/chat/completions` with SSE streaming (LM Studio,
//!   llama.cpp, vLLM, LocalAI, OpenRouter, Ollama's `/v1` shim, ...).
//! * Ollama native `/api/chat` with NDJSON streaming. This is the default for
//!   the `ollama` provider because it is the only Ollama endpoint that honours
//!   `num_ctx`; via `/v1` every model silently runs with Ollama's small
//!   default context and truncates the system prompt.
//!
//! Tool calls are accumulated per `index` (parallel tool calls work), usage is
//! requested via `stream_options`, and servers or models without native tool
//! support fall back automatically to the Hermes-style `<tool_call>` text
//! protocol.

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::config::{ApiStyle, Config};
use crate::interrupt::Interrupt;
use crate::util::short_id;

// ---------------------------------------------------------------------------
// Message model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Raw JSON arguments as produced by the model (may be invalid JSON).
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    #[serde(default)]
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Tool name for `Tool` messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl Message {
    pub fn system(c: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: c.into(),
            tool_calls: vec![],
            tool_call_id: None,
            name: None,
        }
    }
    pub fn user(c: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: c.into(),
            tool_calls: vec![],
            tool_call_id: None,
            name: None,
        }
    }
    pub fn assistant(c: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: c.into(),
            tool_calls: vec![],
            tool_call_id: None,
            name: None,
        }
    }
    pub fn tool(id: &str, name: &str, c: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: c.into(),
            tool_calls: vec![],
            tool_call_id: Some(id.to_string()),
            name: Some(name.to_string()),
        }
    }

    pub fn estimated_tokens(&self) -> usize {
        let calls: usize = self
            .tool_calls
            .iter()
            .map(|t| t.arguments.len() + t.name.len() + 16)
            .sum();
        (self.content.len() + calls).div_ceil(4) + 4
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: String,
    pub parameters: Value,
}

impl ToolSpec {
    fn to_wire(&self) -> Value {
        json!({
            "type": "function",
            "function": { "name": self.name, "description": self.description, "parameters": self.parameters }
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ChatResponse {
    pub content: String,
    pub reasoning: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Option<Usage>,
    pub finish_reason: Option<String>,
}

/// Incremental output while a response streams in.
pub enum Delta<'a> {
    Content(&'a str),
    Reasoning(&'a str),
    ToolCall(&'a str),
}

pub struct ChatOptions<'a> {
    pub temperature: f32,
    pub max_tokens: u32,
    pub num_ctx: u32,
    pub tools: Option<&'a [ToolSpec]>,
    pub stream: bool,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("cannot reach local AI engine at {url}: {detail}\n  -> Start Ollama: `ollama serve`  |  LM Studio: enable the local server  |  check `qsl-agent --health-check`")]
    Connect { url: String, detail: String },
    #[error("request to {url} timed out: {detail}. Large models can be slow; raise `request_timeout_secs` in config.toml")]
    Timeout { url: String, detail: String },
    #[error("local AI model error [HTTP {status}]: {body}")]
    Http { status: u16, body: String },
    #[error("invalid response from engine: {0}")]
    Parse(String),
    #[error("interrupted")]
    Interrupted,
}

impl ProviderError {
    fn retryable(&self) -> bool {
        match self {
            Self::Connect { .. } | Self::Timeout { .. } => true,
            Self::Http { status, .. } => matches!(status, 408 | 429 | 500 | 502 | 503 | 504),
            _ => false,
        }
    }

    fn mentions(&self, needles: &[&str]) -> bool {
        let text = self.to_string().to_lowercase();
        needles.iter().any(|n| text.contains(n))
    }

    /// The engine rejected tool definitions (model template lacks tool support).
    pub fn tools_unsupported(&self) -> bool {
        matches!(
            self,
            Self::Http {
                status: 400..=500,
                ..
            }
        ) && self.mentions(&[
            "does not support tools",
            "tools is not supported",
            "tool use is not supported",
            "tool_choice",
            "\"tools\"",
        ])
    }

    /// Truncated / malformed tool-call JSON produced server-side.
    pub fn truncated_tool_json(&self) -> bool {
        self.mentions(&[
            "unexpected end of json",
            "invalid tool call arguments",
            "error parsing tool call",
            "failed to parse tool call",
        ])
    }

    pub fn context_overflow(&self) -> bool {
        self.mentions(&[
            "context length",
            "context window",
            "maximum context",
            "too many tokens",
            "exceeds the context",
            "context_length_exceeded",
            "prompt is too long",
            "context size",
            "exceed_context_size",
            "n_ctx",
        ])
    }
}

// ---------------------------------------------------------------------------
// Model tiers (display + hints)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelTier {
    Small,
    Medium,
    Large,
}

impl ModelTier {
    /// Classify by the parameter count embedded in the model name ("qwen2.5:32b").
    pub fn from_model_name(name: &str) -> Self {
        let lower = name.to_lowercase();
        let re =
            regex::Regex::new(r"(?:^|[^0-9.])(\d+(?:\.\d+)?)b(?:[^a-z]|$)").expect("tier regex");
        let params = re
            .captures_iter(&lower)
            .filter_map(|c| c[1].parse::<f64>().ok())
            .fold(None, |acc: Option<f64>, v| {
                Some(acc.map_or(v, |a| a.max(v)))
            });
        match params {
            Some(p) if p >= 35.0 => Self::Large,
            Some(p) if p >= 10.0 => Self::Medium,
            Some(_) => Self::Small,
            None if lower.contains("large") => Self::Large,
            None if lower.contains("medium") => Self::Medium,
            None => Self::Small,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Small => "Small (<=9B) [High-Speed / Compact KV Cache]",
            Self::Medium => "Medium (10B-34B) [Balanced Workstation]",
            Self::Large => "Large (>=35B) [Deep Reasoning / High VRAM]",
        }
    }
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

pub struct LlmClient {
    http: reqwest::Client,
    pub provider: String,
    pub api: ApiStyle,
    server_root: String,
    openai_base: String,
    api_key: Option<String>,
    max_retries: u32,
    /// Cleared when the engine rejects `tools`; we then use text tool calls.
    native_tools: AtomicBool,
    /// Cleared when the engine rejects `stream_options`.
    stream_usage: AtomicBool,
}

impl LlmClient {
    pub fn new(cfg: &Config) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(cfg.request_timeout_secs))
            .pool_idle_timeout(Duration::from_secs(120))
            .tcp_nodelay(true)
            .user_agent(concat!("qsl-agent/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Self {
            http,
            provider: cfg.provider.clone(),
            api: cfg.api_style(),
            server_root: cfg.server_root(),
            openai_base: cfg.openai_base(),
            api_key: cfg.api_key.clone().filter(|k| !k.trim().is_empty()),
            max_retries: cfg.max_retries,
            native_tools: AtomicBool::new(true),
            stream_usage: AtomicBool::new(true),
        }
    }

    pub fn endpoint(&self) -> String {
        match self.api {
            ApiStyle::Ollama => format!("{}/api/chat", self.server_root),
            _ => format!("{}/chat/completions", self.openai_base),
        }
    }

    pub fn native_tools(&self) -> bool {
        self.native_tools.load(Ordering::Relaxed)
    }

    pub fn disable_native_tools(&self) {
        self.native_tools.store(false, Ordering::Relaxed);
    }

    fn auth(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.api_key {
            Some(k) => rb.bearer_auth(k),
            None => rb,
        }
    }

    fn map_send_error(url: &str, e: reqwest::Error) -> ProviderError {
        let detail = root_cause(&e);
        if e.is_timeout() {
            ProviderError::Timeout {
                url: url.into(),
                detail,
            }
        } else if e.is_connect() || e.is_request() {
            ProviderError::Connect {
                url: url.into(),
                detail,
            }
        } else {
            ProviderError::Parse(detail)
        }
    }

    // -- discovery ----------------------------------------------------------

    pub async fn list_models(&self) -> Result<Vec<String>, ProviderError> {
        if self.provider == "ollama" || self.api == ApiStyle::Ollama {
            let url = format!("{}/api/tags", self.server_root);
            if let Ok(res) = self
                .auth(self.http.get(&url))
                .timeout(Duration::from_secs(5))
                .send()
                .await
            {
                if res.status().is_success() {
                    if let Ok(v) = res.json::<Value>().await {
                        let names: Vec<String> = v["models"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|m| m["name"].as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();
                        if !names.is_empty() {
                            return Ok(names);
                        }
                    }
                }
            }
        }
        let url = format!("{}/models", self.openai_base);
        let res = self
            .auth(self.http.get(&url))
            .timeout(Duration::from_secs(8))
            .send()
            .await
            .map_err(|e| Self::map_send_error(&url, e))?;
        let status = res.status();
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(ProviderError::Http {
                status: status.as_u16(),
                body: crate::util::truncate_chars(&body, 300),
            });
        }
        let v: Value = res
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;
        Ok(v["data"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|m| m["id"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default())
    }

    /// First chat-capable model (embedding models are skipped).
    pub async fn auto_detect_model(&self) -> Option<String> {
        let models = self.list_models().await.ok()?;
        models
            .iter()
            .find(|m| !m.to_lowercase().contains("embed"))
            .or(models.first())
            .cloned()
    }

    async fn get_json(&self, url: &str) -> Option<Value> {
        let res = self
            .auth(self.http.get(url))
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .ok()?;
        if !res.status().is_success() {
            return None;
        }
        let text = res.text().await.ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Ollama model metadata used to size the context window: maximum
    /// context, KV-cache bytes per token, weight size and current residency.
    pub async fn model_meta(&self, model: &str) -> crate::sizing::ModelMeta {
        let mut meta = crate::sizing::ModelMeta::default();
        if self.api != ApiStyle::Ollama || model.is_empty() {
            return meta;
        }
        let url = format!("{}/api/show", self.server_root);
        let show: Option<Value> = async {
            let res = self
                .auth(self.http.post(&url))
                .json(&json!({ "model": model }))
                .timeout(Duration::from_secs(5))
                .send()
                .await
                .ok()?;
            let text = res.text().await.ok()?;
            serde_json::from_str(&text).ok()
        }
        .await;
        if let Some(info) = show.as_ref().and_then(|v| v["model_info"].as_object()) {
            meta.max_ctx = info
                .iter()
                .find(|(k, _)| k.ends_with(".context_length"))
                .and_then(|(_, v)| v.as_u64())
                .map(|n| n.min(u32::MAX as u64) as u32);
            meta.kv_bytes_per_token = crate::sizing::kv_bytes_per_token(info);
        }
        if let Some(tags) = self
            .get_json(&format!("{}/api/tags", self.server_root))
            .await
        {
            meta.weights_bytes = tags["models"]
                .as_array()
                .and_then(|a| a.iter().find(|m| m["name"].as_str() == Some(model)))
                .and_then(|m| m["size"].as_u64());
        }
        if let Some(ps) = self.get_json(&format!("{}/api/ps", self.server_root)).await {
            meta.resident_bytes = ps["models"]
                .as_array()
                .map(|a| a.iter().filter_map(|m| m["size"].as_u64()).sum())
                .unwrap_or(0);
        }
        meta
    }

    /// Context window actually loaded by an OpenAI-compatible server, where
    /// the server (not the request) decides it: llama.cpp `/props`,
    /// LM Studio `/api/v0/models`.
    pub async fn server_context(&self, model: &str) -> Option<u32> {
        if self.api == ApiStyle::Ollama {
            return None;
        }
        if let Some(v) = self.get_json(&format!("{}/props", self.server_root)).await {
            if let Some(n) = v["default_generation_settings"]["n_ctx"]
                .as_u64()
                .or(v["n_ctx"].as_u64())
            {
                return Some(n as u32);
            }
        }
        if let Some(v) = self
            .get_json(&format!("{}/api/v0/models", self.server_root))
            .await
        {
            let models = v["data"].as_array()?;
            let m = models
                .iter()
                .find(|m| m["id"].as_str() == Some(model))
                .or(models.first())?;
            return m["loaded_context_length"]
                .as_u64()
                .or(m["max_context_length"].as_u64())
                .map(|n| n as u32);
        }
        None
    }

    pub async fn health_check(&self) -> Result<String, ProviderError> {
        if self.api == ApiStyle::Ollama {
            let url = format!("{}/api/version", self.server_root);
            let res = self
                .auth(self.http.get(&url))
                .timeout(Duration::from_secs(4))
                .send()
                .await
                .map_err(|e| Self::map_send_error(&url, e))?;
            let v: Value = res.json().await.unwrap_or(Value::Null);
            return Ok(format!(
                "Ollama {}",
                v["version"].as_str().unwrap_or("(unknown version)")
            ));
        }
        let n = self.list_models().await?.len();
        Ok(format!("{} model(s) served at {}", n, self.openai_base))
    }

    // -- chat ---------------------------------------------------------------

    /// Run one chat completion, streaming deltas into `on_delta`.
    /// Transient failures before any output are retried with backoff.
    pub async fn chat(
        &self,
        model: &str,
        messages: &[Message],
        opts: &ChatOptions<'_>,
        interrupt: &Interrupt,
        on_delta: &mut (dyn FnMut(Delta<'_>) + Send),
    ) -> Result<ChatResponse, ProviderError> {
        let mut attempt = 0u32;
        let mut adapted = false;
        loop {
            if interrupt.is_set() {
                return Err(ProviderError::Interrupted);
            }
            let mut emitted = false;
            let result = {
                let mut tracking = |d: Delta<'_>| {
                    emitted = true;
                    on_delta(d)
                };
                self.chat_once(model, messages, opts, interrupt, &mut tracking)
                    .await
            };
            match result {
                Ok(r) => return Ok(r),
                Err(e)
                    if !emitted
                        && !adapted
                        && self.stream_usage.load(Ordering::Relaxed)
                        && e.mentions(&["stream_options"]) =>
                {
                    self.stream_usage.store(false, Ordering::Relaxed);
                    adapted = true;
                }
                Err(e)
                    if !emitted
                        && opts.tools.is_some()
                        && self.native_tools()
                        && e.tools_unsupported() =>
                {
                    tracing::warn!("engine rejected native tools; switching to text tool protocol");
                    self.disable_native_tools();
                }
                Err(e) if !emitted && e.retryable() && attempt < self.max_retries => {
                    attempt += 1;
                    let backoff = Duration::from_millis(750 * 2u64.pow(attempt - 1) + jitter_ms());
                    tracing::warn!(attempt, ?backoff, error = %e, "transient provider error; retrying");
                    if !interrupt.sleep(backoff).await {
                        return Err(ProviderError::Interrupted);
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }

    async fn chat_once(
        &self,
        model: &str,
        messages: &[Message],
        opts: &ChatOptions<'_>,
        interrupt: &Interrupt,
        on_delta: &mut (dyn FnMut(Delta<'_>) + Send),
    ) -> Result<ChatResponse, ProviderError> {
        let native = self.native_tools();
        let tools = opts.tools.filter(|t| !t.is_empty() && native);
        let (url, body) = match self.api {
            ApiStyle::Ollama => {
                let mut body = json!({
                    "model": model,
                    "messages": wire_messages_ollama(messages, native),
                    "stream": opts.stream,
                    "options": {
                        "num_ctx": opts.num_ctx,
                        "num_predict": opts.max_tokens,
                        "temperature": opts.temperature,
                    },
                });
                if let Some(t) = tools {
                    body["tools"] = Value::Array(t.iter().map(ToolSpec::to_wire).collect());
                }
                (format!("{}/api/chat", self.server_root), body)
            }
            _ => {
                let mut body = json!({
                    "model": model,
                    "messages": wire_messages_openai(messages, native),
                    "stream": opts.stream,
                    "temperature": opts.temperature,
                    "max_tokens": opts.max_tokens,
                });
                if opts.stream && self.stream_usage.load(Ordering::Relaxed) {
                    body["stream_options"] = json!({ "include_usage": true });
                }
                if let Some(t) = tools {
                    body["tools"] = Value::Array(t.iter().map(ToolSpec::to_wire).collect());
                    body["tool_choice"] = json!("auto");
                }
                (format!("{}/chat/completions", self.openai_base), body)
            }
        };

        let send = self.auth(self.http.post(&url)).json(&body).send();
        let resp = tokio::select! {
            r = send => r.map_err(|e| Self::map_send_error(&url, e))?,
            _ = interrupt.cancelled() => return Err(ProviderError::Interrupted),
        };
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            let body = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| {
                    v["error"]["message"]
                        .as_str()
                        .or(v["error"].as_str())
                        .or(v["message"].as_str())
                        .map(String::from)
                })
                .unwrap_or(text);
            return Err(ProviderError::Http {
                status: status.as_u16(),
                body: crate::util::truncate_chars(body.trim(), 800),
            });
        }

        let is_event_stream = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|ct| ct.contains("event-stream") || ct.contains("ndjson"));

        if !opts.stream || (self.api != ApiStyle::Ollama && !is_event_stream) {
            let text = tokio::select! {
                t = resp.text() => t.map_err(|e| ProviderError::Parse(e.to_string()))?,
                _ = interrupt.cancelled() => return Err(ProviderError::Interrupted),
            };
            // Ollama answers stream:false with one JSON object; some servers
            // ignore `stream` entirely, so detect SSE text here as well.
            if text.trim_start().starts_with("data:") {
                let mut acc = Accumulator::default();
                for line in text.lines() {
                    if let Some(done) = acc.feed_sse_line(line, on_delta)? {
                        if done {
                            break;
                        }
                    }
                }
                return Ok(acc.finish());
            }
            let v: Value = serde_json::from_str(&text).map_err(|e| {
                ProviderError::Parse(format!("{e}: {}", crate::util::truncate_chars(&text, 200)))
            })?;
            let r = match self.api {
                ApiStyle::Ollama => parse_ollama_full(&v)?,
                _ => parse_openai_full(&v)?,
            };
            if !r.reasoning.is_empty() {
                on_delta(Delta::Reasoning(&r.reasoning));
            }
            if !r.content.is_empty() {
                on_delta(Delta::Content(&r.content));
            }
            return Ok(r);
        }

        let mut stream = resp.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();
        let mut acc = Accumulator::default();
        let ollama = self.api == ApiStyle::Ollama;
        'outer: loop {
            let chunk = tokio::select! {
                c = stream.next() => c,
                _ = interrupt.cancelled() => return Err(ProviderError::Interrupted),
            };
            let Some(chunk) = chunk else { break };
            let chunk = chunk.map_err(|e| {
                if e.is_timeout() {
                    ProviderError::Timeout {
                        url: url.clone(),
                        detail: "no data received from the engine".into(),
                    }
                } else {
                    ProviderError::Parse(format!("stream interrupted: {}", root_cause(&e)))
                }
            })?;
            buf.extend_from_slice(&chunk);
            while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buf.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line);
                let done = if ollama {
                    acc.feed_ollama_line(&line, on_delta)?
                } else {
                    acc.feed_sse_line(&line, on_delta)?
                };
                if done == Some(true) {
                    break 'outer;
                }
            }
        }
        if !buf.is_empty() {
            let line = String::from_utf8_lossy(&buf).to_string();
            if ollama {
                acc.feed_ollama_line(&line, on_delta)?;
            } else {
                acc.feed_sse_line(&line, on_delta)?;
            }
        }
        Ok(acc.finish())
    }
}

fn jitter_ms() -> u64 {
    (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0)
        % 400) as u64
}

fn root_cause(e: &dyn std::error::Error) -> String {
    let mut msg = e.to_string();
    let mut src = e.source();
    while let Some(s) = src {
        msg = s.to_string();
        src = s.source();
    }
    msg
}

// ---------------------------------------------------------------------------
// Wire serialisation
// ---------------------------------------------------------------------------

/// Hermes-style text form of a tool call, for engines without native tools.
pub fn tool_call_as_text(tc: &ToolCall) -> String {
    let args: Value =
        serde_json::from_str(&tc.arguments).unwrap_or_else(|_| Value::String(tc.arguments.clone()));
    format!(
        "<tool_call>\n{}\n</tool_call>",
        json!({ "name": tc.name, "arguments": args })
    )
}

/// Collapse messages for engines without native tool support: tool calls
/// become `<tool_call>` text and results become `<tool_response>` user turns.
fn textify(messages: &[Message]) -> Vec<(Role, String)> {
    let mut out: Vec<(Role, String)> = Vec::new();
    for m in messages {
        let (role, content) = match m.role {
            Role::Assistant if !m.tool_calls.is_empty() => {
                let calls: Vec<String> = m.tool_calls.iter().map(tool_call_as_text).collect();
                (
                    Role::Assistant,
                    format!("{}\n{}", m.content, calls.join("\n"))
                        .trim()
                        .to_string(),
                )
            }
            Role::Tool => (
                Role::User,
                format!(
                    "<tool_response>\n{}\n</tool_response>",
                    json!({ "name": m.name.clone().unwrap_or_default(), "content": m.content })
                ),
            ),
            r => (r, m.content.clone()),
        };
        match out.last_mut() {
            Some((last_role, last)) if *last_role == role && role == Role::User => {
                last.push_str("\n\n");
                last.push_str(&content);
            }
            _ => out.push((role, content)),
        }
    }
    out
}

fn role_str(r: Role) -> &'static str {
    match r {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

pub fn wire_messages_openai(messages: &[Message], native_tools: bool) -> Vec<Value> {
    if !native_tools {
        return textify(messages)
            .into_iter()
            .map(|(r, c)| json!({ "role": role_str(r), "content": c }))
            .collect();
    }
    messages
        .iter()
        .map(|m| match m.role {
            Role::Assistant if !m.tool_calls.is_empty() => json!({
                "role": "assistant",
                "content": if m.content.is_empty() { Value::Null } else { Value::String(m.content.clone()) },
                "tool_calls": m.tool_calls.iter().map(|tc| json!({
                    "id": tc.id,
                    "type": "function",
                    "function": { "name": tc.name, "arguments": normalize_args(&tc.arguments) },
                })).collect::<Vec<_>>(),
            }),
            Role::Tool => json!({
                "role": "tool",
                "tool_call_id": m.tool_call_id.clone().unwrap_or_default(),
                "content": m.content,
            }),
            r => json!({ "role": role_str(r), "content": m.content }),
        })
        .collect()
}

pub fn wire_messages_ollama(messages: &[Message], native_tools: bool) -> Vec<Value> {
    if !native_tools {
        return textify(messages)
            .into_iter()
            .map(|(r, c)| json!({ "role": role_str(r), "content": c }))
            .collect();
    }
    messages
        .iter()
        .map(|m| match m.role {
            Role::Assistant if !m.tool_calls.is_empty() => json!({
                "role": "assistant",
                "content": m.content,
                "tool_calls": m.tool_calls.iter().map(|tc| json!({
                    "function": {
                        "name": tc.name,
                        "arguments": serde_json::from_str::<Value>(&tc.arguments).ok().filter(Value::is_object).unwrap_or_else(|| json!({})),
                    }
                })).collect::<Vec<_>>(),
            }),
            Role::Tool => json!({
                "role": "tool",
                "content": m.content,
                "tool_name": m.name.clone().unwrap_or_default(),
            }),
            r => json!({ "role": role_str(r), "content": m.content }),
        })
        .collect()
}

/// Servers re-validate assistant tool-call arguments on the way back in;
/// a malformed string from an earlier step would poison every later request.
fn normalize_args(raw: &str) -> String {
    match serde_json::from_str::<Value>(raw) {
        Ok(v) if v.is_object() => raw.to_string(),
        _ => "{}".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Response parsing
// ---------------------------------------------------------------------------

#[derive(Default)]
struct PartialCall {
    id: String,
    name: String,
    args: String,
}

#[derive(Default)]
struct Accumulator {
    content: String,
    reasoning: String,
    calls: Vec<(u64, PartialCall)>,
    usage: Option<Usage>,
    finish_reason: Option<String>,
}

impl Accumulator {
    fn call_at(&mut self, index: u64) -> &mut PartialCall {
        if let Some(pos) = self.calls.iter().position(|(i, _)| *i == index) {
            return &mut self.calls[pos].1;
        }
        self.calls.push((index, PartialCall::default()));
        &mut self.calls.last_mut().expect("just pushed").1
    }

    /// Feed one SSE line. Returns `Some(true)` at `[DONE]`.
    fn feed_sse_line(
        &mut self,
        line: &str,
        on_delta: &mut (dyn FnMut(Delta<'_>) + Send),
    ) -> Result<Option<bool>, ProviderError> {
        let line = line.trim_end_matches(['\r', '\n']);
        let Some(data) = line.strip_prefix("data:") else {
            return Ok(None);
        };
        let data = data.trim();
        if data == "[DONE]" {
            return Ok(Some(true));
        }
        if data.is_empty() {
            return Ok(None);
        }
        let v: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => return Ok(None),
        };
        if let Some(err) = v.get("error") {
            let msg = err["message"]
                .as_str()
                .or(err.as_str())
                .unwrap_or("unknown error")
                .to_string();
            return Err(ProviderError::Http {
                status: err["code"].as_u64().unwrap_or(500) as u16,
                body: msg,
            });
        }
        if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
            self.usage = Some(Usage {
                prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0),
                completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0),
            });
        }
        let Some(choice) = v["choices"].get(0) else {
            return Ok(Some(false));
        };
        if let Some(fr) = choice["finish_reason"].as_str() {
            self.finish_reason = Some(fr.to_string());
        }
        let delta = &choice["delta"];
        for key in ["reasoning_content", "reasoning"] {
            if let Some(r) = delta[key].as_str().filter(|r| !r.is_empty()) {
                self.reasoning.push_str(r);
                on_delta(Delta::Reasoning(r));
                break;
            }
        }
        if let Some(c) = delta["content"].as_str().filter(|c| !c.is_empty()) {
            self.content.push_str(c);
            on_delta(Delta::Content(c));
        }
        if let Some(calls) = delta["tool_calls"].as_array() {
            for (pos, call) in calls.iter().enumerate() {
                let index = call["index"].as_u64().unwrap_or(pos as u64);
                let started_name;
                {
                    let slot = self.call_at(index);
                    if let Some(id) = call["id"].as_str().filter(|s| !s.is_empty()) {
                        slot.id = id.to_string();
                    }
                    started_name = match call["function"]["name"].as_str().filter(|s| !s.is_empty())
                    {
                        Some(n) if slot.name.is_empty() => {
                            slot.name = n.to_string();
                            Some(n.to_string())
                        }
                        Some(n) if slot.name != n && !slot.name.ends_with(n) => {
                            slot.name.push_str(n);
                            None
                        }
                        _ => None,
                    };
                    match &call["function"]["arguments"] {
                        Value::String(s) => slot.args.push_str(s),
                        Value::Null => {}
                        other => slot.args = other.to_string(),
                    }
                }
                if let Some(n) = started_name {
                    on_delta(Delta::ToolCall(&n));
                }
            }
        }
        Ok(Some(false))
    }

    /// Feed one Ollama NDJSON line. Returns `Some(true)` when `done`.
    fn feed_ollama_line(
        &mut self,
        line: &str,
        on_delta: &mut (dyn FnMut(Delta<'_>) + Send),
    ) -> Result<Option<bool>, ProviderError> {
        let line = line.trim();
        if line.is_empty() {
            return Ok(None);
        }
        let v: Value = serde_json::from_str(line).map_err(|e| {
            ProviderError::Parse(format!("{e}: {}", crate::util::truncate_chars(line, 200)))
        })?;
        if let Some(err) = v["error"].as_str() {
            return Err(ProviderError::Http {
                status: 500,
                body: err.to_string(),
            });
        }
        let msg = &v["message"];
        if let Some(t) = msg["thinking"].as_str().filter(|t| !t.is_empty()) {
            self.reasoning.push_str(t);
            on_delta(Delta::Reasoning(t));
        }
        if let Some(c) = msg["content"].as_str().filter(|c| !c.is_empty()) {
            self.content.push_str(c);
            on_delta(Delta::Content(c));
        }
        if let Some(calls) = msg["tool_calls"].as_array() {
            for call in calls {
                let name = call["function"]["name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let args = match &call["function"]["arguments"] {
                    Value::String(s) => s.clone(),
                    Value::Null => "{}".into(),
                    other => other.to_string(),
                };
                let index = self.calls.len() as u64;
                let id = call["id"].as_str().map(String::from).unwrap_or_default();
                self.calls.push((
                    index,
                    PartialCall {
                        id,
                        name: name.clone(),
                        args,
                    },
                ));
                on_delta(Delta::ToolCall(&name));
            }
        }
        if v["done"].as_bool() == Some(true) {
            self.usage = Some(Usage {
                prompt_tokens: v["prompt_eval_count"].as_u64().unwrap_or(0),
                completion_tokens: v["eval_count"].as_u64().unwrap_or(0),
            });
            self.finish_reason = v["done_reason"].as_str().map(String::from);
            return Ok(Some(true));
        }
        Ok(Some(false))
    }

    fn finish(self) -> ChatResponse {
        let tool_calls = self
            .calls
            .into_iter()
            .filter(|(_, c)| !c.name.trim().is_empty())
            .map(|(_, c)| ToolCall {
                id: if c.id.is_empty() {
                    format!("call_{}", short_id())
                } else {
                    c.id
                },
                name: c.name.trim().to_string(),
                arguments: if c.args.trim().is_empty() {
                    "{}".into()
                } else {
                    c.args
                },
            })
            .collect();
        ChatResponse {
            content: self.content,
            reasoning: self.reasoning,
            tool_calls,
            usage: self.usage,
            finish_reason: self.finish_reason,
        }
    }
}

fn parse_openai_full(v: &Value) -> Result<ChatResponse, ProviderError> {
    if let Some(err) = v.get("error") {
        return Err(ProviderError::Http {
            status: 500,
            body: err["message"]
                .as_str()
                .unwrap_or("unknown error")
                .to_string(),
        });
    }
    let choice = v["choices"]
        .get(0)
        .ok_or_else(|| ProviderError::Parse("no choices returned by model".into()))?;
    let msg = &choice["message"];
    let tool_calls = msg["tool_calls"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|c| {
                    let name = c["function"]["name"].as_str()?.to_string();
                    let arguments = match &c["function"]["arguments"] {
                        Value::String(s) => s.clone(),
                        Value::Null => "{}".into(),
                        other => other.to_string(),
                    };
                    let id = c["id"]
                        .as_str()
                        .map(String::from)
                        .unwrap_or_else(|| format!("call_{}", short_id()));
                    Some(ToolCall {
                        id,
                        name,
                        arguments,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(ChatResponse {
        content: msg["content"].as_str().unwrap_or_default().to_string(),
        reasoning: msg["reasoning_content"]
            .as_str()
            .or(msg["reasoning"].as_str())
            .unwrap_or_default()
            .to_string(),
        tool_calls,
        usage: v.get("usage").filter(|u| u.is_object()).map(|u| Usage {
            prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0),
            completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0),
        }),
        finish_reason: choice["finish_reason"].as_str().map(String::from),
    })
}

fn parse_ollama_full(v: &Value) -> Result<ChatResponse, ProviderError> {
    let mut acc = Accumulator::default();
    let mut sink = |_: Delta<'_>| {};
    acc.feed_ollama_line(&v.to_string(), &mut sink)?;
    Ok(acc.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all(lines: &[&str]) -> ChatResponse {
        let mut acc = Accumulator::default();
        let mut sink = |_: Delta<'_>| {};
        for l in lines {
            acc.feed_sse_line(l, &mut sink).unwrap();
        }
        acc.finish()
    }

    #[test]
    fn sse_accumulates_parallel_tool_calls_by_index() {
        let r = feed_all(&[
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"a","function":{"name":"read_file","arguments":"{\"pa"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":1,"id":"b","function":{"name":"list_dir","arguments":"{}"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\": \"x}y\"}"}}]}}]}"#,
            r#"data: {"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5}}"#,
            "data: [DONE]",
        ]);
        assert_eq!(r.tool_calls.len(), 2);
        assert_eq!(r.tool_calls[0].name, "read_file");
        assert_eq!(r.tool_calls[0].arguments, r#"{"path": "x}y"}"#);
        assert_eq!(r.tool_calls[1].id, "b");
        assert_eq!(r.usage.unwrap().prompt_tokens, 10);
    }

    #[test]
    fn sse_reasoning_and_content() {
        let r = feed_all(&[
            r#"data: {"choices":[{"delta":{"reasoning_content":"think "}}]}"#,
            r#"data: {"choices":[{"delta":{"content":"Hello"}}]}"#,
            r#"data: {"choices":[{"delta":{"content":" world"},"finish_reason":"stop"}]}"#,
        ]);
        assert_eq!(r.reasoning, "think ");
        assert_eq!(r.content, "Hello world");
        assert_eq!(r.finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn ollama_lines() {
        let mut acc = Accumulator::default();
        let mut sink = |_: Delta<'_>| {};
        acc.feed_ollama_line(
            r#"{"message":{"role":"assistant","content":"","thinking":"hmm"},"done":false}"#,
            &mut sink,
        )
        .unwrap();
        acc.feed_ollama_line(
            r#"{"message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":"list_dir","arguments":{"path":"."}}}]},"done":false}"#,
            &mut sink,
        )
        .unwrap();
        let done = acc.feed_ollama_line(r#"{"message":{"role":"assistant","content":"ok"},"done":true,"prompt_eval_count":7,"eval_count":3}"#, &mut sink).unwrap();
        assert_eq!(done, Some(true));
        let r = acc.finish();
        assert_eq!(r.reasoning, "hmm");
        assert_eq!(r.tool_calls[0].arguments, r#"{"path":"."}"#);
        assert_eq!(r.usage.unwrap().completion_tokens, 3);
    }

    #[test]
    fn openai_wire_format_matches_spec() {
        let mut a = Message::assistant("");
        a.tool_calls = vec![ToolCall {
            id: "c1".into(),
            name: "read_file".into(),
            arguments: "{\"path\":\"a\"}".into(),
        }];
        let wire = wire_messages_openai(&[a, Message::tool("c1", "read_file", "data")], true);
        assert_eq!(wire[0]["tool_calls"][0]["type"], "function");
        assert_eq!(
            wire[0]["tool_calls"][0]["function"]["arguments"],
            "{\"path\":\"a\"}"
        );
        assert!(wire[0]["content"].is_null());
        assert_eq!(wire[1]["tool_call_id"], "c1");
    }

    #[test]
    fn text_mode_wraps_tool_traffic() {
        let mut a = Message::assistant("Let me look.");
        a.tool_calls = vec![ToolCall {
            id: "c1".into(),
            name: "list_dir".into(),
            arguments: "{}".into(),
        }];
        let wire = wire_messages_openai(
            &[
                Message::user("hi"),
                a,
                Message::tool("c1", "list_dir", "x"),
                Message::tool("c2", "list_dir", "y"),
            ],
            false,
        );
        assert_eq!(
            wire.len(),
            3,
            "consecutive tool results merge into one user turn"
        );
        assert!(wire[1]["content"].as_str().unwrap().contains("<tool_call>"));
        assert_eq!(wire[2]["role"], "user");
        assert!(wire[2]["content"]
            .as_str()
            .unwrap()
            .contains("<tool_response>"));
    }

    #[test]
    fn malformed_history_args_are_sanitized() {
        assert_eq!(normalize_args("{\"a\":"), "{}");
        assert_eq!(normalize_args("{\"a\":1}"), "{\"a\":1}");
    }

    #[test]
    fn model_tier_classification() {
        assert_eq!(ModelTier::from_model_name("llama3.2:3b"), ModelTier::Small);
        assert_eq!(
            ModelTier::from_model_name("huihui-qwen35-9b-256k:latest"),
            ModelTier::Small
        );
        assert_eq!(
            ModelTier::from_model_name("qwen2.5-coder:14b"),
            ModelTier::Medium
        );
        assert_eq!(ModelTier::from_model_name("qwen2.5:32b"), ModelTier::Medium);
        assert_eq!(ModelTier::from_model_name("llama3.3:70b"), ModelTier::Large);
        assert_eq!(
            ModelTier::from_model_name("qwen2.5:72b-instruct-q4_k_m"),
            ModelTier::Large
        );
        assert_eq!(ModelTier::from_model_name("gpt-oss:120b"), ModelTier::Large);
        assert_eq!(ModelTier::from_model_name("qwen3:0.6b"), ModelTier::Small);
    }

    #[test]
    fn error_classification() {
        let e = ProviderError::Http {
            status: 400,
            body: "registry.ollama.ai/library/gemma does not support tools".into(),
        };
        assert!(e.tools_unsupported());
        assert!(!e.retryable());
        assert!(ProviderError::Http {
            status: 503,
            body: String::new()
        }
        .retryable());
        let e = ProviderError::Http {
            status: 400,
            body: "request (3326 tokens) exceeds the available context size (2048 tokens), try increasing it".into(),
        };
        assert!(e.context_overflow());
        let e = ProviderError::Http {
            status: 500,
            body: "unexpected end of JSON input".into(),
        };
        assert!(e.truncated_tool_json());
    }
}
