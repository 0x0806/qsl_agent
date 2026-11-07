#!/usr/bin/env bash
# QSL Agent installer for Linux and macOS.
#
#   curl -fsSL https://raw.githubusercontent.com/0x0806/qsl_agent/main/install.sh | bash
#
# Installs the `qsl-agent` binary plus a `qslagent` command (same program) so
# either name works from any shell. Options (after `bash -s --` when piping):
#   --version <tag>   install a specific release (e.g. v0.5.0); default: latest
#   --prefix <dir>    install into <dir> instead of the default location
#   --user            install for the current user only (~/.local/bin)
#   --from-source     skip the prebuilt binary and build with cargo
#   --uninstall       remove QSL Agent (config and vault are left untouched)
#   -h, --help        show this help
# Environment: QSL_VERSION, QSL_PREFIX, QSL_BASE_URL (download mirror override).
set -euo pipefail

REPO="0x0806/qsl_agent"
BIN="qsl-agent"
ALIAS="qslagent"
VERSION="${QSL_VERSION:-latest}"
PREFIX="${QSL_PREFIX:-}"
BASE_URL="${QSL_BASE_URL:-}"
USER_ONLY=0
FROM_SOURCE=0
UNINSTALL=0

if [ -t 1 ]; then B=$'\033[1m'; G=$'\033[32m'; Y=$'\033[33m'; R=$'\033[31m'; Z=$'\033[0m'; else B=; G=; Y=; R=; Z=; fi
info() { printf '%s==>%s %s\n' "$B" "$Z" "$*"; }
ok()   { printf '%s ok%s %s\n' "$G" "$Z" "$*"; }
warn() { printf '%swarn%s %s\n' "$Y" "$Z" "$*" >&2; }
die()  { printf '%serror%s %s\n' "$R" "$Z" "$*" >&2; exit 1; }
usage() { sed -n '2,15p' "${BASH_SOURCE[0]:-$0}" 2>/dev/null | sed 's/^# \{0,1\}//'; exit 0; }

while [ $# -gt 0 ]; do
  case "$1" in
    --version)     [ $# -ge 2 ] || die "--version needs a value"; VERSION="$2"; shift 2 ;;
    --prefix)      [ $# -ge 2 ] || die "--prefix needs a value"; PREFIX="$2"; shift 2 ;;
    --user)        USER_ONLY=1; shift ;;
    --from-source) FROM_SOURCE=1; shift ;;
    --uninstall)   UNINSTALL=1; shift ;;
    -h|--help)     usage ;;
    *) die "unknown option: $1 (try --help)" ;;
  esac
done

have() { command -v "$1" >/dev/null 2>&1; }

# ---- choose install directory ------------------------------------------------
SUDO=""
if [ -n "$PREFIX" ]; then
  DEST="$PREFIX"
elif [ "$USER_ONLY" -eq 1 ]; then
  DEST="$HOME/.local/bin"
else
  DEST="/usr/local/bin"
fi

can_write() { # directory (or its nearest existing parent) is writable
  local d="$1"
  while [ ! -d "$d" ]; do d="$(dirname "$d")"; done
  [ -w "$d" ]
}

if ! can_write "$DEST"; then
  if [ "$(id -u)" -ne 0 ] && have sudo && [ -z "$PREFIX" ] && [ "$USER_ONLY" -eq 0 ]; then
    SUDO="sudo"
    info "$DEST needs elevated rights; using sudo"
  elif [ -z "$PREFIX" ] && [ "$USER_ONLY" -eq 0 ]; then
    warn "cannot write to $DEST and sudo is unavailable; installing to ~/.local/bin"
    DEST="$HOME/.local/bin"
  else
    die "cannot write to $DEST"
  fi
fi

# ---- uninstall ---------------------------------------------------------------
if [ "$UNINSTALL" -eq 1 ]; then
  removed=0
  for d in "$DEST" "$HOME/.local/bin" /usr/local/bin; do
    for f in "$d/$BIN" "$d/$ALIAS"; do
      if [ -e "$f" ] || [ -L "$f" ]; then
        if [ -w "$d" ]; then rm -f "$f"; else sudo rm -f "$f"; fi
        ok "removed $f"; removed=1
      fi
    done
  done
  [ "$removed" -eq 1 ] || info "nothing to remove"
  echo "Config (~/.config/qsl-agent) and your Obsidian vault were left in place."
  exit 0
fi

