#!/usr/bin/env bash
# Decide whether a release is a "feature release" that should be published to crates.io.
#
# Usage: release-flags.sh <bump> <version>
#   bump     major | minor | patch
#   version  the new version, e.g. 0.16.0 or 0.16.0-rc.1
#
# Prints `publish=true` for a minor or major bump of a stable version, `publish=false`
# otherwise (patch releases, dependency bumps and pre-releases). The output is meant to be
# appended to $GITHUB_OUTPUT.
set -euo pipefail

bump="${1:-}"
version="${2:-}"
if [ -z "$bump" ] || [ -z "$version" ]; then
  echo "usage: release-flags.sh <bump> <version>" >&2
  exit 1
fi

case "$bump" in
  minor | major) publish=true ;;
  patch) publish=false ;;
  *)
    echo "error: unknown bump type '$bump' (expected major, minor or patch)" >&2
    exit 1
    ;;
esac

# pre-releases are never published
case "$version" in
  *-*) publish=false ;;
  *) ;;
esac

echo "publish=$publish"
