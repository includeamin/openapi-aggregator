#!/bin/sh
# Install/uninstall script for openapi-aggregator
# Usage:
#   Install:    curl -sSfL https://raw.githubusercontent.com/OWNER/openapi-aggregator/main/install.sh | sh
#   Uninstall:  curl -sSfL https://raw.githubusercontent.com/OWNER/openapi-aggregator/main/install.sh | sh -s -- --uninstall
#
# Options (via environment variables):
#   VERSION     - specific version to install (e.g. v0.1.0). Defaults to latest.
#   INSTALL_DIR - directory to install to. Defaults to /usr/local/bin.

set -e

REPO="OWNER/openapi-aggregator"
BINARY="openapi-aggregator"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

# --- helpers ---------------------------------------------------------------

info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$1"; }
error() { printf '\033[1;31m[error]\033[0m %s\n' "$1" >&2; exit 1; }

need() {
  command -v "$1" >/dev/null 2>&1 || error "required command not found: $1"
}

# --- detect OS & arch ------------------------------------------------------

detect_target() {
  OS=$(uname -s)
  ARCH=$(uname -m)

  case "$OS" in
    Linux)
      case "$ARCH" in
        x86_64)  TARGET="x86_64-unknown-linux-musl" ;;
        aarch64) TARGET="aarch64-unknown-linux-musl" ;;
        arm64)   TARGET="aarch64-unknown-linux-musl" ;;
        *)       error "unsupported Linux architecture: $ARCH" ;;
      esac
      EXT="tar.gz"
      ;;
    Darwin)
      case "$ARCH" in
        x86_64)  TARGET="x86_64-apple-darwin" ;;
        arm64)   TARGET="aarch64-apple-darwin" ;;
        aarch64) TARGET="aarch64-apple-darwin" ;;
        *)       error "unsupported macOS architecture: $ARCH" ;;
      esac
      EXT="tar.gz"
      ;;
    MINGW*|MSYS*|CYGWIN*|Windows_NT)
      TARGET="x86_64-pc-windows-msvc"
      EXT="zip"
      ;;
    *)
      error "unsupported OS: $OS"
      ;;
  esac

  info "detected target: $TARGET"
}

# --- resolve version -------------------------------------------------------

resolve_version() {
  if [ -n "$VERSION" ]; then
    TAG="$VERSION"
  else
    need curl
    TAG=$(curl -sSfL "https://api.github.com/repos/${REPO}/releases/latest" \
      | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')
    [ -n "$TAG" ] || error "could not determine latest release"
  fi
  info "installing version: $TAG"
}

# --- download & install ----------------------------------------------------

download_and_install() {
  need curl

  FILENAME="${BINARY}-${TARGET}.${EXT}"
  URL="https://github.com/${REPO}/releases/download/${TAG}/${FILENAME}"

  TMPDIR=$(mktemp -d)
  trap 'rm -rf "$TMPDIR"' EXIT

  info "downloading $URL"
  curl -sSfL -o "${TMPDIR}/${FILENAME}" "$URL" \
    || error "download failed — check that version ${TAG} exists and has a ${TARGET} build"

  info "extracting..."
  case "$EXT" in
    tar.gz)
      tar xzf "${TMPDIR}/${FILENAME}" -C "$TMPDIR"
      ;;
    zip)
      need unzip
      unzip -q "${TMPDIR}/${FILENAME}" -d "$TMPDIR"
      ;;
  esac

  # Install binary
  if [ -w "$INSTALL_DIR" ]; then
    mv "${TMPDIR}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
  else
    info "elevated permissions required to install to ${INSTALL_DIR}"
    sudo mv "${TMPDIR}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
  fi

  chmod +x "${INSTALL_DIR}/${BINARY}"

  info "installed ${BINARY} to ${INSTALL_DIR}/${BINARY}"
  "${INSTALL_DIR}/${BINARY}" --version 2>/dev/null || true
}

# --- uninstall -------------------------------------------------------------

uninstall() {
  if [ ! -f "${INSTALL_DIR}/${BINARY}" ]; then
    error "${BINARY} not found in ${INSTALL_DIR}"
  fi

  info "removing ${INSTALL_DIR}/${BINARY}"
  if [ -w "$INSTALL_DIR" ]; then
    rm -f "${INSTALL_DIR}/${BINARY}"
  else
    info "elevated permissions required to remove from ${INSTALL_DIR}"
    sudo rm -f "${INSTALL_DIR}/${BINARY}"
  fi

  info "${BINARY} has been uninstalled"
}

# --- main ------------------------------------------------------------------

case "${1:-}" in
  --uninstall)
    uninstall
    ;;
  *)
    detect_target
    resolve_version
    download_and_install
    info "done!"
    ;;
esac
