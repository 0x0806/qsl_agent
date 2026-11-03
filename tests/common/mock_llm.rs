//! Scripted mock inference server speaking both the OpenAI-compatible SSE
//! protocol and Ollama's native NDJSON protocol. Behaviour is chosen by
//! keywords in the first user message, so tests are deterministic.
//!
//! Shared by `tests/e2e.rs` and `examples/mock_llm.rs` (used by
//! `build-test/smoke.sh`).

#![allow(dead_code)]

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct MockState {
    pub requests: Arc<Mutex<Vec<Value>>>,
}

pub const MODEL: &str = "mock-coder:7b";

/// Start on 127.0.0.1:`port` (0 = random). Returns the bound port.
pub async fn start(port: u16) -> (u16, MockState) {
    let state = MockState::default();
    let app = Router::new()
        .route(
            "/v1/models",
            get(|| async {
                Json(json!({"object": "list", "data": [{"id": MODEL}, {"id": "nomic-embed-text"}]}))
            }),
        )
        .route("/v1/chat/completions", post(openai_chat))
        .route(
            "/api/tags",
            get(|| async { Json(json!({"models": [{"name": MODEL}]})) }),
        )
        .route(
            "/api/version",
            get(|| async { Json(json!({"version": "0.0.0-mock"})) }),
        )
        .route(
            "/api/show",
            post(|| async { Json(json!({"model_info": {"llama.context_length": 8192}})) }),
        )
        .route("/api/chat", post(ollama_chat))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("bind mock");
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });
    (port, state)
}

/// What the scripted model does next.
enum Plan {
    Text(String),
    Tools(Vec<(String, Value)>),
    Error(StatusCode, String),
}

/// The latest real user request (not tool responses or system notes).
fn current_task(messages: &[Value]) -> String {
    messages
        .iter()
        .rev()
        .filter(|m| m["role"] == "user")
        .filter_map(|m| m["content"].as_str())
        .find(|c| {
            !c.contains("<tool_response>")
                && !c.starts_with("[SYSTEM NOTE]")
                && !c.starts_with("Step budget reached")
        })
        .unwrap_or("")
        .to_string()
}

/// Tool results that belong to the current task (after its user message).
fn results_since_task(messages: &[Value], task: &str) -> Vec<String> {
    let start = messages
        .iter()
        .rposition(|m| m["role"] == "user" && m["content"].as_str() == Some(task))
        .unwrap_or(0);
    tool_results(&messages[start..])
}

fn tool_results(messages: &[Value]) -> Vec<String> {
    messages
        .iter()
        .filter(|m| {
            m["role"] == "tool"
                || (m["role"] == "user"
                    && m["content"]
                        .as_str()
                        .unwrap_or("")
                        .contains("<tool_response>"))
        })
        .map(|m| m["content"].as_str().unwrap_or("").to_string())
        .collect()
}

