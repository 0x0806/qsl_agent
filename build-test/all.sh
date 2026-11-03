#!/usr/bin/env bash
# Build the release binary, run the full test suite, then smoke-test it.
set -euo pipefail
cd "$(dirname "$0")"
./build.sh
./test.sh
./smoke.sh
