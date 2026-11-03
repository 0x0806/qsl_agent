//! HTTP API (`qsl-agent --serve 127.0.0.1:8765`) for scripts, editors and
//! other local programs. Requests are serialised through one agent; tools
//! run non-interactively (risky commands are denied unless `--approval yolo`).
//!
//! Auth: set `QSL_API_TOKEN` and send `Authorization: Bearer <token>`.
//! Binding to a non-loopback address requires a token.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::agent::Agent;

#[derive(Clone)]
struct AppState {
    agent: Arc<Mutex<Agent>>,
    token: Option<String>,
}

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    new_session: bool,
}

fn authorized(state: &AppState, headers: &HeaderMap) -> bool {
    match &state.token {
        None => true,
        Some(t) => headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .is_some_and(|given| constant_time_eq(given.trim().as_bytes(), t.as_bytes())),
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn err(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(json!({ "ok": false, "error": msg.into() }))).into_response()
}

async fn chat(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ChatRequest>,
) -> Response {
    if !authorized(&s, &headers) {
        return err(StatusCode::UNAUTHORIZED, "missing or invalid bearer token");
    }
    if req.message.trim().is_empty() {
        return err(StatusCode::BAD_REQUEST, "message must not be empty");
    }
    let mut agent = s.agent.lock().await;
    if req.new_session {
        agent.new_session();
    } else if let Some(id) = req.session_id.as_deref().filter(|id| *id != agent.meta.id) {
        if let Err(e) = agent.resume(id) {
            return err(StatusCode::NOT_FOUND, e);
        }
    }
    match agent.run_turn(&req.message).await {
        Ok(o) => Json(json!({
            "ok": true,
            "reply": o.text,
            "session_id": agent.meta.id,
            "steps": o.steps,
            "tool_calls": o.tool_calls,
            "elapsed_ms": o.elapsed.as_millis() as u64,
            "tokens_in": o.tokens_in,
            "tokens_out": o.tokens_out,
        }))
        .into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, e),
    }
}

async fn health(State(s): State<AppState>) -> Response {
    let agent = s.agent.lock().await;
    let engine = agent.client.health_check().await;
    let body = json!({
        "ok": engine.is_ok(),
        "version": env!("CARGO_PKG_VERSION"),
        "provider": agent.config.provider,
        "model": agent.config.model,
        "endpoint": agent.client.endpoint(),
        "context": agent.num_ctx,
        "engine": engine.map_err(|e| e.to_string()),
        "session_id": agent.meta.id,
    });
    let code = if body["ok"].as_bool() == Some(true) {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (code, Json(body)).into_response()
}

async fn sessions(State(s): State<AppState>, headers: HeaderMap) -> Response {
    if !authorized(&s, &headers) {
        return err(StatusCode::UNAUTHORIZED, "missing or invalid bearer token");
    }
    let agent = s.agent.lock().await;
    Json(json!({ "ok": true, "sessions": agent.store.list(100) })).into_response()
}

pub async fn serve(agent: Agent, addr: SocketAddr) -> anyhow::Result<()> {
    let token = std::env::var("QSL_API_TOKEN")
        .ok()
        .filter(|t| !t.trim().is_empty());
    if !addr.ip().is_loopback() && token.is_none() {
        anyhow::bail!(
            "refusing to listen on non-loopback address {addr} without QSL_API_TOKEN set"
        );
    }
    let state = AppState {
        agent: Arc::new(Mutex::new(agent)),
        token,
    };
    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/chat", post(chat))
        .route("/v1/sessions", get(sessions))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!(
        "qsl-agent API listening on http://{addr}  (POST /v1/chat, GET /health, GET /v1/sessions)"
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    state.agent.lock().await.save_session();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_comparison() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }
}
