#!/usr/bin/env bash
# Run openapi-aggregator for the GitHub Action: build the arguments from the inputs, run it,
# map the exit code to a `result` output, annotate failures and write the job summary.
#
# Environment (all inputs arrive here, never interpolated into script text):
#   BINARY               the openapi-aggregator binary to run
#   INPUT_CONFIG         config file (default openapi-aggregator.yaml)
#   INPUT_OUTPUT         committed spec to compare (check) or to write (write)
#   INPUT_DENY_WARNINGS  "true" to pass --deny-warnings (check mode)
#   INPUT_ALLOW_OUTSIDE  "true" to pass --allow-outside-config-dir
#   INPUT_WORKDIR        directory to run in (default .)
#   INPUT_MODE           "check" (default, never writes) or "write"
#   GITHUB_OUTPUT, GITHUB_STEP_SUMMARY, RUNNER_TEMP   set by GitHub Actions
#
# Can be sourced to reuse build_args.

MAX_LOG_BYTES=102400

# Fill the ARGS array from the INPUT_* variables.
build_args() {
  local mode="${INPUT_MODE:-check}"
  local config="${INPUT_CONFIG:-openapi-aggregator.yaml}"
  local output="${INPUT_OUTPUT:-}"
  ARGS=()
  case "$mode" in
    check)
      ARGS+=(--check -c "$config")
      if [ -n "$output" ]; then ARGS+=(-o "$output"); fi
      if [ "${INPUT_DENY_WARNINGS:-false}" = "true" ]; then ARGS+=(--deny-warnings); fi
      ;;
    write)
      if [ -z "$output" ]; then
        echo "error: mode 'write' requires the 'output' input" >&2
        return 1
      fi
      ARGS+=(-c "$config" -o "$output")
      ;;
    *)
      echo "error: unknown mode '$mode' (expected 'check' or 'write')" >&2
      return 1
      ;;
  esac
  if [ "${INPUT_ALLOW_OUTSIDE:-false}" = "true" ]; then ARGS+=(--allow-outside-config-dir); fi
  return 0
}

# Escape text for the data part of a GitHub workflow command.
escape_data() {
  local s="$1"
  s="${s//'%'/'%25'}"
  s="${s//$'\r'/'%0D'}"
  s="${s//$'\n'/'%0A'}"
  printf '%s' "$s"
}

write_summary() { # heading, fence language, log file
  [ -n "${GITHUB_STEP_SUMMARY:-}" ] || return 0
  {
    printf '## openapi-aggregator\n\n%s\n\n' "$1"
    if [ -s "$3" ]; then
      printf '```%s\n' "$2"
      head -c "$MAX_LOG_BYTES" "$3"
      printf '\n```\n'
      if [ "$(wc -c < "$3" | tr -d ' ')" -gt "$MAX_LOG_BYTES" ]; then
        printf '\n_Output truncated to 100 KB._\n'
      fi
    fi
  } >> "$GITHUB_STEP_SUMMARY"
}

set_result() {
  if [ -n "${GITHUB_OUTPUT:-}" ]; then printf 'result=%s\n' "$1" >> "$GITHUB_OUTPUT"; fi
}

main() {
  local mode="${INPUT_MODE:-check}"
  local config="${INPUT_CONFIG:-openapi-aggregator.yaml}"
  local output="${INPUT_OUTPUT:-}"
  local workdir="${INPUT_WORKDIR:-.}"
  local binary="${BINARY:?BINARY is not set}"

  # A relative path to the binary is relative to where we were started, not to workdir.
  case "$binary" in
    /* | [A-Za-z]:*) ;;
    */*) binary="$PWD/$binary" ;;
    *) ;;
  esac

  build_args || exit 1
  cd "$workdir" || { echo "error: working-directory '$workdir' does not exist" >&2; exit 1; }

  local log
  log=$(mktemp "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/openapi-aggregator-log.XXXXXX")
  local code=0
  "$binary" "${ARGS[@]}" 2> "$log" || code=$?
  cat "$log" >&2

  if [ "$mode" = "write" ]; then
    if [ "$code" -eq 0 ]; then
      write_summary "Regenerated \`$output\`." text /dev/null
    else
      printf '::error title=Could not regenerate the spec::%s\n' "openapi-aggregator exited with code $code"
      write_summary "**Failed to regenerate \`$output\`** (exit code $code)" text "$log"
    fi
    rm -f "$log"
    exit "$code"
  fi

  case "$code" in
    0)
      set_result passed
      write_summary "✅ Check passed: the merged spec is valid and up to date." text /dev/null
      ;;
    2)
      set_result stale
      printf '::error title=Generated spec is out of date::%s\n' \
        "$(escape_data "run \"openapi-aggregator -c $config -o $output\" and commit the result")"
      write_summary "❌ **The committed spec is out of date.** Regenerate it and commit the result." diff "$log"
      ;;
    1)
      set_result invalid
      printf '::error title=Validation failed::%s\n' "see the findings in the job summary"
      write_summary "❌ **Validation failed.**" text "$log"
      ;;
    *)
      set_result invalid
      printf '::error title=Validation failed::%s\n' "openapi-aggregator exited with code $code"
      write_summary "❌ **openapi-aggregator failed** (exit code $code)." text "$log"
      ;;
  esac
  rm -f "$log"
  exit "$code"
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  set -euo pipefail
  main "$@"
fi
