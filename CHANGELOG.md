# Changelog

## v0.5.0 — Unified build (2026-11-04)

v0.5 merges the working v0.2 build (box-drawing terminal UI, Obsidian memory,
skills, approval prompts, adaptive step budget) with the parts of the v0.4
rewrite that were worth keeping (sessions, streaming, retries, sandboxing,
HTTP mode). It replaces both. The v0.4 tree did not compile; v0.5 builds with
zero warnings under `cargo clippy -D warnings` and ships with 75 unit tests,
16 end-to-end tests against a mock LLM, and a 25-check interactive terminal
test suite (`build-test/`).

### Fixed — v0.4 (HEAD) defects

1. `Cargo.toml` declared a bench (`benches/agent_turn.rs`) that does not exist, so cargo could not even parse the manifest.
2. Module `tools` was defined twice (`src/tools.rs` and `src/tools/mod.rs`), a hard compile error.
3. `src/tools/execute.rs` was a code fragment (no type, no imports) that referenced a non-existent `ctx.approval`.
4. `main.rs` never declared `cli`, `ui`, `skills` or `approval`, so the interactive UI was unreachable dead code.
5. `PrometheusBuilder::with_http_listener` requires a disabled crate feature (compile error), and port 9999 was hard-coded.
6. `health.rs` moved the same `watch::Sender` twice (compile error).
7. The security deny list used a regex lookahead (`(?!tmp)`), which the `regex` crate rejects. Once the crate compiled, this would have panicked on the first shell command.
8. Assistant tool calls were serialised without the OpenAI `type`/`function` wrapper and with object arguments, so servers rejected every request after the first tool call.
9. The streaming tool-call parser ignored `index` and assumed each chunk ended in `}`: parallel calls were lost and arguments containing `}` were cut short.
10. Streaming never requested `stream_options.include_usage`, so token accounting was always zero.
11. `Agent::run` and the stdio transport both read stdin concurrently and stole each other's lines.
12. JSON tracing logs were written to stdout, on top of the UI.
13. Sessions could never be resumed: a new UUID event log was created on every start.
14. The event log's 64 KB `BufWriter` was never flushed (data loss with `panic = "abort"`), and log rotation renamed the file while it was still open.
15. `/retry` popped only the last message and `/undo` truncated the wrong events.
16. Compaction could split tool-call/result pairs; `assert_well_formed` then failed every later turn with a misleading "context overflow".
17. The config had to be `./config.toml` in the working directory, failed if any `allowed_paths` entry was missing, and was incompatible with v0.2 configs.
18. The memory flush task was aborted on drop, losing pending writes.
19. Mutating tools ran concurrently with no approval gate.
20. SIGHUP "reload" logged the new config but never applied it.

### Fixed — v0.2 defects

21. Byte-index string slicing (`&stdout[..10_000]`, `&content[..15_000]`, `&s[..57]`) panicked on multi-byte UTF-8, and `panic = "abort"` killed the process.
22. Command timeouts left the child process tree running.
23. `[SYSTEM DIRECTIVE]` system messages were injected mid-conversation, which many chat templates reject.
24. Ollama was used through `/v1`, which ignores `num_ctx`, so every model silently ran with Ollama's small default context.
25. Path checks were substring-based (bypassable with `..` or symlinks), `list_dir` was unchecked, and Obsidian note paths could escape the vault.
26. `skill_create` accepted names like `../../x`, allowing writes anywhere.
27. `edit_file` silently replaced every occurrence of the target.
28. Approval: Enter meant "allow", `strict` behaved like `ask`, and `[a]` on one shell command approved every future command, including `rm`.
29. Blocking stdin reads ran inside the async runtime.
30. History compaction dropped messages by count and could orphan tool results.

### Fixed — found during v0.5 testing

31. rustyline saved and restored the SIGINT disposition over tokio's handler, so Ctrl+C killed the agent and orphaned running commands. Fixed with rustyline's `signal-hook` mode plus early handler registration.
32. On small context windows, compaction could summarise away the *current* request, and the summary was lost on `--continue`.
33. Streamed text wrapped mid-word.

### Improvements

