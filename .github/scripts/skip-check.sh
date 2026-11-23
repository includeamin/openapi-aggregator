#!/usr/bin/env bash
# Decide whether the release workflow has anything to release. Prints `skip=true|false` on
# stdout (append it to $GITHUB_OUTPUT); explanations go to stderr. Run from the repository.
#
# Environment:
#   EVENT_NAME  the triggering event: push, schedule or workflow_dispatch
#   BUMP_INPUT  the bump type given to a manual run (empty otherwise)
#
# Rules, relative to the last tag:
#   - no tag yet: release.
#   - no new commits: skip, unless a manual run asked for a specific bump.
#   - only dependency updates: skip for `push` (they ship with the next real change or the
#     weekly scheduled release); a `schedule` or manual run releases them.
#   - anything else: release.
set -euo pipefail

event="${EVENT_NAME:-}"
bump="${BUMP_INPUT:-}"
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

last_tag=$(git describe --tags --abbrev=0 2>/dev/null || echo "")
if [ -z "$last_tag" ]; then
  echo "skip=false"
  exit 0
fi

count=$(git rev-list "${last_tag}..HEAD" --count)
if [ "$count" -eq 0 ] && [ -z "$bump" ]; then
  echo "No new commits since ${last_tag}, skipping." >&2
  echo "skip=true"
elif [ "$event" = "push" ] \
  && [ "$(git log --format=%s "${last_tag}..HEAD" | bash "$here/release-skip.sh")" = "skip=true" ]; then
  echo "Only dependency updates since ${last_tag}; they ship with the next real change or the weekly release." >&2
  echo "skip=true"
else
  echo "${count} commit(s) since ${last_tag}" >&2
  echo "skip=false"
fi
