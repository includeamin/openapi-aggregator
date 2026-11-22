#!/usr/bin/env bash
# Tests for release-flags.sh. Run: bash .github/scripts/test-release-flags.sh
set -uo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
SCRIPT="$HERE/release-flags.sh"
PASS=0
FAIL=0

pass() { PASS=$((PASS + 1)); }
fail() { FAIL=$((FAIL + 1)); printf 'FAIL: %s\n' "$1" >&2; }

expect() { # bump version expected-output
  local out
  out=$(bash "$SCRIPT" "$1" "$2" 2>/dev/null)
  if [ "$out" = "$3" ]; then pass; else fail "$1 $2: expected '$3', got '$out'"; fi
}

expect minor 0.16.0 publish=true
expect major 1.0.0 publish=true
expect patch 0.15.1 publish=false
expect minor 0.16.0-rc.1 publish=false
expect major 1.0.0-beta publish=false
expect patch 0.15.1-rc.1 publish=false

# an unknown bump type is an error that names the bump
if err=$(bash "$SCRIPT" bogus 1.0.0 2>&1 >/dev/null); then
  fail "unknown bump must fail"
else
  case "$err" in *bogus*) pass ;; *) fail "unknown bump error should mention 'bogus': $err" ;; esac
fi

# missing arguments are an error
if bash "$SCRIPT" >/dev/null 2>&1; then fail "no arguments must fail"; else pass; fi
if bash "$SCRIPT" minor >/dev/null 2>&1; then fail "missing version must fail"; else pass; fi

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