1. **Live streaming** of tokens (OpenAI SSE and Ollama NDJSON) inside the classic frames, with markdown-aware styling for headings, code fences and quotes, and word-aware wrapping.
2. **Live reasoning display** for `reasoning_content`, `reasoning`, Ollama `thinking` and `<think>` tags (even when split across chunks); `/think on|off`.
3. **Native Ollama `/api/chat`** so `num_ctx` is actually applied; the model's maximum context is read from `/api/show` and respected.
4. **Parallel tool calls**: accumulated by index, and independent read-only calls run concurrently.
5. **Automatic text tool-calling fallback** (Hermes `<tool_call>` protocol) when the engine or model has no native tool support; also parses `<function=…>` and bare or fenced JSON calls.
6. **Lenient argument repair** (trailing commas, code fences, double-encoded JSON, truncation) and **argument-name aliases** (`file_path`, `old_string`, ...), which matter for small models.
7. **Retries** with exponential backoff and jitter for connection errors, 408/429/5xx; automatic `stream_options` compatibility fallback.
8. **Ctrl+C interrupts the turn** and kills the running command's process group; pressing it twice force-quits.
9. **LLM context compaction**: summarises completed turns, never splits tool pairs, keeps the current request verbatim, uses hysteresis, has an extractive fallback, persists the summary in the session; `/compact`.
10. **History repair** for orphaned or interrupted tool calls before every request.
11. **Persistent sessions** (atomic JSONL): `/sessions`, `/resume`, `-C/--continue`, `-r/--resume`, `/title`, `/sessions rm`.
12. **Working `/undo [n]` and `/retry`.**
13. **File checkpoints** before every write or edit, with `/rollback` and `/checkpoints`.
14. **New tools**: `search_files` (regex code search), `find_files` (glob), `todo` (task planner), `web_fetch` (HTML to text), `memory_search`, `memory_forget`, `session_search`.
15. **Sub-agent delegation** (`delegate_task`) with an isolated context and step budget.
16. **Safer `edit_file`**: uniqueness check, `replace_all`, whitespace-tolerant matching, closest-line hints, and echo of the edited region.
17. **Atomic file writes** that preserve permissions; `read_file` line ranges with continuation hints and binary detection; `list_dir` with depth and sizes.
18. **Hardened shell tool**: own process group, `workdir`, per-call timeout, 4 MB capture cap, non-interactive environment (`PAGER=cat`, `GIT_TERMINAL_PROMPT=0`, ...).
19. **Three-tier SecurityGuard**: 22 hard-deny and 25 review rules, line-continuation normalisation, symlink- and `..`-safe path resolution, sensitive-file blocklist, protected system directories, optional `allowed_roots`, and its own config file protected from agent writes.
20. **Approval modes** `ask` / `always` / `strict` / `yolo`; `[d]` denies a tool for the session; Enter defaults to deny; risky commands are always reviewed; headless runs never auto-run risky commands.
21. **Secret redaction** (private keys, API tokens) in displayed tool output, memories and daily logs.
22. **Obsidian memory**: traversal-proof note paths, ranked memory search, duplicate detection, `/forget`, case-insensitive note lookup.
23. **KV-cache-friendly prompting**: the system prompt stays stable for the whole session, so local engines reuse their prompt cache across steps; compact mode for windows of 8K or less.
24. **Project context files** (`AGENTS.md`, `QSL.md`, `.qsl/instructions.md`, `CLAUDE.md`) loaded automatically.
25. **31 built-in skills** (the 18 from v0.2 plus 13 from the v0.4 catalog), project-local `.qsl/skills`, `/use <skill>` pinning, and similar-skill suggestions.
26. **REPL upgrades**: tab completion and inline hints for commands and arguments, multi-line input (`\` or `"""`), `//` escape, persistent history, and new commands (`/cd`, `/tools`, `/usage`, `/export`, `/todo`, `/forget`, ...).
27. **Status line** after every turn: time, steps, tool calls, context fill, tokens, tok/s.
28. **Smarter step budget**: interactive extend / synthesize / stop is retained, with one automatic extension headless, reasoning-aware nudges for empty replies, and a repetition guard that blocks the third identical call.
29. **One-shot CLI**: positional prompt, stdin (`-P -`, pipes appended as context), `--json`, `--quiet`, `--yolo`, `--max-steps`, `--config`, `--no-stream`, `--no-color`, `--list-sessions`, and meaningful exit codes (0 / 1 / 2 / 130).
30. **HTTP API** (`--serve`): `POST /v1/chat`, `GET /health`, `GET /v1/sessions`; bearer token; refuses non-loopback addresses without a token.
31. **Backward-compatible config**: v0.2 files work unchanged; every key has a default; values are clamped; corrupt files are backed up; secrets are masked; saves are atomic; `QSL_*` environment overrides.
32. **Extended health check**: 7 probes (engine, models, context/tool mode, vault, skills, state directory, host tools) with a non-zero exit code on failure.
33. **File logging** to `<state_dir>/qsl-agent.log` (`--verbose`), never to the terminal.
34. **Portable release build**: removed `target-cpu=native` (binaries crashed on older CPUs); fat LTO and stripping are kept, and cold start is about 2 ms.
35. **`build-test/` toolkit**: release build, full test suite, smoke tests against a dual-protocol mock LLM, a pseudo-terminal REPL suite, and an optional live test against your local model.
