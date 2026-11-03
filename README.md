# QSL Agent

<div align="center">

![Rust](https://img.shields.io/badge/Rust-1.89%2B-orange?style=for-the-badge&logo=rust)
![License](https://img.shields.io/badge/License-Source--Available%20(Non--Redistributable)-red?style=for-the-badge)
![Release](https://img.shields.io/badge/Release-v0.5.0-brightgreen?style=for-the-badge)
![Architecture](https://img.shields.io/badge/Architecture-100%25%20Local-purple?style=for-the-badge)
![Security](https://img.shields.io/badge/Security-3--Tier%20Guard-red?style=for-the-badge)
![PKM](https://img.shields.io/badge/PKM-Obsidian%20Native-blueviolet?style=for-the-badge)

**Autonomous, fast, local-first AI agent built natively in Rust.**
*Local inference | Zero cloud accounts | Obsidian long-term memory | Sandboxed tool execution*

Developed by **[Quantum Secular Labs](https://quantumseclabs.com/)**
Repository: **[github.com/0x0806/qsl_agent](https://github.com/0x0806/qsl_agent)**

</div>

---

## Table of Contents

1. [Overview](#overview)
2. [Quick Start](#quick-start)
3. [Key Features](#key-features)
4. [Architecture](#architecture)
5. [Self-Healing ReAct Engine](#self-healing-react-engine)
6. [Terminal UI](#terminal-ui)
7. [Permission Model & SecurityGuard](#permission-model--securityguard)
8. [Obsidian Memory, Sessions & Skills](#obsidian-memory-sessions--skills)
9. [Local AI Providers](#local-ai-providers)
10. [Command-Line Reference](#command-line-reference)
11. [Interactive REPL Commands](#interactive-repl-commands)
12. [Tool Reference](#tool-reference)
13. [HTTP API](#http-api)
14. [Configuration](#configuration)
15. [Building & Testing](#building--testing)
16. [Troubleshooting](#troubleshooting)
17. [About Quantum Secular Labs](#about-quantum-secular-labs)
18. [License](#license)

---

## Overview

QSL Agent is an autonomous agent for Linux and macOS. It talks directly to on-premise inference engines (Ollama, LM Studio, llama.cpp, vLLM, LocalAI or any OpenAI-compatible server) and acts on your machine through a sandboxed set of tools. Long-term memory, daily logs and skills live as plain Markdown in your Obsidian vault.

It compiles to a single native binary (about 10 MB stripped, around 2 ms cold start) with no Python runtime and no telemetry. Prompts and data stay on your machine; the only network traffic is to the inference server you configure, plus the optional `web_fetch` tool, which you can disable.

See [CHANGELOG.md](CHANGELOG.md) for the full v0.5 list of fixes and improvements.

---

## Quick Start

```bash
# 1. Start a local engine and pull a model
ollama serve &
ollama pull qwen2.5-coder:7b

# 2. Build
git clone https://github.com/0x0806/qsl_agent.git && cd qsl_agent
./build-test/build.sh                  # -> build-test/bin/qsl-agent
sudo install -m 755 build-test/bin/qsl-agent /usr/local/bin/qsl-agent

# 3. Run
qsl-agent --health-check               # verify engine, model, vault
qsl-agent                              # interactive REPL
qsl-agent "summarise the README in this folder"   # one-shot task
```

---

## Key Features

- **Live streaming UI**: tokens, reasoning (`<think>`, `reasoning_content`, Ollama `thinking`) and tool activity render live in box-drawing frames, with no emojis.
- **Adaptive ReAct engine**: parallel read-only tool calls, a repetition guard, a step budget with interactive extension, self-healing for truncated tool JSON, and a guaranteed final answer.
- **Works with any local model**: native tool calling where supported, automatic fallback to the Hermes `<tool_call>` text protocol where not, plus argument repair and alias handling for small models.
- **Real context management**: Ollama's native API with a true `num_ctx` (auto-clamped to the model's maximum), LLM summarisation of older turns, and repair of interrupted tool calls.
- **Sessions**: every conversation is saved; resume with `-C` or `/resume`; `/undo`, `/retry`, `/export`; search past sessions.
- **Checkpoints**: every file write or edit is backed up first, so `/rollback` undoes the last turn's file changes.
- **3-tier SecurityGuard**: catastrophic commands are hard-blocked, risky ones always reviewed; paths are symlink- and `..`-safe; secrets are redacted.
- **Obsidian cortex**: profile and memories in the system prompt, ranked memory search, daily journals, wikilinks.
- **31 built-in skills**, plus your own `SKILL.md` playbooks (vault, `~/.config/qsl-agent/skills`, or per-project `.qsl/skills`).
- **Sub-agents**: `delegate_task` hands self-contained research to a fresh-context sub-agent.
- **Ctrl+C stops the current turn** (and its child processes), not the program.
- **Scriptable**: `--json`, `--quiet`, stdin input, exit codes, and an HTTP API (`--serve`).

---

## Architecture

```
┌──────────────────────────────────────────────────────────────────────────┐
│  CLI / REPL (rustyline: history, completion, multi-line)  |  HTTP API      │
└──────────────────────────────────┬───────────────────────────────────────┘
                                   │
                        ┌──────────▼──────────┐       ┌──────────────────────┐
                        │  Adaptive ReAct Loop │◄─────►│ Context Manager       │
                        │  (agent.rs)          │       │ compaction, repair    │
                        └─────┬──────────┬────┘       └──────────────────────┘
                              │          │
            ┌─────────────────▼──┐   ┌───▼──────────────────────┐
            │ LLM Client          │   │ Approval Gate             │
            │ OpenAI SSE / Ollama │   │ ask | always | strict |   │
            │ native NDJSON,      │   │ yolo  [y][a][n][d]        │
            │ retries, fallback   │   └───┬──────────────────────┘
            └─────────────────────┘       │
                                  ┌───────▼─────────────────────┐
                                  │ SecurityGuard (deny / ask)  │
                                  │ path resolution, redaction  │
                                  └───────┬─────────────────────┘
          ┌──────────────────────┬────────┴──────────┬──────────────────────┐
┌─────────▼────────┐ ┌───────────▼───────┐ ┌─────────▼────────┐ ┌───────────▼──────┐
│ Shell & FS tools │ │ Obsidian memory   │ │ Skills (31+)     │ │ Sessions,        │
│ + checkpoints    │ │ + daily journal   │ │ SKILL.md         │ │ todo, sub-agents │
└──────────────────┘ └───────────────────┘ └──────────────────┘ └──────────────────┘
```

| Module | Responsibility |
|:--|:--|
| `agent.rs` | ReAct loop, tool orchestration, compaction, sub-agents |
| `provider.rs` | OpenAI-compatible and Ollama-native clients, streaming parsers, retries |
| `tools/` | Tool schemas and implementations (`fs`, `shell`, `web`) |
| `security.rs` / `approval.rs` | Command classification, path policy, permission prompts |
| `memory.rs` / `skills.rs` / `session.rs` / `checkpoint.rs` | Persistence layers |
| `ui.rs` / `cli.rs` / `server.rs` | Terminal UI, REPL, HTTP API |

---

## Self-Healing ReAct Engine

- **Step budget** (`max_react_steps`, default 35). Two steps before the limit, the model is told to wrap up. At the limit, interactive sessions get a menu:
  ```text
  ╭── [STEP LIMIT REACHED] ─────────────────────────────────────────
  │  Agent reached its step budget (35/35 steps).
  ├─────────────────────────────────────────────────────────────────
  │  [c] Extend (+15 steps)  │  [f] Force synthesis  │  [s] Stop turn
  ```
  Headless runs extend once automatically, then force a tool-free synthesis, so you always get an answer.
- **Repetition guard**: the second identical call gets a warning appended to its result; the third is blocked.
- **Truncated tool JSON** (output limit hit mid-call): the agent asks the model to continue in smaller pieces (`append_file`).
- **Empty or reasoning-only replies**: a targeted nudge asks for the visible answer; as a last resort the reasoning itself is returned.
- **Context overflow** reported by the engine: compact, then retry once.
- **Interrupted turns** (Ctrl+C): partial results stay in context and dangling tool calls are repaired automatically.

---

## Terminal UI

The UI uses Unicode box drawing only. Every frame adapts to the terminal width, and colours respect `NO_COLOR` / `--no-color`.

```text
╭── [REASONING & COGNITIVE PLANNING] ───────────────────────────────────
│  The user wants the .txt files in proj/. I'll locate them with
│  find_files, then read each one.
╰───────────────────────────────────────────────────────────────────────
╭── [ACTION: find_files] ───────────────────────────────────────────────
│  pattern:     *.txt
╰───────────────────────────────────────────────────────────────────────
╭── [RESULT: find_files] [OK] [0.0s] ───────────────────────────────────
│  2 file(s):
╰───────────────────────────────────────────────────────────────────────
╭── [RESPONSE] ─────────────────────────────────────────────────────────
│  There are 2 .txt files: a.txt (alpha) and b.txt (beta).
╰───────────────────────────────────────────────────────────────────────
  [31.5s | 4 steps | 3 tools | ctx 4.0k/16.4k 25% | in 15.7k out 356 (11 tok/s)]
```

---

## Permission Model & SecurityGuard

### Approval modes (`-a`, `/approval`, `approval_mode`)

| Mode | Normal mutating tools (write, edit, shell, ...) | Risky commands (rm, sudo, git push, installs, kill, ...) |
|:--|:--|:--|
| `ask` *(default)* | Prompt `[y]` once, `[a]` session, `[n]` deny, `[d]` deny for the session | Prompt every time (`[a]` not offered) |
| `always` | Automatic | Prompt every time |
| `strict` | Prompt every time | Prompt every time |
| `yolo` | Automatic | Automatic (**sandboxes only**) |

Read-only tools never prompt. Memory appends and skill creation write only into the agent's own vault area and don't prompt in `ask` mode. Pressing **Enter means deny**. In non-interactive runs (`-P`, `--serve`), risky commands and everything under `strict` are denied with an explanation the model can act on.

```text
╭── [PERMISSION REVIEW] ────────────────────────────────────────────────
│  Tool:      execute_command
│  Command:   rm build/old.log
│  Risk:      deletes files
├───────────────────────────────────────────────────────────────────────
│  [y] Allow once  │  [n] Deny  │  [d] Deny (session)
╰───────────────────────────────────────────────────────────────────────
  permission [y/a/n/d] (default n) >
```

### SecurityGuard

- **Hard-blocked in every mode**: recursive deletion of `/`, `~`, `.` or `*`; `mkfs`; partition editors; raw writes to block devices; fork bombs; `curl|sh`-style remote execution; base64-decoded payloads piped to a shell; `/dev/tcp` reverse shells; reading `/etc/shadow` or SSH private keys; power-state changes; firewall flushes; erasing shell history.
- **Paths are resolved** (normalised, symlinks followed) before checking. Sensitive files (`/etc/shadow`, SSH keys, `~/.aws/credentials`, `~/.kube/config`, `~/.netrc`, `~/.gnupg/`, ...) cannot be read; system directories (`/etc`, `/usr`, `/boot`, `/sys`, ...) cannot be written. The agent cannot rewrite its own config file. Set `allowed_roots` to confine file tools to specific directories.
- **Commands** run in their own process group with a timeout. A timeout or Ctrl+C kills the whole tree.
- **Secrets** (private keys, `sk-`, `ghp_`, `AKIA`, Slack and Google tokens) are redacted from displayed output, memories and journals.

---

## Obsidian Memory, Sessions & Skills

```
Your_Obsidian_Vault/QSL_Agent/
├── Profile.md        # identity, user and host profile (loaded into every session)
├── Memories.md       # one fact per bullet (recent facts in the prompt, older ones via memory_search)
├── Daily/2026-11-04.md   # journal of every turn
├── Architecture/  Security/  Research/
└── Skills/<name>/SKILL.md   # 31 seeded skills plus the ones the agent creates
```

- **Vault discovery**: `$OBSIDIAN_VAULT`, the Obsidian app registry (including Flatpak), `~/Documents/*`, `~/Obsidian`, `~/Notes`, `~/Vault`, home sub-folders, and the current directory. `/vault auto` re-detects.
- **Memory**: the agent saves durable facts with `obsidian_append` (duplicates skipped) and can search (`memory_search`) or correct (`memory_forget`) them. Use `/remember`, `/forget` and `/memory [query]` yourself.
- **Sessions** are stored in `~/.local/state/qsl-agent/sessions/`. Use `qsl-agent -C` to continue the last one, `/sessions` and `/resume 3` to switch, and `/export md`. The agent can search past sessions with `session_search`.
- **Skills** use progressive disclosure: only names and descriptions go into the prompt, and full procedures load via `skill_read`. Pin one with `/use rust-cargo`. Project-local skills in `.qsl/skills/` override vault copies.
- **Project instructions**: `AGENTS.md`, `QSL.md`, `.qsl/instructions.md` or `CLAUDE.md` in the working directory are loaded automatically.

Built-in skills: `api-design`, `benchmark`, `code-review`, `codebase-inspection`, `data-analysis`, `data-transform`, `database-sql`, `debug`, `dependency-update`, `doc-writer`, `docker-container`, `explain-code`, `file-organizer`, `git-workflow`, `incident-response`, `markdown-docs`, `migration`, `network-diagnostics`, `obsidian-vault-manager`, `python-env`, `refactor`, `rust-cargo`, `security-audit`, `shell-script`, `software-development`, `sql-query`, `system-ops`, `test-writer`, `text-processing`, `web-research`, `yaml-toml-config`.

---

## Local AI Providers

| Provider | Default endpoint | Protocol |
|:--|:--|:--|
| `ollama` *(default)* | `http://127.0.0.1:11434` | Native `/api/chat` (applies `num_ctx`, streams `thinking`) |
| `lmstudio` | `http://127.0.0.1:1234/v1` | OpenAI-compatible SSE |
| `custom` | `-e URL` / `custom_url` | OpenAI-compatible SSE (llama.cpp, vLLM, LocalAI, OpenRouter, ...) |

```bash
qsl-agent -p lmstudio
qsl-agent -e http://127.0.0.1:8080/v1 -m my-model            # llama-server
QSL_API_KEY=sk-... qsl-agent -e https://openrouter.ai/api/v1 -m qwen/qwen3-coder
```

If `model` is empty, the first chat model on the engine is used (embedding models are skipped). Models without native tool calling (e.g. templates that report only the `completion` capability) automatically switch to the `<tool_call>` text protocol. Switch at runtime with `/models`, `/model <name>` and `/provider <name> [url]`.

---

## Command-Line Reference

```text
Usage: qsl-agent [OPTIONS] [PROMPT]...

  [PROMPT]...                 One-shot task (same as -P)
  -P, --prompt <PROMPT>       Non-interactive prompt ("-" reads stdin)
  -m, --model <MODEL>         Model name
  -p, --provider <PROVIDER>   ollama | lmstudio | custom
  -e, --endpoint <URL>        Custom OpenAI-compatible endpoint (implies custom)
      --api-key <KEY>         Bearer token for remote endpoints [env: QSL_API_KEY]
  -v, --vault <PATH>          Obsidian vault
  -a, --approval <MODE>       ask | always | strict | yolo
      --yolo                  Same as --approval yolo
  -t, --temperature <T>       0.0 - 2.0
  -c, --context-size <N>      Context window (tokens)
      --max-steps <N>         ReAct steps per turn
      --config <PATH>         Config file [env: QSL_CONFIG]
  -C, --continue              Continue the most recent session
  -r, --resume <ID>           Resume a session (id, prefix or list number)
      --no-stream             Disable token streaming
      --no-color              Disable colours
  -q, --quiet                 One-shot: print only the final answer
      --json                  One-shot: print a JSON result
      --serve [ADDR]          HTTP API (default 127.0.0.1:8765)
      --list-models / --list-sessions / --show-config / --health-check
      --verbose               Debug log to <state_dir>/qsl-agent.log
```

```bash
qsl-agent "Audit src/ for security issues and record findings in Obsidian"
git diff | qsl-agent -q "write a conventional commit message for this diff"
qsl-agent --json -a strict "list TODOs in this repo" | jq -r .reply
```

Exit codes: `0` success, `1` agent or engine error, `2` usage error, `130` interrupted.

---

## Interactive REPL Commands

| Command | Description |
|:--|:--|
| `/help` | Command manual |
| `/health` | Engine, model, vault, skills, state and host-tool diagnostics |
| `/models`, `/model <name>` | List or switch models (the conversation is kept) |
| `/provider <p> [url]` | Switch engine |
| `/approval <mode>` (`/mode`) | `ask` \| `always` \| `strict` \| `yolo` |
| `/vault [path\|auto]` | Show or switch the Obsidian vault |
| `/memory [query]`, `/remember <fact>`, `/forget <text>` | Long-term memory |
| `/skills`, `/skill <name>`, `/use <name\|off>` | Skill catalog, instructions, pinning |
| `/todo` | Current task plan |
| `/sessions [rm <id>]`, `/resume <id\|#>`, `/new` (`/clear`), `/title <t>` | Sessions |
| `/undo [n]`, `/retry` | Rewind turns or re-run the last prompt |
| `/rollback`, `/checkpoints` | Undo the last turn's file changes |
| `/compact` | Summarise older context now |
| `/export [md\|json] [path]` | Export the conversation |
| `/usage` (`/stats`) | Token and session statistics |
| `/think on\|off`, `/stream on\|off` | Display toggles |
| `/tools`, `/cd <dir>`, `/config`, `/exit` | Misc |

Input: end a line with `\` or wrap text in `"""` for multi-line prompts; start with `//` to send a literal `/`; Tab completes commands and arguments; **Ctrl+C interrupts the running turn** (press twice to force quit).

---

## Tool Reference

| Tool | Class | Description |
|:--|:--|:--|
| `execute_command` | mutating | Shell command (`command`, optional `workdir`, `timeout`) |
| `read_file` | read | Text file, optional `start_line` / `end_line` |
| `write_file` | mutating | Create or overwrite (atomic, checkpointed) |
| `append_file` | mutating | Append (for large files) |
| `edit_file` | mutating | Unique exact replacement (`target` → `replacement`, `replace_all`) |
| `list_dir` | read | Directory listing with sizes (`depth` 1-3) |
| `search_files` | read | Regex search across files (`pattern`, `path`, `glob`, `ignore_case`) |
| `find_files` | read | Glob file search |
| `todo` | read | Task plan for multi-step work |
| `obsidian_search` / `obsidian_read` / `obsidian_list` | read | Vault access |
| `obsidian_write` | mutating | Create, overwrite or append a note |
| `obsidian_append` | mutating* | Save a long-term memory |
| `memory_search` / `memory_forget` | read / mutating | Query or correct memories |
| `skill_list` / `skill_read` / `skill_create` | read / read / mutating* | Skills |
| `session_search` | read | Search past conversations |
| `web_fetch` | read | HTTP GET with HTML-to-text (`allow_web = false` disables it) |
| `delegate_task` | read | Run a sub-agent on a self-contained subtask |

\* Low-risk writes into the agent's own vault area; no prompt in `ask` mode.

---

## HTTP API

```bash
QSL_API_TOKEN=change-me qsl-agent --serve 127.0.0.1:8765
curl -s localhost:8765/v1/chat -H 'authorization: Bearer change-me' \
     -H 'content-type: application/json' -d '{"message":"what changed in git today?"}'
```

| Endpoint | Body / result |
|:--|:--|
| `POST /v1/chat` | `{"message": "...", "session_id"?: "...", "new_session"?: false}` → `{"ok", "reply", "session_id", "steps", "tool_calls", "elapsed_ms", "tokens_in", "tokens_out"}` |
| `GET /health` | Engine status, model, context size |
| `GET /v1/sessions` | Saved sessions |

Requests are processed one at a time and run non-interactively. Binding to a non-loopback address requires `QSL_API_TOKEN`. A systemd user unit is provided in `systemd/qsl-agent.service`.

---

## Configuration

The config lives in `~/.config/qsl-agent/config.toml` (or `--config` / `$QSL_CONFIG`). It is created on first run; every key is optional, and v0.2 config files keep working. See [`config.example.toml`](config.example.toml) for all keys with comments. The most important ones:

```toml
provider = "ollama"            # ollama | lmstudio | custom
model = ""                     # empty = auto-detect
context_size = 16384           # num_ctx, clamped to the model's maximum
max_tokens = 4096              # reply budget (capped at 1/4 of the context)
approval_mode = "ask"          # ask | always | strict | yolo
max_react_steps = 35
command_timeout_secs = 60
allowed_roots = []             # confine file tools, e.g. ["~/projects"]
allow_web = true
stream = true
show_reasoning = true
```

Environment overrides: `QSL_PROVIDER`, `QSL_MODEL`, `QSL_ENDPOINT`, `QSL_API_KEY`, `QSL_APPROVAL`, `QSL_CONFIG`, `OBSIDIAN_VAULT`, `NO_COLOR`. An invalid config is backed up to `config.toml.bak` and replaced with defaults, with a warning showing the parse error.

---

## Building & Testing

```bash
./build-test/all.sh        # release build + fmt + clippy + tests + smoke tests
./build-test/build.sh      # optimised, stripped binary -> build-test/bin/qsl-agent
./build-test/test.sh       # cargo fmt --check, clippy -D warnings, unit + e2e tests
./build-test/smoke.sh      # built binary vs. a mock LLM, incl. interactive PTY suite
./build-test/live.sh       # optional: a real task against your local model
```

The test suites never touch your real config, vault or sessions; each run uses a throwaway `HOME`. The mock LLM (`cargo run --example mock_llm`) speaks both the OpenAI SSE and Ollama NDJSON protocols. See [build-test/README.md](build-test/README.md).

Requirements: Rust 1.89+, plus `curl` and `python3` for the smoke tests. The `target/` directory is only a build cache; the resulting binary is self-contained.

---

## Troubleshooting

| Symptom | Fix |
|:--|:--|
| `cannot reach local AI engine` | Start the engine (`ollama serve`, LM Studio → Start Server) and run `qsl-agent --health-check` |
| `no model configured and none detected` | `ollama pull qwen2.5-coder:7b`, or `/model <name>` |
| Model ignores tools / prints JSON | Handled automatically (text protocol fallback). Prefer tool-capable models such as Qwen 2.5/3 Coder or Llama 3.1+ |
| Answers forget earlier context | Raise `context_size` (the engine must have the VRAM); check the `ctx` value in the status line |
| `[SECURITY BLOCKED]` | The command matched a hard-deny rule; run it yourself if you really mean it |
| `DENIED ... non-interactive mode` | Risky commands need a human; run interactively, or use `--yolo` in a sandbox |
| Slow first token | Large prompt processing on small GPUs; the stable system prompt lets the engine cache it after the first step |
| Step limit reached | `[c]` adds 15 steps; or raise `max_react_steps` / `--max-steps` |
| Need details | `qsl-agent --verbose`, then read `~/.local/state/qsl-agent/qsl-agent.log` |

---

## About Quantum Secular Labs

**[Quantum Secular Labs](https://quantumseclabs.com/)** (QuantumSec Labs) is a specialized cybersecurity and technology engineering firm focused on digital infrastructure defense, enterprise vulnerability assessment and penetration testing (VAPT), offensive security auditing, and applied Artificial Intelligence research.

QSL engineered **QSL Agent** to directly eliminate the security vulnerabilities, secret leakage, and compliance hazards inherent in cloud-tethered AI tools. By uniting defensive cybersecurity principles with native Rust systems engineering, QSL Agent provides developers, security auditors, and enterprises with an autonomous, sovereign local intelligence engine purpose-built for zero-trust environments.

---

## License

QSL Agent is released under the **[QSL Team Source-Available License v1.0 (2026)](LICENSE)** by **[Quantum Secular Labs](https://quantumseclabs.com/)**.

- **Source-Available**: You may view, inspect, audit, compile, and run the software locally on your own hardware for personal, academic, or internal operational use.
- **Strictly Non-Redistributable**: You may **NOT** redistribute, resell, mirror, syndicate, sub-license, fork publicly, or distribute pre-compiled binaries or packages to any third party without explicit prior written authorization from Quantum Secular Labs.
- **Commercial & Cloud Exclusivity**: Providing QSL Agent as a hosted service (SaaS/PaaS/API wrapper) or commercial redistribution is reserved exclusively to **[Quantum Secular Labs](https://quantumseclabs.com/)**.
- **All Rights Reserved**: All copyrights, patents, and distribution rights remain solely with Quantum Secular Labs.
