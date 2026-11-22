#!/usr/bin/env bash
# Offline tests for action/install.sh and action/run.sh. Run: bash action/test.sh
set -uo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
TMP=$(mktemp -d)
SERVER_PID=""
PASS=0
FAIL=0

cleanup() {
  [ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2>/dev/null
  rm -rf "$TMP"
}
trap cleanup EXIT

pass() { PASS=$((PASS + 1)); }
fail() { FAIL=$((FAIL + 1)); printf 'FAIL: %s\n' "$1" >&2; }

assert_eq() { # actual expected message
  if [ "$1" = "$2" ]; then pass; else fail "$3: expected '$2', got '$1'"; fi
}
assert_contains() { # haystack needle message
  case "$1" in *"$2"*) pass ;; *) fail "$3: '$2' not found in: $1" ;; esac
}
assert_not_contains() { # haystack needle message
  case "$1" in *"$2"*) fail "$3: '$2' unexpectedly found" ;; *) pass ;; esac
}
assert_file() { if [ -e "$1" ]; then pass; else fail "$2: missing file $1"; fi; }
assert_no_file() { if [ -e "$1" ]; then fail "$2: unexpected file $1"; else pass; fi; }

# --- helpers ---------------------------------------------------------------

host_os() { case "$(uname -s)" in Linux) echo Linux ;; Darwin) echo macOS ;; *) echo Windows ;; esac; }
host_arch() { case "$(uname -m)" in x86_64|amd64) echo X64 ;; *) echo ARM64 ;; esac; }
sha256() { if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'; else shasum -a 256 "$1" | awk '{print $1}'; fi; }

# --- tests for install.sh --------------------------------------------------

test_install() {
  # shellcheck source=install.sh
  . "$HERE/install.sh"

  # detect_target
  t() { RUNNER_OS=$1 RUNNER_ARCH=$2 detect_target 2>/dev/null; }
  assert_eq "$(t Linux X64)" x86_64-unknown-linux-musl "linux x64"
  assert_eq "$(t Linux ARM64)" aarch64-unknown-linux-musl "linux arm64"
  assert_eq "$(t macOS X64)" x86_64-apple-darwin "macos x64"
  assert_eq "$(t macOS ARM64)" aarch64-apple-darwin "macos arm64"
  assert_eq "$(t Windows X64)" x86_64-pc-windows-msvc "windows x64"
  if t Windows ARM64 >/dev/null; then fail "windows arm64 should be unsupported"; else pass; fi
  if t Plan9 X64 >/dev/null; then fail "plan9 should be unsupported"; else pass; fi

  # a tiny release server: /latest redirects to /tag/v9.9.9, everything else is served from releases/
  mkdir -p "$TMP/releases/download/v1.2.3"
  cat > "$TMP/server.py" <<'PY'
import http.server, os, sys
root = sys.argv[1]
class H(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *a, **k):
        super().__init__(*a, directory=root, **k)
    def do_GET(self):
        if self.path == "/latest":
            self.send_response(302); self.send_header("Location", "/tag/v9.9.9"); self.end_headers(); return
        if self.path.startswith("/tag/"):
            self.send_response(200); self.end_headers(); self.wfile.write(b"ok"); return
        super().do_GET()
    def log_message(self, *a): pass
srv = http.server.HTTPServer(("127.0.0.1", 0), H)
print(srv.server_address[1], flush=True)
srv.serve_forever()
PY
  python3 "$TMP/server.py" "$TMP/releases" > "$TMP/port" &
  SERVER_PID=$!
  for _ in $(seq 1 50); do [ -s "$TMP/port" ] && break; sleep 0.1; done
  PORT=$(cat "$TMP/port")
  BASE="http://127.0.0.1:$PORT/download"

  # resolve_version
  r() { VERSION=$1 ACTION_REF=$2 OPENAPI_AGGREGATOR_LATEST_URL="http://127.0.0.1:$PORT/latest" resolve_version; }
  assert_eq "$(r '' main)" v9.9.9 "branch ref falls back to latest"
  assert_eq "$(r '' '')" v9.9.9 "empty ref falls back to latest"
  assert_eq "$(r '' v0.14.0)" v0.14.0 "tag ref"
  assert_eq "$(r '' 0.14.0)" v0.14.0 "tag ref without v"
  assert_eq "$(r '' v0.14.0-rc.1)" v0.14.0-rc.1 "pre-release ref"
  assert_eq "$(r 1.5.0 main)" v1.5.0 "explicit version wins and is normalised"
  assert_eq "$(r v2.0.0 v0.14.0)" v2.0.0 "explicit version beats the ref"

  # a fake release for the host platform
  OS=$(host_os); ARCH=$(host_arch)
  TARGET=$(RUNNER_OS=$OS RUNNER_ARCH=$ARCH detect_target)
  ASSET="openapi-aggregator-$TARGET.tar.gz"
  mkdir -p "$TMP/stub"
  printf '#!/bin/sh\necho "stub 1.2.3"\n' > "$TMP/stub/openapi-aggregator"
  chmod +x "$TMP/stub/openapi-aggregator"
  RELEASE="$TMP/releases/download/v1.2.3"
  tar czf "$RELEASE/$ASSET" -C "$TMP/stub" openapi-aggregator
  printf '%s  %s\n' "$(sha256 "$RELEASE/$ASSET")" "$ASSET" > "$RELEASE/checksums.txt"

  runi() { # extra env assignments are passed through; prints stdout, stderr goes to $TMP/err
    (
      export RUNNER_OS=$OS RUNNER_ARCH=$ARCH RUNNER_TEMP="$TMP/runner" GITHUB_PATH="$TMP/github_path" \
        VERSION=v1.2.3 ACTION_REF="" OPENAPI_AGGREGATOR_BASE_URL="$BASE"
      mkdir -p "$RUNNER_TEMP"
      "$@" bash "$HERE/install.sh" 2> "$TMP/err"
    )
  }

  # success
  : > "$TMP/github_path"
  out=$(runi env)
  bin=$(printf '%s\n' "$out" | tail -n 1)
  assert_file "$bin" "installed binary exists"
  assert_eq "$("$bin")" "stub 1.2.3" "installed binary runs"
  assert_contains "$(cat "$TMP/github_path")" "$(dirname "$bin")" "GITHUB_PATH updated"

  # tampered archive
  rm -rf "$TMP/runner"; : > "$TMP/github_path"
  cp "$RELEASE/$ASSET" "$TMP/asset.good"
  printf 'tampered' >> "$RELEASE/$ASSET"
  if runi env >/dev/null; then fail "tampered archive must fail"; else pass; fi
  assert_contains "$(cat "$TMP/err")" "checksum" "mismatch error mentions checksum"
  assert_eq "$(cat "$TMP/github_path")" "" "GITHUB_PATH untouched after mismatch"
  assert_eq "$(find "$TMP/runner" -name openapi-aggregator -type f 2>/dev/null | wc -l | tr -d ' ')" "0" "nothing extracted after mismatch"
  cp "$TMP/asset.good" "$RELEASE/$ASSET"

  # missing checksums.txt
  mv "$RELEASE/checksums.txt" "$TMP/checksums.bak"
  if runi env >/dev/null; then fail "missing checksums.txt must fail"; else pass; fi
  assert_contains "$(cat "$TMP/err")" "checksums.txt" "missing checksums error"
  mv "$TMP/checksums.bak" "$RELEASE/checksums.txt"

  # unknown version
  if runi env VERSION=v7.7.7 >/dev/null; then fail "unknown version must fail"; else pass; fi
  assert_contains "$(cat "$TMP/err")" "v7.7.7" "unknown version error names the version"

  # unsupported platform
  if runi env RUNNER_OS=Plan9 >/dev/null; then fail "unsupported platform must fail"; else pass; fi
  assert_contains "$(cat "$TMP/err")" "unsupported" "unsupported platform error"
}

test_install

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
