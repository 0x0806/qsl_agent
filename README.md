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

[Features](#key-features) • [Architecture](#architecture--2026-agent-standards) • [Releases](#releases--installation) • [Skills (18 Seeded)](#extensible-skills-catalog-18-seeded-skills) • [Security & Approvals](#multi-tier-permission--security-guard) • [CLI Reference](#cli-flags--options) • [About QSL](#about-quantum-secular-labs)

</div>

---

## Table of Contents

1. [Overview](#overview)
2. [Key Features](#key-features)
3. [Architecture & 2026 Agent Standards](#architecture--2026-agent-standards)
4. [Empirical Performance Benchmark](#empirical-performance-benchmark)
5. [Multi-Tier Permission & Security Guard](#multi-tier-permission--security-guard)
6. [Obsidian-Backed PKM & Long-Term Memory](#obsidian-backed-pkm--long-term-memory)
7. [Extensible Skills Catalog (18 Seeded Skills)](#extensible-skills-catalog-18-seeded-skills)
8. [Authoring Custom Skills](#authoring-custom-skills)
9. [Local AI Engines & Dynamic Model Discovery](#local-ai-engines--dynamic-model-discovery)
10. [Releases & Installation](#releases--installation)
11. [CLI Flags & Options](#cli-flags--options)
12. [Interactive Terminal REPL & Slash Commands](#interactive-terminal-repl--slash-commands)
13. [Deterministic Tool Definitions (13 Built-in Tools)](#deterministic-tool-definitions-13-built-in-tools)
14. [Configuration Reference (`config.toml`)](#configuration-reference-configtoml)
15. [Troubleshooting & Diagnostics](#troubleshooting--diagnostics)
16. [About Quantum Secular Labs](#about-quantum-secular-labs)
17. [License & Intellectual Property](#license--intellectual-property)

---

## Overview

QSL Agent is an autonomous systems agent implemented in native Rust for Linux and macOS environments. It operates directly against on-premise inference runtimes—including Ollama, LM Studio, and standard OpenAI-compatible endpoints—without routing prompts, data, or telemetry through remote cloud platforms.

Unlike agent frameworks requiring complex Python virtual environments, heavy package trees, and runtime dependencies, QSL Agent compiles into an immutable, single **4.5 MB stripped binary**. It binds to your local Obsidian vault as a bidirectional Personal Knowledge Management (PKM) cortex, ensuring all memories, user profiles, daily execution traces, and operational playbooks remain human-readable files on your own filesystem.

---

## Key Features

- **Native Systems Performance**: Sub-3ms process start latency and ~15MB idle RAM footprint.
- **Dynamic Model Auto-Discovery**: Automatically queries local engine catalogs at startup to bind available models across different hardware configurations without hardcoding.
- **Adaptive KV Cache Allocation**: Dynamically bounds inference context per-prompt (`(estimated_tokens + 2048).clamp(4096, context_size)`), reducing CPU RAM cache allocation overhead and accelerating time-to-first-token.
- **Live Elapsed-Time Feedback**: Asynchronous terminal ticker (`[Step N] (Xs) /`) during local model generation turns.
- **Air-Gapped Privacy Architecture**: Zero network telemetry, zero external authentication tokens, and zero cloud lock-in.
- **3-Tier Permission Manager**: Granular runtime authorization (`[y]` allow once, `[a]` always allow for session, `[n]` deny) for state-altering operations.
- **SecurityGuard Sandbox**: Real-time regex inspection preventing destructive shell operations (`rm -rf`, raw device writes, system partition tampering) and private key leakage.
- **Obsidian Cortex**: Profiles, daily activity logs, and long-term memories persist as standard Markdown with native bidirectional `[[wikilinks]]`.
- **18 Seeded Production Skills**: Pre-packaged operational guidelines covering codebase audits, TDD debugging, Docker, SQL, Git, and Linux administration.
- **Dual-Mode Tool Execution**: Parses both standard OpenAI JSON tool payloads and regex `<tool_call>` XML blocks from quantized models.
- **Minimal Professional ASCII CLI**: High-contrast Unicode box-drawing frames with zero emoji clutter and build-synchronized version headers.

---

## Architecture & 2026 Agent Standards

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
                         │  ReAct Agent Loop   │
                         │  (Step Controller)  │
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

## Empirical Performance Benchmark

Measurements performed on Linux x86_64 across identical agentic task executions (local Ollama backend, Qwen 2.5/3.5 models):

| Metric | Traditional Python Stack (Hermes / AutoGPT) | QSL Agent (Rust v0.2.0) | Performance Advantage |
|:---|:---:|:---:|:---|
| **Cold Start Latency** | 450ms – 2,200ms (interpreter + import overhead) | **< 3ms** (native compiled ELF binary) | **>150x faster startup** |
| **Process Idle RAM** | 180MB – 420MB (Python runtime + wheel dependencies) | **~15MB** (zero garbage collection overhead) | **~94% memory reduction** |
| **Distribution Footprint** | Multi-GB (virtualenv, wheels, torch/pip caches) | **4.5MB Single Stripped Binary** | **Self-contained zero-dependency artifact** |
| **External Dependencies** | Python 3.10+, pip, system build toolchains | **None** (statically linked against glibc) | **Zero environment drift** |
| **Telemetry & Cloud Auth**| Mandatory remote API keys or portal sign-ins | **Zero** (100% On-Premise Air-Gapped) | **Absolute Data Sovereignty** |
| **Persistent Memory Format**| Proprietary vector databases / cloud sync | **Obsidian Markdown (`[[wikilinks]]`)** | **Interoperable, Git-trackable files** |
| **Safety Interception** | Post-exception handler / unconstrained shell | **Pre-execution Regex Sandbox + 3-Tier Gate** | **Deterministic command filtering** |

---

## Multi-Tier Permission & Security Guard

### 1. Interactive Permission Review (Mutating Tools)
When the model invokes a state-altering tool (`execute_command`, `write_file`, `edit_file`, `obsidian_write`, `obsidian_append`, `skill_create`), QSL Agent suspends execution and displays an interactive modal:

```text
╭─── [PERMISSION REVIEW] ───────────────────────────────────────────╮
│  Tool:  execute_command
│  Args:  {"command": "cargo test --all"}
├───────────────────────────────────────────────────────────────────┤
│  [y] Allow once  │  [a] Always allow (session)  │  [n] Deny
╰───────────────────────────────────────────────────────────────────╯
  permission > 
```

- **`[y]` Allow Once**: Approves only this specific call.
- **`[a]` Always Allow**: Authorizes this tool class for the remainder of the session without interrupting again.
- **`[n]` Deny**: Instantly rejects execution and feeds the refusal message back into the model so it can formulate an alternative plan.

Modes can be toggled using `/approval <ask | always | strict>` or via the `-a` CLI argument:
- **`Ask`** *(Default)*: Interactively prompts for any mutating call; read-only operations pass automatically.
- **`Always`**: Autonomous hands-off execution for trusted CI/script pipelines.
- **`Strict`**: Every mutating call requires manual `[y]` approval (no session-wide bypass permitted).

### 2. SecurityGuard Regex Sandbox
Before any shell command or filesystem write is attempted, `SecurityGuard` evaluates the operation:
- **Destructive Deletion Interception**: Intercepts `rm -rf`, `find -delete`, `find -exec rm`, `xargs rm`, and `shred`.
- **Filesystem Integrity**: Blocks raw device overwrites (`> /dev/sd*`, `> /dev/nvme*`), partition tools (`fdisk`, `parted`), and `mkfs`.
- **Privilege & Secret Shield**: Prohibits direct access or reads to `/etc/shadow`, `/etc/sudoers`, SSH private keys (`id_rsa`, `id_ed25519`), AWS credentials, and kubeconfigs.
- **Protected Mounts**: Rejects file write operations targeting `/boot`, `/sys`, `/proc`, `/dev`, `/usr/bin`, `/sbin`, and `/etc`.
- **Execution Ceiling**: Imposes an asynchronous timeout (default: 30s) to abort hanging processes.

---

## Obsidian-Backed PKM & Long-Term Memory

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

- **Dynamic Vault Auto-Discovery**: Never hardcoded to a specific path or machine. QSL Agent automatically scans the `OBSIDIAN_VAULT` environment variable, the official Obsidian app registry (`obsidian.json`), and common directories (`~/Documents/*`, `~/Obsidian`, `~/Notes`, `~/Vault`) for active `.obsidian/` roots. If no vault exists yet, it cleanly initializes a default at `~/Documents/Obsidian_Vault`. Use `/vault` in the REPL to list all detected vaults or `/vault auto` to re-trigger auto-discovery.
- **Profile & Context Injection**: On every new session, recent entries from `Profile.md` and `Memories.md` are dynamically summarized and injected into the agent's system prompt.
- **Daily Journaling**: Every completed turn automatically logs timestamped exchanges into `Daily/YYYY-MM-DD.md`.
- **Bidirectional Links**: The agent is instructed to use Obsidian `[[wikilinks]]` to interlink topics and project notes.

---

## Extensible Skills Catalog (18 Seeded Skills)

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

## Authoring Custom Skills

Create a new directory inside `<Obsidian_Vault>/QSL_Agent/Skills/<skill-name>/` containing a `SKILL.md` file:

```markdown
---
name: kubernetes-helm
description: "Deploy, inspect, and package Kubernetes applications using Helm charts"
tags: [kubernetes, helm, devops, containers]
version: 1.0.0
year: 2026
---

# Kubernetes & Helm Skill (2026 Edition)

## Purpose
Inspect, lint, and deploy manifests safely using helm and kubectl.

## Guidelines
1. Always run `helm lint` before applying chart modifications.
2. Verify target cluster context using `kubectl config current-context`.
3. Check pod status using `kubectl get pods -n <namespace>`.
```

Alternatively, tell the agent in plain language to create the skill using the built-in `skill_create` tool.

---

## Local AI Engines & Dynamic Model Discovery

Different machines feature different hardware specifications and local model rosters. QSL Agent requires **zero hardcoded model dependencies**. On every startup, it dynamically queries the active engine catalog:

### 1. Ollama (Default Engine)
```bash
# Start Ollama engine
ollama serve

# Pull any local model suited for your hardware (e.g. 7B, 9B, 14B, 32B)
ollama pull huihui-qwen35-9b-256k:latest
# or
ollama pull llama3.3:latest
# or
ollama pull mistral:latest

# Launch QSL Agent - it automatically detects and binds to your installed model:
./Releases/qsl-agent
```

- **Dynamic Fallback**: If multiple models exist, QSL Agent binds to the first loaded model.
- **Runtime Model Switch**: Type `/models` inside the REPL to list all available local models, and `/model <name>` to switch instantly without restarting.

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

## Releases & Installation

### Pre-Compiled Standalone Binaries (Linux x86_64)
Ready-to-run release binaries are generated in the `Releases/` folder:

- **`Releases/qsl-agent`**: Latest production stripped binary (4.5 MB).
- **`Releases/qsl-agent-v0.2.0-linux-x86_64`**: Version-tagged immutable release archive.

Run directly without installing any runtime dependencies:
```bash
chmod +x Releases/qsl-agent
./Releases/qsl-agent --health-check
```

### Build from Source
#### Prerequisites
- Linux or macOS with Rust 1.80+ (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- Standard Unix utilities (`bash`, `curl`, `grep`, `find`)

#### Compile & Strip
```bash
git clone https://github.com/qsl-team/qsl-agent.git
cd qsl-agent

# Build fully optimized release binary
cargo build --release

# Stage production binary into Releases/
mkdir -p Releases
cp target/release/qsl_agent Releases/qsl-agent
strip Releases/qsl-agent
cp Releases/qsl-agent Releases/qsl-agent-v0.2.0-linux-x86_64
```

---

## CLI Flags & Options

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

### Examples
```bash
# Launch interactive REPL with auto-detected local model
./Releases/qsl-agent

# Execute a one-shot automated task non-interactively
./Releases/qsl-agent -P "Inspect src/ for memory leaks and update architecture notes in Obsidian"

# Run with LM Studio and strict permissions
./Releases/qsl-agent -p lmstudio -a strict

# Run health diagnostics
./Releases/qsl-agent --health-check
```

---

## Interactive Terminal REPL & Slash Commands

When launched without a prompt argument, QSL Agent opens an interactive readline shell:

```text
╭────────────────────────────────────────────────────────────────────────────╮
│ QSL AGENT — AUTONOMOUS LOCAL INTELLIGENCE (v0.2.0)                         │
│ [100% LOCAL]  [ZERO CLOUD]  [OBSIDIAN CORTEX]  [SECURITY GUARD]              │
├────────────────────────────────────────────────────────────────────────────┤
│ Engine:    ollama                                                            │
│ Model:     huihui-qwen35-9b-256k:latest                                      │
│ Approval:  Ask [Interactive Review: y/a/n]                                   │
│ Skills:    18 operational skills active                                      │
│ Vault:     /home/user/Documents/Obsidian_Vault                               │
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

## Deterministic Tool Definitions (13 Built-in Tools)

The agent dynamically decides which tool to call:

| Tool Name | Parameters | Description |
|:---|:---|:---|
| `execute_command` | `command: string` | Run sandboxed local bash command with timeout protection. |
| `read_file` | `path: string`, `start_line?: int`, `end_line?: int` | Read text file or slice specific line ranges. |
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
# AI Inference Engine ("ollama", "lmstudio", or "custom")
provider = "ollama"

# Engine Endpoints
ollama_url = "http://127.0.0.1:11434"
lmstudio_url = "http://127.0.0.1:1234/v1"
custom_url = ""

# Active Model (leave blank to auto-detect at startup)
model = "huihui-qwen35-9b-256k:latest"

# Obsidian Vault Cortex (leave empty to auto-detect from local system)
obsidian_vault_path = ""

# Model Generation Parameters
temperature = 0.2
max_tokens = 4096
context_size = 16384

# Runtime & Security Settings
auto_approve_commands = false    # Set to true for unattended autonomous execution
max_react_steps = 20            # Maximum reasoning iterations per turn
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

### 3. Corrupted Configuration
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
