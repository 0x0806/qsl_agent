//! Standalone scripted mock inference server for smoke tests and demos.
//!
//!   cargo run --example mock_llm -- 18080
//!
//! Serves both `/v1/chat/completions` (OpenAI SSE) and `/api/chat` (Ollama).
//! Put `[mock:write] /abs/path`, `[mock:shell]`, `[mock:think]`, ... in the
//! prompt to trigger scripted behaviours (see tests/common/mock_llm.rs).

#[path = "../tests/common/mock_llm.rs"]
mod mock_llm;

#[tokio::main]
async fn main() {
    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(18080);
    let (port, _state) = mock_llm::start(port).await;
    println!(
        "mock LLM listening on http://127.0.0.1:{port} (model: {})",
        mock_llm::MODEL
    );
    let _ = tokio::signal::ctrl_c().await;
}
