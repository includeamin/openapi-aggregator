#!/usr/bin/env bash
# Publish openapi-aggregator-core and openapi-aggregator to crates.io, safely and idempotently.
#
# Environment:
#   EXPECTED_VERSION     optional; fail (before any upload) if the workspace version differs
#   DRY_RUN              "true" (default) packages and verifies without uploading; "false" publishes
#   CARGO_REGISTRY_TOKEN required for a real publish; when it is empty the publish is skipped
#   CRATES_API           crates.io API base (default https://crates.io/api/v1/crates)
#   CARGO_TOML           workspace manifest to read the version from (default Cargo.toml)
#   CARGO                cargo executable (default cargo)
#
# A crate whose exact version already exists on crates.io is skipped, so re-running after a
# partial failure (or after a manual catch-up) is safe. Can be sourced to reuse the functions.

CRATES="openapi-aggregator-core openapi-aggregator"
USER_AGENT="openapi-aggregator-release (https://github.com/includeamin/openapi-aggregator)"

log() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

# The workspace version: the first `version = "..."` in the manifest ([workspace.package]).
workspace_version() {
  local manifest="${1:-Cargo.toml}" version
  version=$(grep -m1 '^version' "$manifest" | sed 's/.*"\(.*\)".*/\1/')
  [ -n "$version" ] || die "could not read the workspace version from $manifest"
  printf '%s' "$version"
}

# crate_published <name> <version>: succeeds when that exact version is on crates.io.
crate_published() {
  local api="${CRATES_API:-https://crates.io/api/v1/crates}" code
  code=$(curl -s -o /dev/null -w '%{http_code}' -A "$USER_AGENT" "$api/$1/$2") \
    || die "could not reach the crates.io API for $1 $2"
  case "$code" in
    200) return 0 ;;
    404) return 1 ;;
    *) die "unexpected HTTP $code from the crates.io API for $1 $2" ;;
  esac
}

main() {
  local cargo="${CARGO:-cargo}" dry_run="${DRY_RUN:-true}" version crate published=0
  local excludes=() args=()

  version=$(workspace_version "${CARGO_TOML:-Cargo.toml}")
  if [ -n "${EXPECTED_VERSION:-}" ] && [ "$EXPECTED_VERSION" != "$version" ]; then
    die "version mismatch: the workspace is at $version but $EXPECTED_VERSION was expected"
  fi

  for crate in $CRATES; do
    if crate_published "$crate" "$version"; then
      log "$crate $version is already on crates.io; skipping it"
      excludes+=(--exclude "$crate")
      published=$((published + 1))
    fi
  done
  if [ "$published" -ge 2 ]; then
    log "Nothing to publish: every crate is already on crates.io at $version"
    return 0
  fi

  if [ "$dry_run" != "true" ] && [ -z "${CARGO_REGISTRY_TOKEN:-}" ]; then
    log "::notice::CARGO_REGISTRY_TOKEN is not set; skipping the crates.io publish for $version"
    return 0
  fi

  args=(publish --workspace)
  if [ "${#excludes[@]}" -gt 0 ]; then args+=("${excludes[@]}"); fi
  if [ "$dry_run" = "true" ]; then args+=(--dry-run); fi
  log "Running: cargo ${args[*]}"
  "$cargo" "${args[@]}"
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  set -euo pipefail
  main "$@"
fi
