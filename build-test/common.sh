# Shared helpers for the build-test scripts (sourced, not executed).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HERE="$ROOT/build-test"
BIN_DIR="$HERE/bin"
OUT_DIR="$HERE/out"
mkdir -p "$BIN_DIR" "$OUT_DIR"

if [ -t 1 ]; then
  C_OK=$'\e[32m'; C_ERR=$'\e[31m'; C_HDR=$'\e[1;36m'; C_DIM=$'\e[2m'; C_END=$'\e[0m'
else
  C_OK=""; C_ERR=""; C_HDR=""; C_DIM=""; C_END=""
fi

section() { printf '\n%s== %s ==%s\n' "$C_HDR" "$*" "$C_END"; }
ok()      { printf '  %s[PASS]%s %s\n' "$C_OK" "$C_END" "$*"; }
fail()    { printf '  %s[FAIL]%s %s\n' "$C_ERR" "$C_END" "$*"; FAILURES=$((FAILURES + 1)); }
note()    { printf '  %s%s%s\n' "$C_DIM" "$*" "$C_END"; }
FAILURES=0

need() {
  command -v "$1" >/dev/null 2>&1 || { printf '%s[ERROR]%s required tool missing: %s\n' "$C_ERR" "$C_END" "$1"; exit 2; }
}

free_port() {
  python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()' 2>/dev/null \
    || echo $(( 20000 + RANDOM % 20000 ))
}

# Start the scripted mock LLM; sets MOCK_PORT and MOCK_PID.
start_mock() {
  (cd "$ROOT" && cargo build --quiet --example mock_llm)
  MOCK_PORT="$(free_port)"
  "$ROOT/target/debug/examples/mock_llm" "$MOCK_PORT" >"$OUT_DIR/mock.log" 2>&1 &
  MOCK_PID=$!
  for _ in $(seq 1 50); do
    curl -fs "http://127.0.0.1:$MOCK_PORT/v1/models" >/dev/null 2>&1 && return 0
    sleep 0.1
  done
  echo "mock server failed to start"; cat "$OUT_DIR/mock.log"; exit 1
}

stop_mock() { [ -n "${MOCK_PID:-}" ] && kill "$MOCK_PID" 2>/dev/null || true; }

# Isolated HOME with a config pointing at the mock; sets QHOME.
make_home() {
  QHOME="$(mktemp -d "${TMPDIR:-/tmp}/qsl-bt-XXXXXX")"
  cat >"$QHOME/config.toml" <<EOF
provider = "custom"
custom_url = "http://127.0.0.1:$MOCK_PORT/v1"
model = "mock-coder:7b"
obsidian_vault_path = "$QHOME/vault"
state_dir = "$QHOME/state"
max_retries = 0
EOF
}

# Run the agent inside the isolated HOME.
qsl() {
  (cd "$QHOME" && HOME="$QHOME" XDG_CONFIG_HOME="$QHOME/.config" XDG_STATE_HOME="$QHOME/.local/state" \
    QSL_CONFIG="$QHOME/config.toml" NO_COLOR=1 "$QSL_BIN" "$@" </dev/null)
}

pick_binary() {
  if [ -x "$BIN_DIR/qsl-agent" ]; then QSL_BIN="$BIN_DIR/qsl-agent"
  else (cd "$ROOT" && cargo build --quiet); QSL_BIN="$ROOT/target/debug/qsl-agent"; fi
}
