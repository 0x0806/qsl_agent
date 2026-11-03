use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::{get, post}, Json, Router};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::agent::AgentHandle;
use crate::error::Result;
use crate::session_registry::SessionRegistry;

#[derive(Clone, Debug)]
pub enum Transport {
    Stdio,
    Unix(PathBuf),
    Http(SocketAddr),
}

#[derive(Debug, Deserialize)]
struct ChatReq { text: String }

#[derive(Debug, Serialize)]
struct ChatResp { ok: bool, reply: Option<String>, error: Option<String> }

pub async fn serve(
    transport: Transport,
    agent: AgentHandle,
    registry: Arc<SessionRegistry>,
    shutdown: CancellationToken,
) -> Result<()> {
    match transport {
        Transport::Stdio => serve_stdio(agent, shutdown).await,
        Transport::Unix(p) => serve_unix(p, agent, shutdown).await,
        Transport::Http(addr) => serve_http(addr, agent, registry, shutdown).await,
    }
}

async fn serve_stdio(agent: AgentHandle, shutdown: CancellationToken) -> Result<()> {
    use tokio::io::stdin;
    let mut lines = BufReader::new(stdin()).lines();
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
            line = lines.next_line() => {
                let Some(line) = line? else { break };
                let line = line.trim();
                if line.is_empty() { continue; }
                if line == "/quit" { break; }
                match agent.chat(line.to_string()).await {
                    Ok(reply) => println!("{reply}"),
                    Err(e) => eprintln!("error: {e}"),
                }
            }
        }
    }
    Ok(())
}

async fn serve_unix(path: PathBuf, agent: AgentHandle, shutdown: CancellationToken) -> Result<()> {
    let _ = tokio::fs::remove_file(&path).await;
    let listener = tokio::net::UnixListener::bind(&path)?;
    info!(path = %path.display(), "unix socket listening");
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
            accept = listener.accept() => {
                let Ok((stream, _)) = accept else { continue };
                let agent = agent.clone();
                tokio::spawn(async move {
                    let (rd, mut wr) = stream.into_split();
                    let mut lines = BufReader::new(rd).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let line = line.trim();
                        if line.is_empty() { continue; }
                        let resp = match agent.chat(line.to_string()).await {
                            Ok(r) => serde_json::to_string(&ChatResp { ok: true, reply: Some(r), error: None }).unwrap(),
                            Err(e) => serde_json::to_string(&ChatResp { ok: false, reply: None, error: Some(e.to_string()) }).unwrap(),
                        };
                        if wr.write_all(resp.as_bytes()).await.is_err() { break; }
                        if wr.write_all(b"\n").await.is_err() { break; }
                    }
                });
            }
        }
    }
    let _ = tokio::fs::remove_file(&path).await;
    Ok(())
}

async fn serve_http(
    addr: SocketAddr,
    agent: AgentHandle,
    registry: Arc<SessionRegistry>,
    shutdown: CancellationToken,
) -> Result<()> {
    let app = Router::new()
        .route("/chat", post(http_chat))
        .route("/health", get(http_health))
        .route("/stats", get(http_stats))
        .route("/sessions", get(http_sessions))
        .with_state(HttpState { agent, registry });

    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!(%addr, "http listening");
    // ⚠️ VERIFY: axum 0.7 builder chain
    axum::serve(listener, app)
        .with_graceful_shutdown(async move { shutdown.cancelled().await })
        .await
        .map_err(|e| crate::error::AgentError::Io(std::io::Error::other(e.to_string())))?;
    Ok(())
}

#[derive(Clone)]
struct HttpState { agent: AgentHandle, registry: Arc<SessionRegistry> }

async fn http_chat(State(s): State<HttpState>, Json(req): Json<ChatReq>) -> impl IntoResponse {
    match s.agent.chat(req.text).await {
        Ok(r) => (StatusCode::OK, Json(ChatResp { ok: true, reply: Some(r), error: None })),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ChatResp { ok: false, reply: None, error: Some(e.to_string()) })),
    }
}

async fn http_health(State(s): State<HttpState>) -> impl IntoResponse {
    match s.agent.health().await {
        Ok(h) => (StatusCode::OK, Json(serde_json::to_value(h).unwrap_or_default())),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))),
    }
}

async fn http_stats(State(s): State<HttpState>) -> impl IntoResponse {
    match s.agent.stats().await {
        Ok(st) => (StatusCode::OK, Json(serde_json::to_value(st).unwrap_or_default())),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))),
    }
}

async fn http_sessions(State(s): State<HttpState>) -> impl IntoResponse {
    match s.registry.list().await {
        Ok(list) => (StatusCode::OK, Json(serde_json::to_value(list).unwrap_or_default())),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))),
    }
}

#[allow(dead_code)]
fn _warn_use() { warn!("transport module"); }
