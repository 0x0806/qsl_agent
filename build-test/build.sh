#!/usr/bin/env bash
# Build an optimised, stripped release binary into build-test/bin/.
source "$(dirname "$0")/common.sh"
need cargo

section "Release build (LTO, stripped)"
cd "$ROOT"
cargo build --release --locked 2>&1 | tail -n 3
install -m 755 target/release/qsl-agent "$BIN_DIR/qsl-agent"

size=$(du -h "$BIN_DIR/qsl-agent" | cut -f1)
version=$("$BIN_DIR/qsl-agent" --version)
sha=$(sha256sum "$BIN_DIR/qsl-agent" | cut -d' ' -f1)
ok "$version  ($size)  -> build-test/bin/qsl-agent"
note "sha256 $sha"
start=$(date +%s%N); "$BIN_DIR/qsl-agent" --version >/dev/null; end=$(date +%s%N)
note "cold start: $(( (end - start) / 1000000 )) ms"
echo
echo "Install system-wide with:  sudo install -m 755 build-test/bin/qsl-agent /usr/local/bin/qsl-agent"