fn plan(body: &Value) -> Plan {
    let messages = body["messages"].as_array().cloned().unwrap_or_default();
    let has_tools = body
        .get("tools")
        .is_some_and(|t| t.as_array().is_some_and(|a| !a.is_empty()));
    let system = messages
        .first()
        .and_then(|m| m["content"].as_str())
        .unwrap_or("");
    let task = current_task(&messages);
    let results = results_since_task(&messages, &task);
    let last = results.last().cloned().unwrap_or_default();

    if system.starts_with("You are compressing") {
        return Plan::Text("- summary: user asked for several things".into());
    }
    if task.contains("[mock:write]") {
        let path = task
            .split_whitespace()
            .find(|w| w.starts_with('/'))
            .unwrap_or("/tmp/qsl-mock.txt")
            .to_string();
        return if results.is_empty() {
            Plan::Tools(vec![(
                "write_file".into(),
                json!({"path": path, "content": "hello from qsl"}),
            )])
        } else {
            Plan::Text(format!(
                "Done. TOOL_RESULT: {}",
                last.lines().next().unwrap_or("")
            ))
        };
    }
    if task.contains("[mock:parallel]") {
        let dir = task
            .split_whitespace()
            .find(|w| w.starts_with('/'))
            .unwrap_or("/tmp")
            .to_string();
        return if results.is_empty() {
            Plan::Tools(vec![
                ("list_dir".into(), json!({"path": dir})),
                (
                    "search_files".into(),
                    json!({"pattern": "needle", "path": dir}),
                ),
            ])
        } else {
            Plan::Text(format!(
                "RESULTS={} NEEDLE={}",
                results.len(),
                results.iter().any(|r| r.contains("needle"))
            ))
        };
    }
    if task.contains("[mock:danger]") {
        return if results.is_empty() {
            Plan::Tools(vec![(
                "execute_command".into(),
                json!({"command": "rm -rf /"}),
            )])
        } else {
            Plan::Text(format!("BLOCKED={}", last.contains("SECURITY BLOCKED")))
        };
    }
    if task.contains("[mock:risky]") {
        return if results.is_empty() {
            Plan::Tools(vec![(
                "execute_command".into(),
                json!({"command": "rm /tmp/qsl-nonexistent-file"}),
            )])
        } else {
            Plan::Text(format!("DENIED={}", last.contains("DENIED")))
        };
    }
    if task.contains("[mock:shell]") {
        return if results.is_empty() {
            Plan::Tools(vec![(
                "execute_command".into(),
                json!({"command": "echo shell-ok-$((6*7))"}),
            )])
        } else {
            Plan::Text(format!("SHELL={}", last.contains("shell-ok-42")))
        };
    }
    if task.contains("[mock:sleep]") {
        return if results.is_empty() {
            Plan::Tools(vec![(
                "execute_command".into(),
                json!({"command": "sleep 30 && echo finished"}),
            )])
        } else {
            Plan::Text("SLEEP_DONE".into())
        };
    }
    if task.contains("[mock:think]") {
        return Plan::Text("<think>secret planning</think>The answer is 42.".into());
    }
    if task.contains("[mock:loop]") {
        // Always asks for the same call: exercises the repetition guard and step budget.
        return Plan::Tools(vec![("list_dir".into(), json!({"path": "/tmp"}))]);
    }
    if task.contains("[mock:notools]") {
        if has_tools {
            return Plan::Error(
                StatusCode::BAD_REQUEST,
                format!("registry.ollama.ai/library/{MODEL} does not support tools"),
            );
        }
        return if results.is_empty() {
            Plan::Text("I will look.\n<tool_call>\n{\"name\": \"list_dir\", \"arguments\": {\"path\": \"/tmp\"}}\n</tool_call>".into())
        } else {
            Plan::Text(format!("TEXT_PROTOCOL_OK={}", last.contains("/tmp")))
        };
    }
    if task.contains("[mock:badargs]") {
        return if results.is_empty() {
            Plan::Tools(vec![("list_dir".into(), json!("{\"path\": \"/tmp\",}"))])
        } else {
            Plan::Text(format!(
                "ARGS_REPAIRED={}",
                !last.starts_with("ERROR: arguments")
            ))
        };
    }
    if task.contains("[mock:memory]") {
        return if results.is_empty() {
            Plan::Tools(vec![(
                "obsidian_append".into(),
                json!({"memory": "The user's favourite language is Rust"}),
            )])
        } else {
            Plan::Text("MEMORY_SAVED".into())
        };
    }
    if task.contains("[mock:recall]") {
        let remembered = system.contains("favourite language is Rust");
        return Plan::Text(format!("RECALL={remembered}"));
    }
    let turns = messages.iter().filter(|m| m["role"] == "user").count();
    Plan::Text(format!(
        "Hello from the mock model. turns={turns} said={}",
        messages
            .iter()
            .rev()
            .find(|m| m["role"] == "user")
            .and_then(|m| m["content"].as_str())
            .unwrap_or("")
    ))
}

fn record(state: &MockState, body: &Value) {
    state.requests.lock().unwrap().push(body.clone());
}

