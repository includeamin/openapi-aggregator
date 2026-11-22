#!/usr/bin/env bash
# Offline tests for publish-crates.sh. Run: bash .github/scripts/test-publish-crates.sh
set -uo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
SCRIPT="$HERE/publish-crates.sh"
TMP=$(mktemp -d)
SERVER_PID=""
PASS=0
FAIL=0
FAKE_TOKEN="crates-io-secret-token-1234"

cleanup() {
  if [ -n "$SERVER_PID" ]; then kill "$SERVER_PID" 2>/dev/null; fi
  rm -rf "$TMP"
}
trap cleanup EXIT

pass() { PASS=$((PASS + 1)); }
fail() { FAIL=$((FAIL + 1)); printf 'FAIL: %s\n' "$1" >&2; }
assert_eq() { if [ "$1" = "$2" ]; then pass; else fail "$3: expected '$2', got '$1'"; fi; }
assert_contains() { case "$1" in *"$2"*) pass ;; *) fail "$3: '$2' not found in: $1" ;; esac; }
assert_not_contains() { case "$1" in *"$2"*) fail "$3: '$2' unexpectedly found" ;; *) pass ;; esac; }

# --- fakes -----------------------------------------------------------------

mkdir -p "$TMP/bin"
cat > "$TMP/bin/cargo" <<'FAKE'
#!/bin/sh
# records `publish` invocations (arguments and whether a token was present)
if [ "$1" = "publish" ]; then
  shift
  printf 'publish %s | token=%s\n' "$*" "${CARGO_REGISTRY_TOKEN:+set}" >> "$FAKE_LOG"
  exit "${FAKE_PUBLISH_EXIT:-0}"
fi
echo "unexpected cargo call: $*" >&2
exit 99
FAKE
chmod +x "$TMP/bin/cargo"

# fake crates.io: GET /<crate>/<version> is 200 when listed in $TMP/published (one
# "crate/version" per line), 404 otherwise, and 500 when the file contains ERROR
cat > "$TMP/server.py" <<'PY'
import http.server, os, sys
published = sys.argv[1]
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        lines = open(published).read().split()
        if "ERROR" in lines:
            self.send_response(500)
        elif self.path.lstrip("/") in lines:
            self.send_response(200)
        else:
            self.send_response(404)
        self.end_headers()
    def log_message(self, *a): pass
srv = http.server.HTTPServer(("127.0.0.1", 0), H)
print(srv.server_address[1], flush=True)
srv.serve_forever()
PY
: > "$TMP/published"
python3 "$TMP/server.py" "$TMP/published" > "$TMP/port" &
SERVER_PID=$!
for _ in $(seq 1 50); do [ -s "$TMP/port" ] && break; sleep 0.1; done
PORT=$(cat "$TMP/port")

write_cargo_toml() { printf '[workspace.package]\nversion = "%s"\nedition = "2021"\n' "$1" > "$TMP/Cargo.toml"; }

# run <published-lines> [env assignments...]; sets OUT (stdout+stderr), CODE, and $TMP/log
run() {
  printf '%s\n' "$1" > "$TMP/published"
  shift
  : > "$TMP/log"
  OUT=$(env CARGO="$TMP/bin/cargo" CARGO_TOML="$TMP/Cargo.toml" CRATES_API="http://127.0.0.1:$PORT" \
    FAKE_LOG="$TMP/log" DRY_RUN=false CARGO_REGISTRY_TOKEN="$FAKE_TOKEN" EXPECTED_VERSION="" "$@" \
    bash "$SCRIPT" 2>&1)
  CODE=$?
  LOG=$(cat "$TMP/log")
}

write_cargo_toml 0.16.0

# nothing published yet: one workspace publish, no excludes
run ""
assert_eq "$CODE" "0" "fresh publish succeeds"
assert_eq "$LOG" "publish --workspace | token=set" "fresh publish runs a plain workspace publish"

# core already published: only the CLI crate is published
run "openapi-aggregator-core/0.16.0"
assert_eq "$CODE" "0" "partial re-run succeeds"
assert_eq "$LOG" "publish --workspace --exclude openapi-aggregator-core | token=set" "core is excluded"

# both published: nothing to do
run "openapi-aggregator-core/0.16.0
openapi-aggregator/0.16.0"
assert_eq "$CODE" "0" "full re-run succeeds"
assert_eq "$LOG" "" "full re-run publishes nothing"
assert_contains "$OUT" "already on crates.io" "full re-run says why"

# other versions on crates.io do not count
run "openapi-aggregator-core/0.9.0
openapi-aggregator/0.9.0"
assert_eq "$LOG" "publish --workspace | token=set" "older versions do not block a publish"

# version mismatch fails before any upload
run "" EXPECTED_VERSION=9.9.9
if [ "$CODE" -ne 0 ]; then pass; else fail "version mismatch must fail"; fi
assert_contains "$OUT" "version mismatch" "mismatch message"
assert_eq "$LOG" "" "no publish on mismatch"
run "" EXPECTED_VERSION=0.16.0
assert_eq "$CODE" "0" "matching expected version publishes"

# no token on a real run: skip with a notice, succeed
run "" CARGO_REGISTRY_TOKEN=
assert_eq "$CODE" "0" "no token still succeeds"
assert_eq "$LOG" "" "no token publishes nothing"
assert_contains "$OUT" "CARGO_REGISTRY_TOKEN is not set" "no token notice"

# dry run works without a token
run "" DRY_RUN=true CARGO_REGISTRY_TOKEN=
assert_eq "$CODE" "0" "dry run succeeds"
assert_eq "$LOG" "publish --workspace --dry-run | token=" "dry run passes --dry-run"

# registry errors are failures, not 'not published'
run "ERROR"
if [ "$CODE" -ne 0 ]; then pass; else fail "an API error must fail"; fi
assert_eq "$LOG" "" "no publish when the registry check failed"

# a failing cargo publish fails the script
run "" FAKE_PUBLISH_EXIT=1
if [ "$CODE" -ne 0 ]; then pass; else fail "cargo failure must fail the script"; fi

# the token never shows up in the output
run ""
assert_not_contains "$OUT" "$FAKE_TOKEN" "token not printed (success)"
run "" FAKE_PUBLISH_EXIT=1
assert_not_contains "$OUT" "$FAKE_TOKEN" "token not printed (failure)"

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
