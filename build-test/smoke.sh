#!/usr/bin/env bash
# Smoke-test the built binary against the scripted mock LLM:
# one-shot runs, JSON mode, tools, security, sessions, HTTP API and the
# interactive REPL (through a real pseudo-terminal).
source "$(dirname "$0")/common.sh"
need cargo; need curl; need python3
pick_binary
note "binary: $QSL_BIN"
start_mock
trap 'stop_mock; rm -rf "${QHOME:-}"' EXIT
make_home
note "mock LLM on :$MOCK_PORT, isolated HOME $QHOME"

section "CLI basics"
qsl --version | grep -q "qsl-agent" && ok "--version" || fail "--version"
qsl --version | grep -q "Quantum Secular Labs" && ok "--version shows company and copyright" || fail "--version company"
out=$(qsl "hello" 2>&1 || true)
if echo "$out" | grep -q "Hello" && ! echo "$out" | grep -q "█"; then ok "no logo in non-terminal one-shot output"; else fail "logo printed in one-shot output: $out"; fi
qsl --list-models | grep -q "mock-coder:7b" && ok "--list-models" || fail "--list-models"
qsl --health-check >"$OUT_DIR/health.txt" 2>&1 && ok "--health-check exits 0" || fail "--health-check"
qsl --show-config | grep -q "max_react_steps" && ok "--show-config" || fail "--show-config"

section "One-shot agent runs"
r=$(qsl -q "hello")
[[ "$r" == Hello* ]] && ok "plain answer: ${r:0:50}" || fail "plain answer: $r"
r=$(qsl --json "[mock:write] $QHOME/work/out.txt")
[[ -f "$QHOME/work/out.txt" ]] && echo "$r" | grep -q '"ok":true' && ok "write_file tool round trip" || fail "write_file: $r"
r=$(qsl -q "[mock:shell]"); [[ "$r" == "SHELL=true" ]] && ok "shell tool executes" || fail "shell: $r"
r=$(qsl -q "[mock:danger]" --yolo); [[ "$r" == "BLOCKED=true" ]] && ok "rm -rf / hard-blocked even in yolo" || fail "danger: $r"
r=$(qsl -q "[mock:risky]"); [[ "$r" == "DENIED=true" ]] && ok "risky command denied headless" || fail "risky: $r"
r=$(qsl -q "[mock:think]"); [[ "$r" == "The answer is 42." ]] && ok "<think> stripped from answer" || fail "think: $r"
r=$(qsl -q "[mock:notools]"); [[ "$r" == "TEXT_PROTOCOL_OK=true" ]] && ok "text tool-call fallback" || fail "notools: $r"
r=$(echo "piped input" | (cd "$QHOME" && HOME="$QHOME" QSL_CONFIG="$QHOME/config.toml" NO_COLOR=1 "$QSL_BIN" -q -P -))
[[ "$r" == *"said=piped input"* ]] && ok "prompt from stdin (-P -)" || fail "stdin: $r"

section "Sessions & memory"
qsl -q "[mock:memory]" >/dev/null
grep -q "favourite language is Rust" "$QHOME/vault/QSL_Agent/Memories.md" && ok "memory written to Obsidian" || fail "memory"
r=$(qsl -q "[mock:recall]"); [[ "$r" == "RECALL=true" ]] && ok "memory recalled in new run" || fail "recall: $r"
r=$(qsl -q -C "follow-up"); [[ "$r" == *"turns=2"* ]] && ok "--continue resumes history" || fail "continue: $r"
qsl --list-sessions | grep -q "1\." && ok "--list-sessions" || fail "--list-sessions"

section "HTTP API (--serve)"
API_PORT="$(free_port)"
(cd "$QHOME" && HOME="$QHOME" QSL_CONFIG="$QHOME/config.toml" QSL_API_TOKEN=t0k "$QSL_BIN" --serve "127.0.0.1:$API_PORT" </dev/null >"$OUT_DIR/serve.log" 2>&1) &
SERVE_PID=$!
for _ in $(seq 1 50); do curl -fs "http://127.0.0.1:$API_PORT/health" >/dev/null 2>&1 && break; sleep 0.1; done
code=$(curl -s -o /dev/null -w '%{http_code}' -X POST "http://127.0.0.1:$API_PORT/v1/chat" -H 'content-type: application/json' -d '{"message":"hi"}')
[[ "$code" == "401" ]] && ok "unauthenticated request rejected" || fail "auth: $code"
r=$(curl -s -X POST "http://127.0.0.1:$API_PORT/v1/chat" -H 'authorization: Bearer t0k' -H 'content-type: application/json' -d '{"message":"hi"}')
echo "$r" | grep -q '"ok":true' && ok "POST /v1/chat" || fail "chat: $r"
kill "$SERVE_PID" 2>/dev/null || true

section "Interactive REPL (pseudo-terminal)"
if python3 "$HERE/pty_repl.py" "$QSL_BIN" "$MOCK_PORT" >"$OUT_DIR/pty.log" 2>&1; then
  ok "$(tail -n 1 "$OUT_DIR/pty.log")"
else
  fail "PTY suite (see build-test/out/pty.log)"; grep -E "FAIL|TIMEOUT" "$OUT_DIR/pty.log" | head
fi

echo
[ "$FAILURES" -eq 0 ] && ok "smoke.sh: all checks passed" || { fail "smoke.sh: $FAILURES check(s) failed"; exit 1; }
