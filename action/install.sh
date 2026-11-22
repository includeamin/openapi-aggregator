#!/usr/bin/env bash
# Install a checksum-verified openapi-aggregator release binary.
#
# Environment:
#   VERSION                        release to install (empty: ACTION_REF if it is a version, else latest)
#   ACTION_REF                     the ref the action was invoked at (github.action_ref)
#   RUNNER_OS / RUNNER_ARCH        set by GitHub Actions
#   RUNNER_TEMP                    scratch directory (defaults to $TMPDIR or /tmp)
#   GITHUB_PATH                    when set, the install directory is appended to it
#   OPENAPI_AGGREGATOR_BASE_URL    download base (default: the GitHub releases of the repository)
#   OPENAPI_AGGREGATOR_LATEST_URL  URL that redirects to the latest release tag (for tests)
#
# Prints the absolute path of the installed binary as the last line of stdout; everything
# else goes to stderr. Can be sourced to reuse the functions.

REPO="includeamin/openapi-aggregator"

log() { printf '[openapi-aggregator] %s\n' "$*" >&2; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

# The release target for this runner.
detect_target() {
  case "${RUNNER_OS:-}:${RUNNER_ARCH:-}" in
    Linux:X64) echo "x86_64-unknown-linux-musl" ;;
    Linux:ARM64) echo "aarch64-unknown-linux-musl" ;;
    macOS:X64) echo "x86_64-apple-darwin" ;;
    macOS:ARM64) echo "aarch64-apple-darwin" ;;
    Windows:X64) echo "x86_64-pc-windows-msvc" ;;
    *)
      printf 'error: unsupported runner: os=%s arch=%s\n' "${RUNNER_OS:-?}" "${RUNNER_ARCH:-?}" >&2
      return 1
      ;;
  esac
}

# The v-prefixed release tag to install.
resolve_version() {
  local version="${VERSION:-}"
  if [ -z "$version" ] && printf '%s' "${ACTION_REF:-}" | grep -Eq '^v?[0-9]+\.[0-9]+\.[0-9]+'; then
    version="$ACTION_REF"
  fi
  if [ -z "$version" ]; then
    local url="${OPENAPI_AGGREGATOR_LATEST_URL:-https://github.com/$REPO/releases/latest}"
    local final
    final=$(curl -fsSL --retry 3 -o /dev/null -w '%{url_effective}' "$url") \
      || die "could not determine the latest release from $url"
    version="${final##*/}"
  fi
  case "$version" in
    v[0-9]*) echo "$version" ;;
    [0-9]*) echo "v$version" ;;
    *) die "could not determine a release version (got '$version')" ;;
  esac
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

# verify_checksum <file> <checksums.txt> <asset name>
verify_checksum() {
  local file="$1" sums="$2" asset="$3" expected actual
  expected=$(awk -v name="$asset" '{ f = $2; sub(/^\*/, "", f); if (f == name) { print $1; exit } }' "$sums")
  [ -n "$expected" ] || die "no checksum for $asset in checksums.txt"
  actual=$(sha256_of "$file")
  [ "$expected" = "$actual" ] || die "checksum mismatch for $asset (expected $expected, got $actual)"
}

extract() { # archive destination
  case "$1" in
    *.zip)
      if command -v unzip >/dev/null 2>&1; then
        unzip -q "$1" -d "$2"
      else
        local src="$1" dst="$2"
        if command -v cygpath >/dev/null 2>&1; then src=$(cygpath -w "$1"); dst=$(cygpath -w "$2"); fi
        powershell -NoProfile -Command "Expand-Archive -Force -Path '$src' -DestinationPath '$dst'"
      fi
      ;;
    *) tar xzf "$1" -C "$2" ;;
  esac
}

main() {
  local target tag ext bin asset base scratch work install_dir
  target=$(detect_target)
  tag=$(resolve_version)
  case "$target" in
    *windows*) ext="zip"; bin="openapi-aggregator.exe" ;;
    *) ext="tar.gz"; bin="openapi-aggregator" ;;
  esac
  asset="openapi-aggregator-${target}.${ext}"
  base="${OPENAPI_AGGREGATOR_BASE_URL:-https://github.com/$REPO/releases/download}/$tag"
  scratch="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
  mkdir -p "$scratch"
  work=$(mktemp -d "$scratch/openapi-aggregator-download.XXXXXX")
  trap 'rm -rf "$work"' EXIT

  log "installing $tag for $target"
  curl -fsSL --retry 3 -o "$work/$asset" "$base/$asset" \
    || die "download failed: $base/$asset (does release $tag exist and have a $target build?)"
  curl -fsSL --retry 3 -o "$work/checksums.txt" "$base/checksums.txt" \
    || die "could not download checksums.txt for $tag; releases from before checksums were published cannot be verified, pin a newer version"
  verify_checksum "$work/$asset" "$work/checksums.txt" "$asset"

  install_dir="$scratch/openapi-aggregator-$tag"
  mkdir -p "$install_dir"
  extract "$work/$asset" "$install_dir"
  chmod +x "$install_dir/$bin"

  if [ -n "${GITHUB_PATH:-}" ]; then
    local path_entry="$install_dir"
    if command -v cygpath >/dev/null 2>&1; then path_entry=$(cygpath -w "$install_dir"); fi
    printf '%s\n' "$path_entry" >> "$GITHUB_PATH"
  fi
  echo "$install_dir/$bin"
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  set -euo pipefail
  main "$@"
fi
