# QSL Agent 

<div align="center">

![Rust](https://img.shields.io/badge/Rust-1.80%2B-orange?style=for-the-badge&logo=rust)
![License](https://img.shields.io/badge/License-Source--Available%20(Non--Redistributable)-red?style=for-the-badge)
![Release](https://img.shields.io/badge/Release-v0.2.0%20(Build%202026.11)-brightgreen?style=for-the-badge)
![Binary Size](https://img.shields.io/badge/Binary-4.5MB%20Stripped-blue?style=for-the-badge)
![Architecture](https://img.shields.io/badge/Architecture-100%25%20Local-purple?style=for-the-badge)
![Security](https://img.shields.io/badge/Security-Multi--Tier%20Guard-red?style=for-the-badge)
![PKM](https://img.shields.io/badge/PKM-Obsidian%20Native-blueviolet?style=for-the-badge)

**Autonomous, rapid local intelligence agent engine built natively in Rust.**  
*100% Local Inference | Zero Cloud Accounts | Obsidian PKM Long-Term Memory | Sandboxed Execution Engine*

Developed by **[Quantum Secular Labs](https://quantumseclabs.com/)**  
Repository: **[github.com/0x0806/qsl_agent](https://github.com/0x0806/qsl_agent)**

[Overview](#overview) • [Architecture](#system-architecture--react-loop-design) • [Binary Compilation](#binary-compilation--distribution-guide) • [ReAct Budget](#self-healing-react-engine--dynamic-step-budget) • [Enterprise UI](#enterprise-cli-ui--cognitive-reasoning-indications) • [Skills (18 Seeded)](#seeded-production-skills-catalog-18-skills) • [Security & Approvals](#multi-tier-permission-model--securityguard-sandbox) • [CLI Reference](#command-line-reference--flags) • [About QSL](#about-quantum-secular-labs)

</div>

---

## Table of Contents

1. [Overview](#overview)
2. [Architectural Comparison: QSL Agent vs Traditional Python Stacks](#architectural-comparison-qsl-agent-vs-traditional-python-stacks)
3. [Key Enterprise Features](#key-enterprise-features)
4. [System Architecture & ReAct Loop Design](#system-architecture--react-loop-design)
5. [Self-Healing ReAct Engine & Dynamic Step Budget](#self-healing-react-engine--dynamic-step-budget)
6. [Enterprise CLI UI & Cognitive Reasoning Indications](#enterprise-cli-ui--cognitive-reasoning-indications)
7. [Multi-Tier Permission Model & SecurityGuard Sandbox](#multi-tier-permission-model--securityguard-sandbox)
8. [Obsidian-Backed PKM Cortex & Long-Term Memory](#obsidian-backed-pkm-cortex--long-term-memory)
9. [Seeded Production Skills Catalog (18 Skills)](#seeded-production-skills-catalog-18-skills)
10. [Local AI Provider Integration & Dynamic Discovery](#local-ai-provider-integration--dynamic-discovery)
11. [Binary Compilation & Distribution Guide](#binary-compilation--distribution-guide)
12. [Command-Line Reference & Flags](#command-line-reference--flags)
13. [Interactive Terminal REPL & Slash Commands](#interactive-terminal-repl--slash-commands)
14. [Deterministic Tool Reference (13 Built-in Tools)](#deterministic-tool-reference-13-built-in-tools)
15. [Configuration Reference (`config.toml`)](#configuration-reference-configtoml)
16. [Troubleshooting & Diagnostics](#troubleshooting--diagnostics)
17. [About Quantum Secular Labs](#about-quantum-secular-labs)
18. [License & Intellectual Property](#license--intellectual-property)

---

## Overview

QSL Agent is an enterprise-grade autonomous systems agent engineered in native Rust for modern Linux and macOS environments. It communicates directly with on-premise local inference engines—including Ollama, LM Studio, and standard OpenAI-compatible endpoints—without transmitting prompts, data payloads, or runtime telemetry to external cloud infrastructure.

Unlike conventional agent frameworks that rely on large Python runtimes, interpreter overhead, and extensive dependency trees, QSL Agent compiles into an immutable, standalone **4.5 MB stripped binary**. It integrates natively with your local Obsidian vault as a bidirectional Personal Knowledge Management (PKM) cortex, ensuring all memories, operational playbooks, user preferences, and daily logs remain human-readable Markdown files stored securely on your local storage.

---

## Architectural Comparison: QSL Agent vs Traditional Python Stacks

Empirical measurements gathered on x86_64 Linux executing multi-step autonomous development tasks (local Ollama backend, Qwen 2.5 / 3.5 models):

| Metric | Traditional Python Stack (Hermes / AutoGPT) | QSL Agent (Rust v0.2.0) | Performance Advantage |
|:---|:---:|:---:|:---|
| **Cold Start Latency** | 450ms – 2,200ms (interpreter + import chain) | **< 3ms** (compiled native ELF binary) | **>150x faster startup** |
| **Idle Memory Footprint** | 180MB – 420MB (runtime + pip wheels) | **~15MB** (zero garbage collector overhead) | **~94% memory reduction** |
| **Distribution Footprint** | Multi-GB (virtualenv, site-packages, torch/pip) | **4.5MB Single Stripped Executable** | **Self-contained deployment** |
| **Runtime Dependencies** | Python 3.10+, pip, build toolchains | **None** (statically linked binary) | **Zero environment drift** |
| **Telemetry & Cloud Auth**| Mandatory remote API keys or portal sign-ins | **Zero** (100% On-Premise Air-Gapped) | **Absolute Data Sovereignty** |
| **Persistent Memory Format**| Proprietary vector databases / cloud sync | **Obsidian Markdown (`[[wikilinks]]`)** | **Interoperable, Git-trackable files** |
| **Step Budget Handling** | Hard failure after N steps with unhandled crash | **Dynamic Self-Healing & Synthesis Fallback** | **Resilient turn completion** |
| **Safety Interception** | Post-exception handler / unconstrained shell | **Pre-execution Regex Sandbox + 3-Tier Gate** | **Deterministic command filtering** |

---

## Key Enterprise Features

- **Sub-Millisecond Native Execution**: Immediate cold start with low resident memory overhead.
- **Dynamic Local Model Discovery**: Auto-detects active models from local engines (`/api/tags` and `/v1/models`) without hardcoded model identifiers.
- **Dynamic Obsidian Vault Discovery**: Recursively detects existing Obsidian vaults via environment variables (`OBSIDIAN_VAULT`), the official app registry (`obsidian.json`), and document directories without hardcoded paths.
- **Cognitive Reasoning & Thought Display**: Automatically detects and extracts `<think>...</think>` blocks emitted by modern reasoning models (DeepSeek-R1, Qwen 2.5 / 3.5, HuiHui), rendering them inside an executive reasoning frame.
- **Structured Tool Badges (No Raw JSON Walls)**: Parses and formats tool arguments into structured metadata badges (target path, byte count, line count preview, diff changes) instead of dumping escaped JSON strings.
- **Self-Healing ReAct Engine**: Adaptive step budget management with loop repetition detection, approaching-limit warnings, interactive extension dialogs, and automated synthesis fallback.
- **Executive Response Presentation**: Frames completed turn output in high-contrast response cards with zero emoji clutter.
- **Air-Gapped Privacy Architecture**: Zero network telemetry, zero cloud authentication tokens, and zero external data transmission.
- **3-Tier Permission Manager**: Granular runtime authorization (`[y]` allow once, `[a]` always allow for session, `[n]` deny) for mutating operations.
- **SecurityGuard Sandbox**: Real-time regex inspection preventing destructive shell commands (`rm -rf`, raw device writes, system partition tampering) and private key leakage.
- **18 Seeded Production Skills**: Pre-packaged operational guidelines covering codebase inspection, software development, security audits, Docker, SQL, Git, and Linux administration.

---

## System Architecture & ReAct Loop Design

```
┌────────────────────────────────────────────────────────────────────────┐
│                        QSL Agent CLI (Terminal)                        │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │
                         ┌──────────▼──────────┐
                         │   CLI & UI Engine   │
                         │ (Box-drawing Frames)│
                         └──────────┬──────────┘
                                    │
                         ┌──────────▼──────────┐
                         │  Adaptive ReAct Loop│
                         │ (Dynamic Controller)│
                         └─────┬───────────┬───┘
                               │           │
        ┌───────────────────────┴──┐     ┌──┴─────────────────────────┐
        │   Local AI Provider      │     │  Interactive Approval Gate │
        │  (Ollama / LM Studio)    │     │   [y] Once | [a] Always    │
        │  Auto-Detect & GGUF      │     │   [n] Deny Session         │
        └──────────────────────────┘     └──┬─────────────────────────┘
                                            │
                         ┌─────────────────▼─────────────────┐
                         │      SecurityGuard Sandbox        │
                         │  (Regex Destructive Interceptor)  │
                         └─────────────────┬─────────────────┘
                                           │
                 ┌─────────────────────────┼─────────────────────────┐
                 │                         │                         │
      ┌──────────▼──────────┐   ┌──────────▼──────────┐   ┌──────────▼──────────┐
      │   OS Tools Engine   │   │ Obsidian PKM Memory │   │  18 Domain Skills   │
      │ (exec, read, write) │   │ (Memories, Profile) │   │ (SKILL.md Workflows)│
      └─────────────────────┘   └─────────────────────┘   └─────────────────────┘
```

---

## Self-Healing ReAct Engine & Dynamic Step Budget

Traditional autonomous agents frequently fail when models take more reasoning steps than anticipated or get stuck in repetitive action loops, producing fatal errors such as:
`Agent Error: Agent exceeded maximum ReAct steps (20) for this turn.`

QSL Agent resolves this through a multi-tier adaptive step controller:

### 1. Increased Operational Default
The default `max_react_steps` budget is set to **35** (configured in both `src/config.rs` and `~/.config/qsl-agent/config.toml`), accommodating multi-phase codebase audits and refactoring tasks.

### 2. Repetition & Loop Interception
The engine continuously registers executed tool signatures `(tool_name:arguments)`. If an identical tool call is attempted consecutively, the engine injects a system directive:
```text
[SYSTEM DIRECTIVE] Repeated tool action detected for 'write_file'. You have already
executed this action with identical parameters. Do NOT repeat it. Conclude this task
or transition to the next distinct action now.
```
This breaks model hallucinations and prevents infinite spinning on duplicate commands.

### 3. Approaching-Budget Warning
When `max_steps - 3` is reached, QSL Agent injects an early notice into the model context:
```text
[SYSTEM DIRECTIVE] Approaching step budget limit (step 32 of 35). Conclude remaining
tool operations immediately and synthesize your final comprehensive response.
```

### 4. Interactive Budget Extension Modal
If an interactive turn reaches the step ceiling, QSL Agent pauses and presents an interactive resolution menu:
```text
╭── [STEP LIMIT REACHED] ───────────────────────────────────────────╮
│  Agent reached step budget limit (35/35 steps).                   │
├───────────────────────────────────────────────────────────────────┤
│  [c] Extend (+15 steps)  │  [f] Force synthesis  │  [s] Stop turn    │
╰───────────────────────────────────────────────────────────────────╯
  action [c/f/s] > 
```
- **`[c]` Extend**: Dynamically grants +15 additional ReAct steps to complete complex execution trees.
- **`[f]` Force Synthesis**: Requests an immediate conclusion without tools, prompting the model to summarize all findings gathered so far.
- **`[s]` Stop Turn**: Cleanly halts execution and preserves context without crashing.

### 5. Automated Non-Interactive Fallback (`-P`)
In headless script or CI mode (`-P`), QSL Agent automatically extends the budget once up to 50 steps. If the step limit is reached, it automatically invokes a zero-tool synthesis completion turn. The user **always receives a completed response** instead of an unhandled exception.

---

## Enterprise CLI UI & Cognitive Reasoning Indications

QSL Agent features a minimal, high-contrast terminal user interface built strictly with Unicode box-drawing characters (`╭`, `─`, `╮`, `│`, `╰`, `╯`, `├`, `┤`). In compliance with enterprise standards, it uses **zero emojis**.

### 1. Cognitive Reasoning Frame
Models emitting `<think>...</think>` blocks (such as DeepSeek-R1, Qwen 2.5 / 3.5, and fine-tuned quantized models) have their internal chain of thought extracted and displayed inside an executive card:

```text
╭── [REASONING & COGNITIVE PLANNING] ───────────────────────────────────
│  Analyzing project directory hierarchy and locating Cargo manifests...
│  Found Cargo.toml with 14 dependencies.
│  Planning syntax check via cargo check before performing surgical edits...
╰───────────────────────────────────────────────────────────────────────
```

### 2. Structured Action Badges (No Raw JSON Dumps)
Tool calls are parsed into structured badges displaying relevant metadata rather than escaped JSON text:

```text
╭── [ACTION: write_file] ───────────────────────────────────────────
│  Target:      src/main.rs
│  Payload:     1,420 bytes (42 lines)
╰───────────────────────────────────────────────────────────────────
```

```text
╭── [ACTION: execute_command] ──────────────────────────────────────
│  Command:     cargo test --all
╰───────────────────────────────────────────────────────────────────
```

```text
╭── [ACTION: edit_file] ────────────────────────────────────────────
│  Target:      src/config.rs
│  Diff:        replace 45 bytes with 52 bytes
╰───────────────────────────────────────────────────────────────────
```

### 3. Clean Result Cards
Outputs from commands and filesystem operations are bounded to prevent terminal flooding while preserving exit codes and byte telemetry:

```text
╭── [RESULT: execute_command] [OK] ─────────────────────────────────
│  test result: ok. 14 passed; 0 failed; 0 ignored; finished in 0.06s
╰───────────────────────────────────────────────────────────────────
```

### 4. Executive Final Response Card
The synthesized answer is delivered in a distinguished executive card:

```text
╭── [EXECUTIVE RESPONSE] ───────────────────────────────────────────────
│
│  The codebase audit is complete. All 14 unit tests pass with zero
│  regressions. Architecture documentation has been synchronized to Obsidian.
│
╰───────────────────────────────────────────────────────────────────────
```

---

## Multi-Tier Permission Model & SecurityGuard Sandbox

### 1. Interactive Permission Review (Mutating Tools)
When the model invokes a state-altering tool (`execute_command`, `write_file`, `edit_file`, `obsidian_write`, `obsidian_append`, `skill_create`), QSL Agent suspends execution and displays an authorization prompt:

```text
╭─── [PERMISSION REVIEW] ───────────────────────────────────────────╮
│  Tool:    write_file
│  Target:  src/agent.rs
│  Payload: 12,480 bytes (394 lines)
├───────────────────────────────────────────────────────────────────┤
│  [y] Allow once  │  [a] Always allow (session)  │  [n] Deny
│  Tip: Select [a] to auto-approve subsequent steps without pausing
╰───────────────────────────────────────────────────────────────────╯
  permission > 
```

- **`[y]` Allow Once**: Approves only this specific call.
- **`[a]` Always Allow**: Authorizes this tool class for the remainder of the session without interrupting again.
- **`[n]` Deny**: Instantly rejects execution and feeds the refusal message back into the model so it can formulate an alternative plan.

Permission modes can be set via `/approval <ask | always | strict>` or using the `-a` CLI argument:
- **`Ask`** *(Default)*: Interactively prompts for any mutating call; read-only operations pass automatically.
- **`Always`**: Autonomous execution for trusted workflows.
- **`Strict`**: Every mutating call requires manual `[y]` approval (session-wide bypass disabled).

### 2. SecurityGuard Regex Sandbox
Before any shell command or filesystem write is executed, `SecurityGuard` inspects the payload:
- **Destructive Deletion Interception**: Intercepts `rm -rf`, `find -delete`, `find -exec rm`, `xargs rm`, and `shred`.
- **Filesystem Integrity**: Blocks raw device overwrites (`> /dev/sd*`, `> /dev/nvme*`), partition tools (`fdisk`, `parted`), and `mkfs`.
- **Privilege & Secret Shield**: Prohibits direct access or reads to `/etc/shadow`, `/etc/sudoers`, SSH private keys (`id_rsa`, `id_ed25519`), AWS credentials, and kubeconfigs.
- **Protected Mounts**: Rejects file write operations targeting `/boot`, `/sys`, `/proc`, `/dev`, `/usr/bin`, `/sbin`, and `/etc`.
- **Execution Ceiling**: Imposes an asynchronous timeout (default: 30s) to abort hanging processes.

---

## Obsidian-Backed PKM Cortex & Long-Term Memory

QSL Agent integrates with your local Obsidian vault to store its memory cortex:

```
Your_Obsidian_Vault/
└── QSL_Agent/
    ├── Profile.md          # Agent identity, user preferences, hardware environment
    ├── Memories.md         # Persistent lessons, project facts, architecture decisions
    ├── Daily/
    │   └── 2026-11-03.md   # Chronological session logs with timestamps
    ├── Architecture/       # Project blueprints and repository analysis notes
    ├── Security/           # Vulnerability audit reports and CVE assessments
    ├── Research/           # Technical reference digests and API documentation
    └── Skills/             # 18 seeded and dynamically authored SKILL.md files
```

- **Dynamic Vault Auto-Discovery**: Automatically scans the `OBSIDIAN_VAULT` environment variable, the official Obsidian app registry (`obsidian.json`), and document directories (`~/Documents/*`, `~/Obsidian`, `~/Notes`, `~/Vault`) for active `.obsidian/` roots. If no vault exists yet, it cleanly initializes a default at `~/Documents/Obsidian_Vault`. Use `/vault` in the REPL to list detected vaults or `/vault auto` to re-trigger discovery.
- **Profile & Context Injection**: On every new session, recent entries from `Profile.md` and `Memories.md` are dynamically summarized and injected into the agent's system prompt.
- **Daily Journaling**: Every completed turn automatically logs timestamped exchanges into `Daily/YYYY-MM-DD.md`.
- **Bidirectional Links**: The agent utilizes Obsidian `[[wikilinks]]` to interlink concepts and project notes.

---

## Seeded Production Skills Catalog (18 Skills)

Skills are hot-reloaded automatically on boot from:
1. `~/.config/qsl-agent/skills/`
2. `<Obsidian_Vault>/QSL_Agent/Skills/`
3. `./skills/`

| Skill Name | Description | Key Domain Tags |
|:---|:---|:---|
| **`api-design`** | RESTful APIs, OpenAPI specifications, error contracts, and JSON schema validation. | `api`, `rest`, `openapi`, `contracts` |
| **`codebase-inspection`** | Systematically map directory hierarchies, LOC metrics, and architectural composition. | `code`, `inspection`, `architecture` |
| **`data-analysis`** | CSV/JSON inspection, data transformations, frequency analysis, and statistics. | `data`, `csv`, `json`, `statistics` |
| **`database-sql`** | SQLite/PostgreSQL schema analysis, query indexing, transactions, and optimizations. | `database`, `sql`, `sqlite`, `queries` |
| **`docker-container`** | Multi-stage Dockerfiles, minimal base images, non-root users, and `.dockerignore`. | `docker`, `containers`, `devops`, `cloud` |
| **`file-organizer`** | Directory structure cleanup, project scaffolding, and `.gitignore` auditing. | `files`, `organize`, `scaffold`, `cleanup` |
| **`git-workflow`** | Conventional commits, atomic branch management, diff audits, and release tags. | `git`, `commits`, `branches`, `diff` |
| **`markdown-docs`** | Professional technical documentation, specifications, and README authoring. | `markdown`, `docs`, `readme`, `technical` |
| **`network-diagnostics`** | Port scanning, DNS resolution, socket checks, and HTTP endpoint health probes. | `network`, `dns`, `ports`, `http` |
| **`obsidian-vault-manager`** | Bidirectional wikilinks (`[[Topic]]`), YAML frontmatter taxonomies, and daily logs. | `obsidian`, `pkm`, `knowledge`, `notes` |
| **`python-env`** | `uv` and virtual environment management, packaging, and `pyproject.toml` dependency tracking. | `python`, `uv`, `venv`, `packaging` |
| **`rust-cargo`** | Cargo check/build, strict clippy linting, audit checks, and release binary tuning. | `rust`, `cargo`, `clippy`, `performance` |
| **`security-audit`** | Secrets hunting, command injection reviews, CVE scans, and permission boundaries. | `security`, `audit`, `injection`, `cve` |
| **`software-development`** | Hypothesis-first debugging, test-driven development (TDD), and surgical modifications. | `debugging`, `tdd`, `refactoring`, `software` |
| **`system-ops`** | Arch Linux / Linux administration, pacman/yay telemetry, systemd, and resource checks. | `linux`, `arch`, `systemd`, `performance` |
| **`text-processing`** | Stream parsing with `ripgrep`, `awk`, `sed`, `grep`, and regular expressions. | `text`, `sed`, `awk`, `grep`, `regex` |
| **`web-research`** | Local curl queries, offline documentation retrieval, and API payload extraction. | `web`, `curl`, `research`, `docs` |
| **`yaml-toml-config`** | Syntax validation, schema checking, and safe editing of configuration files. | `yaml`, `toml`, `json`, `config` |

---

## Local AI Provider Integration & Dynamic Discovery

QSL Agent requires **zero hardcoded model dependencies**. On startup, it dynamically queries the active engine catalog:

### 1. Ollama (Default Engine)
```bash
# Start Ollama engine
ollama serve

# Pull any model suited for your hardware
ollama pull huihui-qwen35-9b-256k:latest
# or
ollama pull qwen2.5-coder:7b
# or
ollama pull llama3.3:latest

# Launch QSL Agent - it automatically detects and binds to your model:
./Releases/qsl-agent
```

- **Dynamic Fallback**: If multiple models exist, QSL Agent binds to the first available model.
- **Runtime Model Switch**: Type `/models` inside the REPL to list models, and `/model <name>` to switch instantly.

### 2. LM Studio
1. Open LM Studio on your workstation.
2. Navigate to the **Local Server** tab.
3. Load any quantized GGUF model and start the server on port `1234`.
4. Run QSL Agent targeting LM Studio:
   ```bash
   ./Releases/qsl-agent -p lmstudio
   ```

### 3. Custom OpenAI-Compatible Endpoints
Supports `vLLM`, `llama.cpp` server (`llama-server`), `LocalAI`, or any self-hosted inference engine exposing `/v1/chat/completions`:
```bash
./Releases/qsl-agent -p custom -e http://127.0.0.1:8080/v1
```

---

## Binary Compilation & Distribution Guide

### Do We Need the `target/` Folder to Run or Distribute?
**No.** The `target/` folder is exclusively an intermediate scratch workspace created by Cargo during compilation. It stores temporary compiler artifacts, dependency caches (`.rlib`), and debug symbol tables. 

Once compilation completes, the resulting executable (`target/release/qsl_agent`) is completely self-contained. You do not need to ship, copy, or retain the `target/` folder.

### Step-by-Step Binary Creation

#### 1. Prerequisites
- Linux (x86_64, aarch64) or macOS (Apple Silicon / Intel)
- Rust toolchain 1.80+ (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)

#### 2. Clone the Repository
```bash
git clone https://github.com/0x0806/qsl_agent.git
cd qsl_agent
```

#### 3. Build with Maximum Release Optimizations
```bash
cargo build --release
```
This builds an optimized binary located at `target/release/qsl_agent`.

#### 4. Strip Debug Symbols for Minimal Size
Strip the symbol table to reduce binary size from ~30MB down to **4.5 MB**:
```bash
strip target/release/qsl_agent
```

#### 5. Install or Stage the Binary
You can copy the binary into `Releases/` for local execution or install it globally to your system path:
```bash
# Stage in project release directory
mkdir -p Releases
install -m 755 target/release/qsl_agent Releases/qsl-agent

# Or install system-wide
sudo install -m 755 target/release/qsl_agent /usr/local/bin/qsl-agent
```

#### 6. Clean Intermediate Build Artifacts
You can safely remove the `target/` folder after compilation to recover disk space:
```bash
cargo clean
```
The installed binary in `Releases/qsl-agent` or `/usr/local/bin/qsl-agent` will continue to run independently with zero external dependencies.

---

## Command-Line Reference & Flags

```text
Usage: qsl-agent [OPTIONS]

Options:
  -P, --prompt <PROMPT>            Non-interactive prompt to execute immediately
  -m, --model <MODEL>              Target model name (e.g. huihui-qwen35-9b-256k:latest)
  -p, --provider <PROVIDER>        Provider engine: ollama, lmstudio, or custom
  -e, --endpoint <ENDPOINT>        Custom endpoint URL (e.g. http://127.0.0.1:11434)
  -v, --vault <VAULT>              Path to Obsidian Vault (defaults to auto-detected local vault)
  -a, --approval <APPROVAL>        Approval mode: ask, always, strict
  -t, --temperature <TEMP>         Sampling temperature between 0.0 and 2.0 (default: 0.2)
  -c, --context-size <SIZE>        Context window in tokens (default: 16384)
      --list-models                List detected local models and exit
      --show-config                Print active configuration TOML and exit
      --health-check               Run comprehensive system health check and exit
  -h, --help                       Print help information
  -V, --version                    Print version information
```

### Common Usage Examples
```bash
# Launch interactive REPL with auto-detected local model
qsl-agent

# Execute a one-shot automated task non-interactively
qsl-agent -P "Audit src/ for security issues and record findings in Obsidian"

# Run with LM Studio and strict permissions
qsl-agent -p lmstudio -a strict

# Run health diagnostics
qsl-agent --health-check
```

---

## Interactive Terminal REPL & Slash Commands

When launched without a prompt argument, QSL Agent starts an interactive shell:

```text
╭────────────────────────────────────────────────────────────────────────────╮
│ QSL AGENT — AUTONOMOUS LOCAL INTELLIGENCE (v0.2.0)                         │
│ [100% LOCAL]  [ZERO CLOUD]  [OBSIDIAN CORTEX]  [SECURITY GUARD]            │
├────────────────────────────────────────────────────────────────────────────┤
│ Engine:    ollama                                                          │
│ Model:     huihui-qwen35-9b-256k:latest                                    │
│ Approval:  Ask [Interactive Review: y/a/n]                                 │
│ Skills:    18 operational skills active                                    │
│ Vault:     /home/user/Documents/Obsidian_Vault                             │
│ Org:       Quantum Secular Labs (https://quantumseclabs.com/)              │
├────────────────────────────────────────────────────────────────────────────┤
│ Tip: Type /help for command manual or enter any autonomous task prompt.    │
╰────────────────────────────────────────────────────────────────────────────╯

qsl-agent > 
```

### Slash Commands
| Command | Alias | Description |
|:---|:---|:---|
| `/help` | — | Display the interactive command guide. |
| `/health` | — | Run live health probe on engine, models, and vault. |
| `/models` | — | Query and list all models loaded on current engine. |
| `/model <name>` | — | Switch the active inference model on the fly. |
| `/provider <p>` | — | Switch backend engine (`ollama`, `lmstudio`, `custom`). |
| `/approval <m>` | `/mode` | Change approval mode (`ask`, `always`, `strict`). |
| `/vault [path]` | — | View or switch active Obsidian vault path. |
| `/memory` | — | Inspect profile and recent long-term memories. |
| `/remember <txt>` | — | Append a custom fact or rule directly to Obsidian. |
| `/skills` | — | Display a formatted catalog table of all loaded skills. |
| `/skill <name>` | — | Read the procedural markdown rules for a skill. |
| `/clear` | — | Clear current conversation history. |
| `/config` | — | Output active configuration TOML. |
| `/exit` | `/quit` | Terminate session and exit. |

---

## Deterministic Tool Reference (13 Built-in Tools)

| Tool Name | Parameters | Description |
|:---|:---|:---|
| `execute_command` | `command: string` | Run sandboxed local bash command with timeout protection. |
| `read_file` | `path: string`, `offset?: int`, `limit?: int` | Read text file or slice specific line ranges. |
| `write_file` | `path: string`, `content: string` | Create or overwrite file on disk. |
| `edit_file` | `path: string`, `target: string`, `replacement: string` | Surgical string find-and-replace in file. |
| `list_dir` | `path?: string` | List directory contents with entry types (`DIR`, `FILE`, `LINK`). |
| `obsidian_search` | `query: string` | Full-text keyword search across all notes in vault. |
| `obsidian_read` | `note_path: string` | Read note contents from Obsidian vault by relative path. |
| `obsidian_write` | `note_path: string`, `content: string` | Create or update note in Obsidian vault. |
| `obsidian_list` | `subfolder?: string` | Catalog available notes in vault or subfolder. |
| `obsidian_append` | `memory: string` | Append timestamped memory to `QSL_Agent/Memories.md`. |
| `skill_list` | — | List all available domain skills with summaries. |
| `skill_read` | `name: string` | Read step-by-step procedural guidelines for a skill. |
| `skill_create` | `name: str`, `desc: str`, `tags: [str]`, `instructions: str` | Author and persist a new skill directly to the vault. |

---

## Configuration Reference (`config.toml`)

Configurations are persisted in `~/.config/qsl-agent/config.toml`:

```toml
# =============================================================================
# QSL Agent Configuration (2026 Edition)
# Developed by Quantum Secular Labs (https://quantumseclabs.com/)
# Autonomous, 100% Local & Sovereign Intelligence Engine
# =============================================================================

# AI Inference Engine ("ollama", "lmstudio", or "custom")
provider = "ollama"

# Engine Endpoints
ollama_url = "http://127.0.0.1:11434"
lmstudio_url = "http://127.0.0.1:1234/v1"
custom_url = ""

# Active Model (leave blank to auto-detect at startup)
model = ""

# Obsidian Vault Cortex (leave empty to auto-detect from local system)
obsidian_vault_path = ""

# Model Generation Parameters
temperature = 0.2
max_tokens = 4096
context_size = 16384

# Runtime & Security Settings
auto_approve_commands = false    # Set to true for unattended autonomous execution
max_react_steps = 35            # Adaptive reasoning iterations per turn
command_timeout_secs = 30       # Shell command execution timeout limit
```

---

## Troubleshooting & Diagnostics

### 1. `Connection refused to local AI engine`
- Verify your local server is running:
  - For Ollama: `systemctl --user status ollama` or run `ollama serve`
  - For LM Studio: Open LM Studio and toggle **Start Server** on port 1234.
- Test endpoint health directly:
  ```bash
  curl http://127.0.0.1:11434/api/tags
  ```

### 2. `Execution blocked by SecurityGuard`
- SecurityGuard intercepted a command matching dangerous patterns (`rm -rf`, `find -delete`, writing to `/boot`, reading `/etc/shadow`).
- Review the blocked command in the terminal error frame. Use safer alternatives (e.g. `rm <single_file>` or read specific paths).

### 3. Step Limit Reached
- If the agent reaches its step limit on an extensive task, use the `[c]` key in the interactive prompt to grant +15 additional steps, or increase `max_react_steps` in `~/.config/qsl-agent/config.toml`.

### 4. Configuration Regeneration
- If `config.toml` contains syntax errors, QSL Agent automatically backs it up to `config.toml.bak` and regenerates a clean default configuration.

---

## About Quantum Secular Labs

**[Quantum Secular Labs](https://quantumseclabs.com/)** (QuantumSec Labs) is a specialized cybersecurity and technology engineering firm focused on digital infrastructure defense, enterprise vulnerability assessment and penetration testing (VAPT), offensive security auditing, and applied Artificial Intelligence research.

QSL engineered **QSL Agent** to directly eliminate the security vulnerabilities, secret leakage, and compliance hazards inherent in cloud-tethered AI tools. By uniting defensive cybersecurity principles with native Rust systems engineering, QSL Agent provides developers, security auditors, and enterprises with an autonomous, 100% sovereign local intelligence engine purpose-built for zero-trust environments.

---

## License & Intellectual Property

QSL Agent is released under the **[QSL Team Source-Available License v1.0 (2026)](LICENSE)** by **[Quantum Secular Labs](https://quantumseclabs.com/)**.

- **Open Source / Source-Available**: You have the complete right to view, inspect, audit, compile, and run the software locally on your own hardware for personal, academic, or internal operational use.
- **Strictly Non-Redistributable**: You may **NOT** redistribute, resell, mirror, syndicate, sub-license, fork publicly, or distribute pre-compiled binaries or packages to any third party without explicit prior written authorization from Quantum Secular Labs.
- **Commercial & Cloud Exclusivity**: Providing QSL Agent as a hosted service (SaaS/PaaS/API wrapper) or commercial redistribution is reserved exclusively to **[Quantum Secular Labs](https://quantumseclabs.com/)**.
- **All Rights Reserved**: All copyrights, patents, and distribution rights remain solely with Quantum Secular Labs.
