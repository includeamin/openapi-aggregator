#!/usr/bin/env bash
# Decide whether the commits since the last release are all dependency updates.
#
# Reads commit subjects from stdin, one per line, and prints `skip=true` when there is at least
# one commit and every one of them (ignoring the release bot's own `chore(release)` commits) is
# a dependency update: Dependabot's `chore(deps): ...`, `chore(deps-dev): ...` and
# `ci: bump ...` subjects, or a merge of a dependabot branch. Otherwise prints `skip=false`.
# The output is meant to be appended to $GITHUB_OUTPUT.
set -euo pipefail

dependency_only=true
seen=false

while IFS= read -r subject || [ -n "$subject" ]; do
  case "$subject" in
    "") continue ;;
    "chore(release)"*) continue ;;
  esac
  seen=true
  case "$subject" in
    "chore(deps): "* | "chore(deps-dev): "* | "ci: bump "*) ;;
    "Merge pull request #"*" from "*"/dependabot/"*) ;;
    *) dependency_only=false ;;
  esac
done

if [ "$seen" = true ] && [ "$dependency_only" = true ]; then
  echo "skip=true"
else
  echo "skip=false"
fi
