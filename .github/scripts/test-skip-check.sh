#!/usr/bin/env bash
# Tests for skip-check.sh. Run: bash .github/scripts/test-skip-check.sh
set -uo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
SCRIPT="$HERE/skip-check.sh"
TMP=$(mktemp -d)
PASS=0
FAIL=0

cleanup() { rm -rf "$TMP"; }
trap cleanup EXIT

pass() { PASS=$((PASS + 1)); }
fail() { FAIL=$((FAIL + 1)); printf 'FAIL: %s\n' "$1" >&2; }

new_repo() { # fresh repo with one commit, tagged v0.1.0 unless NOTAG=1
  rm -rf "$TMP/repo"
  mkdir -p "$TMP/repo"
  git -C "$TMP/repo" init -q .
  git -C "$TMP/repo" config user.email test@example.invalid
  git -C "$TMP/repo" config user.name test
  git -C "$TMP/repo" commit -q --allow-empty -m "feat: initial"
  if [ "${NOTAG:-}" != 1 ]; then git -C "$TMP/repo" tag v0.1.0; fi
}
commit() { git -C "$TMP/repo" commit -q --allow-empty -m "$1"; }

# decide <event> [bump input] -> prints the skip= line
decide() {
  (cd "$TMP/repo" && EVENT_NAME="$1" BUMP_INPUT="${2:-}" bash "$SCRIPT" 2>/dev/null)
}
expect() { # expected event [bump] ; description in $DESC
  local expected="$1" event="$2" bump="${3:-}" out
  out=$(decide "$event" "$bump")
  if [ "$out" = "skip=$expected" ]; then pass; else fail "$DESC [$event bump='$bump']: expected skip=$expected, got '$out'"; fi
}

DESC="no tag yet"; NOTAG=1 new_repo
expect false push; expect false schedule; expect false workflow_dispatch

DESC="no new commits"; new_repo
expect true push; expect true schedule; expect true workflow_dispatch
expect false workflow_dispatch patch      # an explicit bump always releases

DESC="only a dependency bump"; new_repo; commit "chore(deps): bump serde from 1.0 to 1.1"
expect true push                           # ships with the next real change...
expect false schedule                      # ...or with the weekly release
expect false workflow_dispatch             # a manual run always releases
expect false workflow_dispatch minor

DESC="several dependency bumps"; new_repo
commit "chore(deps): bump serde from 1.0 to 1.1"; commit "ci: bump the actions group with 3 updates"
expect true push; expect false schedule

DESC="a real change among them"; new_repo
commit "chore(deps): bump serde from 1.0 to 1.1"; commit "fix(core): a real fix"
expect false push; expect false schedule; expect false workflow_dispatch

DESC="a stranded release commit (failed tag push)"; new_repo; commit "chore(release): v0.2.0"
expect false push; expect false schedule   # recover: a stranded bump commit is released

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