async fn openai_chat(State(state): State<MockState>, Json(body): Json<Value>) -> Response {
    record(&state, &body);
    let stream = body["stream"].as_bool().unwrap_or(false);
    let usage = json!({"prompt_tokens": 120, "completion_tokens": 30, "total_tokens": 150});
    match plan(&body) {
        Plan::Error(code, msg) => (code, Json(json!({"error": {"message": msg}}))).into_response(),
        Plan::Text(text) if !stream => Json(json!({
            "choices": [{"index": 0, "message": {"role": "assistant", "content": text}, "finish_reason": "stop"}],
            "usage": usage,
        }))
        .into_response(),
        Plan::Tools(calls) if !stream => {
            let tcs: Vec<Value> = calls
                .iter()
                .enumerate()
                .map(|(i, (n, a))| json!({"id": format!("call_{i}"), "type": "function", "function": {"name": n, "arguments": arg_string(a)}}))
                .collect();
            Json(json!({
                "choices": [{"index": 0, "message": {"role": "assistant", "content": null, "tool_calls": tcs}, "finish_reason": "tool_calls"}],
                "usage": usage,
            }))
            .into_response()
        }
        Plan::Text(text) => {
            let mut events = Vec::new();
            for piece in chunk_text(&text) {
                events.push(json!({"choices": [{"index": 0, "delta": {"content": piece}}]}));
            }
            events.push(json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]}));
            events.push(json!({"choices": [], "usage": usage}));
            sse(events)
        }
        Plan::Tools(calls) => {
            let mut events = Vec::new();
            for (i, (name, args)) in calls.iter().enumerate() {
                let a = arg_string(args);
                let mid = a.char_indices().nth(a.chars().count() / 2).map(|(b, _)| b).unwrap_or(0);
                events.push(json!({"choices": [{"index": 0, "delta": {"tool_calls": [{"index": i, "id": format!("call_{i}"), "type": "function", "function": {"name": name, "arguments": &a[..mid]}}]}}]}));
                events.push(json!({"choices": [{"index": 0, "delta": {"tool_calls": [{"index": i, "function": {"arguments": &a[mid..]}}]}}]}));
            }
            events.push(json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]}));
            events.push(json!({"choices": [], "usage": usage}));
            sse(events)
        }
    }
}

fn arg_string(a: &Value) -> String {
    match a {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn chunk_text(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    chars.chunks(7).map(|c| c.iter().collect()).collect()
}

fn sse(events: Vec<Value>) -> Response {
    let mut body = String::new();
    for e in events {
        body.push_str(&format!("data: {e}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from(body))
        .unwrap()
}

async fn ollama_chat(State(state): State<MockState>, Json(body): Json<Value>) -> Response {
    record(&state, &body);
    let stream = body["stream"].as_bool().unwrap_or(true);
    let done = |extra: Value| {
        let mut v = json!({"model": MODEL, "message": {"role": "assistant", "content": ""}, "done": true, "done_reason": "stop", "prompt_eval_count": 100, "eval_count": 20});
        if let (Some(obj), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            for (k, val) in e {
                obj.insert(k.clone(), val.clone());
            }
        }
        v
    };
    let lines: Vec<Value> = match plan(&body) {
        Plan::Error(code, msg) => return (code, Json(json!({"error": msg}))).into_response(),
        Plan::Text(text) => {
            if !stream {
                return Json(done(
                    json!({"message": {"role": "assistant", "content": text}}),
                ))
                .into_response();
            }
            let mut v: Vec<Value> = chunk_text(&text).into_iter().map(|p| json!({"model": MODEL, "message": {"role": "assistant", "content": p}, "done": false})).collect();
            v.push(done(json!({})));
            v
        }
        Plan::Tools(calls) => {
            let tcs: Vec<Value> = calls
                .iter()
                .map(|(n, a)| {
                    let args = match a {
                        Value::String(s) => serde_json::from_str::<Value>(s).unwrap_or(json!({})),
                        other => other.clone(),
                    };
                    json!({"function": {"name": n, "arguments": args}})
                })
                .collect();
            if !stream {
                return Json(done(
                    json!({"message": {"role": "assistant", "content": "", "tool_calls": tcs}}),
                ))
                .into_response();
            }
            vec![
                json!({"model": MODEL, "message": {"role": "assistant", "content": "", "tool_calls": tcs}, "done": false}),
                done(json!({})),
            ]
        }
    };
    let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
    Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .body(Body::from(body))
        .unwrap()
}