# ---- platform detection ------------------------------------------------------
OS="$(uname -s)"; ARCH="$(uname -m)"
case "$OS" in
  Linux)  os_t="unknown-linux-gnu" ;;
  Darwin) os_t="apple-darwin" ;;
  *) die "unsupported OS: $OS (Windows: use install.ps1)" ;;
esac
case "$ARCH" in
  x86_64|amd64)  arch_t="x86_64" ;;
  arm64|aarch64) arch_t="aarch64" ;;
  *) die "unsupported architecture: $ARCH" ;;
esac
TARGET="${arch_t}-${os_t}"

TMP="$(mktemp -d 2>/dev/null || mktemp -d -t qslagent)"
trap 'rm -rf "$TMP"' EXIT

fetch() { # url, outfile
  if have curl; then curl -fsSL --retry 3 -o "$2" "$1"
  elif have wget; then wget -q -O "$2" "$1"
  else die "need curl or wget"; fi
}

sha256_of() {
  if have sha256sum; then sha256sum "$1" | cut -d' ' -f1
  elif have shasum; then shasum -a 256 "$1" | cut -d' ' -f1
  else return 1; fi
}

# ---- obtain the binary -------------------------------------------------------
download_release() {
  local asset="${BIN}-${TARGET}.tar.gz" url
  if [ -n "$BASE_URL" ]; then url="$BASE_URL"
  elif [ "$VERSION" = "latest" ]; then url="https://github.com/${REPO}/releases/latest/download"
  else url="https://github.com/${REPO}/releases/download/${VERSION}"; fi

  info "Downloading ${asset} (${VERSION})"
  fetch "$url/$asset" "$TMP/$asset" 2>/dev/null || return 1
  if fetch "$url/$asset.sha256" "$TMP/$asset.sha256" 2>/dev/null; then
    local want got
    want="$(cut -d' ' -f1 "$TMP/$asset.sha256" | tr -d '[:space:]')"
    got="$(sha256_of "$TMP/$asset")" || { warn "no sha256 tool; skipping checksum"; got="$want"; }
    [ "$want" = "$got" ] || die "checksum mismatch for $asset (expected $want, got $got)"
    ok "checksum verified"
  else
    warn "no checksum published for this release; skipping verification"
  fi
  tar -xzf "$TMP/$asset" -C "$TMP" || return 1
  [ -f "$TMP/$BIN" ] || return 1
  return 0
}

build_from_source() {
  have cargo || die "no prebuilt binary available and cargo not found. Install Rust (https://rustup.rs) and re-run with --from-source."
  have git || die "git is required to build from source"
  info "Building from source (this takes a few minutes)"
  local ref=()
  [ "$VERSION" != "latest" ] && ref=(--branch "$VERSION")
  git clone --depth 1 ${ref[@]+"${ref[@]}"} "https://github.com/${REPO}.git" "$TMP/src" >/dev/null 2>&1 \
    || die "git clone failed"
  ( cd "$TMP/src" && cargo build --release --locked ) || die "cargo build failed"
  cp "$TMP/src/target/release/$BIN" "$TMP/$BIN"
}

if [ "$FROM_SOURCE" -eq 1 ]; then
  build_from_source
elif ! download_release; then
  warn "no prebuilt binary for ${TARGET}; falling back to a source build"
  build_from_source
fi

# ---- install -----------------------------------------------------------------
$SUDO mkdir -p "$DEST"
$SUDO install -m 755 "$TMP/$BIN" "$DEST/$BIN"
$SUDO ln -sf "$BIN" "$DEST/$ALIAS"
# macOS: clear the download quarantine flag so Gatekeeper does not block it.
if [ "$OS" = "Darwin" ]; then
  $SUDO xattr -d com.apple.quarantine "$DEST/$BIN" 2>/dev/null || true
fi

ver="$("$DEST/$BIN" --version 2>/dev/null || true)"
[ -n "$ver" ] || die "installed binary failed to run: $DEST/$BIN"
ok "installed $ver"
ok "$DEST/$BIN"
ok "$DEST/$ALIAS -> $BIN"

case ":$PATH:" in
  *":$DEST:"*) ;;
  *)
    warn "$DEST is not on your PATH. Add it with:"
    echo "    echo 'export PATH=\"$DEST:\$PATH\"' >> ~/.profile   # then restart your shell"
    ;;
esac

cat <<MSG

Next steps:
  1. Start a local model server (e.g. Ollama: https://ollama.com).
  2. Run:  qslagent --health-check
  3. Run:  qslagent

Uninstall:  curl -fsSL https://raw.githubusercontent.com/${REPO}/main/install.sh | bash -s -- --uninstall
MSG
