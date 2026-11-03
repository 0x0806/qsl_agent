#!/usr/bin/env bash
# Optional: run a real task against your local Ollama / LM Studio model.
#   build-test/live.sh                       # first Ollama model
#   QSL_MODEL=qwen2.5-coder:7b build-test/live.sh
#   QSL_PROVIDER=lmstudio build-test/live.sh
source "$(dirname "$0")/common.sh"
pick_binary
QHOME="$(mktemp -d "${TMPDIR:-/tmp}/qsl-live-XXXXXX")"
trap 'rm -rf "$QHOME"' EXIT
mkdir -p "$QHOME/proj"
printf 'alpha\n' >"$QHOME/proj/a.txt"; printf 'beta\n' >"$QHOME/proj/b.txt"
cat >"$QHOME/config.toml" <<CFG
provider = "${QSL_PROVIDER:-ollama}"
model = "${QSL_MODEL:-}"
obsidian_vault_path = "$QHOME/vault"
state_dir = "$QHOME/state"
CFG
section "Live engine check"
if ! qsl --health-check; then fail "no reachable local engine/model"; exit 1; fi
section "Live task"
qsl "How many .txt files are in $QHOME/proj and what does each contain? Use tools." | tee "$OUT_DIR/live.log"
grep -qi "alpha" "$OUT_DIR/live.log" && grep -qi "beta" "$OUT_DIR/live.log" && ok "model used tools and answered" || fail "unexpected answer (see build-test/out/live.log)"
