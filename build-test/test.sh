#!/usr/bin/env bash
# Static checks + unit tests + end-to-end tests (mock LLM, both protocols).
source "$(dirname "$0")/common.sh"
need cargo
cd "$ROOT"

section "Formatting (cargo fmt --check)"
if cargo fmt --check >/dev/null 2>&1; then ok "code is formatted"; else fail "run: cargo fmt"; fi

section "Lints (cargo clippy -D warnings)"
if cargo clippy --all-targets --quiet -- -D warnings 2>"$OUT_DIR/clippy.log"; then ok "no clippy warnings"
else fail "clippy (see build-test/out/clippy.log)"; tail -n 20 "$OUT_DIR/clippy.log"; fi

section "Unit + end-to-end tests (cargo test)"
if cargo test 2>&1 | tee "$OUT_DIR/test.log" | grep -E "^test result|FAILED|panicked"; then :; fi
if grep -q "FAILED\|error\[" "$OUT_DIR/test.log"; then fail "cargo test (see build-test/out/test.log)"; else ok "all cargo tests passed"; fi

[ "$FAILURES" -eq 0 ] && { echo; ok "test.sh: all checks passed"; } || { echo; fail "test.sh: $FAILURES check(s) failed"; exit 1; }
