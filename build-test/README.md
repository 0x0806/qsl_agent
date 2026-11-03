# build-test

Everything needed to build QSL Agent and verify the result.

| Script | What it does |
|:--|:--|
| `./all.sh` | `build.sh` + `test.sh` + `smoke.sh` |
| `./build.sh` | Optimised release build (LTO, stripped) into `build-test/bin/qsl-agent`; prints size, checksum and cold-start time |
| `./test.sh` | `cargo fmt --check`, `cargo clippy -D warnings`, unit tests and end-to-end tests |
| `./smoke.sh` | Runs the built binary against a scripted mock LLM: one-shot runs, tools, security tiers, sessions, memory, HTTP API, and the interactive REPL through a real pseudo-terminal (`pty_repl.py`) |
| `./live.sh` | Optional: a real tool-using task against your local Ollama / LM Studio model |

Requirements: Rust toolchain, `curl`, `python3` (for the PTY suite).

Every run uses a throwaway `HOME`, so your real config, vault and sessions are
never touched. Logs go to `build-test/out/`.

The mock LLM (`examples/mock_llm.rs`) speaks both the OpenAI-compatible SSE
protocol and Ollama's native NDJSON protocol. You can also run it by hand:

```bash
cargo run --example mock_llm -- 18080
qsl-agent -e http://127.0.0.1:18080/v1 -m mock-coder:7b
```
