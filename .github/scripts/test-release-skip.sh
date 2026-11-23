#!/usr/bin/env bash
# Tests for release-skip.sh. Run: bash .github/scripts/test-release-skip.sh
set -uo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
SCRIPT="$HERE/release-skip.sh"
PASS=0
FAIL=0

pass() { PASS=$((PASS + 1)); }
fail() { FAIL=$((FAIL + 1)); printf 'FAIL: %s\n' "$1" >&2; }

# expect <expected> <subject>... : feeds the subjects (one per line) to the script
expect() {
  local expected="$1" out
  shift
  out=$(printf '%s\n' "$@" | bash "$SCRIPT" 2>/dev/null | head -n 1)
  if [ "$out" = "skip=$expected" ]; then pass; else fail "expected skip=$expected for [$*], got '$out'"; fi
}

# dependency-only: skip
expect true "chore(deps): bump rustls from 0.23.44 to 0.23.46"
expect true "chore(deps-dev): bump vite from 7.1.0 to 7.3.0"
expect true "ci: bump the actions group with 10 updates"
expect true "chore(deps): bump the web-minor-and-patch group in /web with 8 updates" "ci: bump the actions group with 10 updates"
expect true "Merge pull request #5 from includeamin/dependabot/cargo/serde-1.0.230"
# the release bot's own commits are ignored when deciding
expect true "chore(release): v0.16.3" "chore(deps): bump thiserror from 1.0.69 to 2.0.20"

# anything else is a real change: release
expect false "feat: add glob sources"
expect false "fix(core): handle empty paths"
expect false "docs: document path prefixes"
expect false "chore: tidy the workspace"
expect false "fix(deps): patch a vulnerable parser"
expect false "chore(deps): bump serde from 1.0 to 1.1" "feat: use the new serde API"
expect false "chore(deps): bump serde from 1.0 to 1.1" "docs: update the readme"

# nothing to decide on: do not skip (the caller handles "no commits" itself)
expect false
expect false "chore(release): v0.16.3"

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
