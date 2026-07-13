#!/bin/sh
# Install/uninstall script for openapi-aggregator
# Usage:
#   Install:    curl -sSfL https://raw.githubusercontent.com/includeamin/openapi-aggregator/main/install.sh | sh
#   Uninstall:  curl -sSfL https://raw.githubusercontent.com/includeamin/openapi-aggregator/main/install.sh | sh -s -- --uninstall
#
# Options (via environment variables):
#   VERSION     - specific version to install (e.g. v0.1.0). Defaults to latest.
#   INSTALL_DIR - directory to install to. Defaults to ~/.local/bin.
#   OPENAPI_AGGREGATOR_HOME - if set (and INSTALL_DIR is unset), installs to
#                 $OPENAPI_AGGREGATOR_HOME/bin.

set -e

REPO="includeamin/openapi-aggregator"
BINARY="openapi-aggregator"
INSTALL_DIR="${INSTALL_DIR:-}"
APP_HOME="${OPENAPI_AGGREGATOR_HOME:-}"
DEFAULT_INSTALL_DIR="$HOME/.local/bin"

# --- helpers ---------------------------------------------------------------

info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$1"; }
error() { printf '\033[1;31m[error]\033[0m %s\n' "$1" >&2; exit 1; }

print_path_fix_hint() {
  shell_path="${SHELL:-}"
  case "$shell_path" in
    */*) shell_name="${shell_path##*/}" ;;
    *)   shell_name="$shell_path" ;;
  esac

  [ -n "$shell_name" ] || shell_name="unknown"

  case "$shell_name" in
    zsh)
      rc_file="$HOME/.zshrc"
      ;;
    bash)
      rc_file="$HOME/.bashrc"
      ;;
    fish)
      rc_file="$HOME/.config/fish/config.fish"
      ;;
    *)
      rc_file="$HOME/.profile"
      ;;
  esac

  info "${BINARY} is installed but not on PATH in this shell session."
  printf '%s\n' ""
  printf '%s\n' "Run this now:"

  if [ "$shell_name" = "fish" ]; then
    printf '  %s\n' "set -Ux fish_user_paths ${INSTALL_DIR} \$fish_user_paths"
    printf '  %s\n' "exec fish"
  else
    printf '  %s\n' "echo 'export PATH=\"${INSTALL_DIR}:\$PATH\"' >> ${rc_file}"
    printf '  %s\n' ". ${rc_file}"
  fi

  printf '%s\n' ""
}

need() {
  command -v "$1" >/dev/null 2>&1 || error "required command not found: $1"
}

resolve_install_dir() {
  if [ -n "$INSTALL_DIR" ]; then
    return
  fi

  if [ -n "$APP_HOME" ]; then
    INSTALL_DIR="${APP_HOME}/bin"
  else
    INSTALL_DIR="$DEFAULT_INSTALL_DIR"
  fi
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

    # Prefer GitHub API, but include a User-Agent to avoid 403 responses.
    TAG=$(curl -sSfL \
      -H "Accept: application/vnd.github+json" \
      -H "User-Agent: ${BINARY}-install-script" \
      "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null \
      | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/' || true)

    # Fallback for environments where GitHub API is blocked or rate-limited.
    if [ -z "$TAG" ]; then
      info "GitHub API unavailable; resolving latest release via redirect..."
      TAG=$(curl -sSLI -o /dev/null -w '%{url_effective}' \
        "https://github.com/${REPO}/releases/latest" 2>/dev/null \
        | sed -n 's|.*/releases/tag/\([^/?#]*\).*|\1|p' | head -1 || true)
    fi

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
  mkdir -p "${INSTALL_DIR}"
  mv "${TMPDIR}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
  chmod +x "${INSTALL_DIR}/${BINARY}"

  info "installed ${BINARY} to ${INSTALL_DIR}/${BINARY}"
  "${INSTALL_DIR}/${BINARY}" --version 2>/dev/null || true

  # Remind user to add to PATH if needed
  case ":$PATH:" in
    *":${INSTALL_DIR}:"*) ;;
    *) print_path_fix_hint ;;
  esac
}

# --- uninstall -------------------------------------------------------------

uninstall() {
  if [ -n "$INSTALL_DIR" ]; then
    target_path="${INSTALL_DIR}/${BINARY}"
  elif [ -n "$APP_HOME" ]; then
    target_path="${APP_HOME}/bin/${BINARY}"
  else
    target_path="$(command -v "$BINARY" 2>/dev/null || true)"
    [ -n "$target_path" ] || target_path="${DEFAULT_INSTALL_DIR}/${BINARY}"
  fi

  if [ ! -f "$target_path" ]; then
    error "${BINARY} not found (set INSTALL_DIR to uninstall from a custom location)"
  fi

  info "removing ${target_path}"
  rm -f "$target_path"

  info "${BINARY} has been uninstalled"
}

# --- main ------------------------------------------------------------------

case "${1:-}" in
  --uninstall)
    uninstall
    ;;
  *)
    resolve_install_dir
    detect_target
    resolve_version
    download_and_install
    info "done!"
    ;;
esac
