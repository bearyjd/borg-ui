#!/usr/bin/env bash
# Fails if release.yml uses a third-party action by mutable tag/branch instead
# of a full commit SHA. The release job holds signing credentials, so a moved
# tag on a third-party action could exfiltrate them. First-party `actions/*`
# may stay on version tags.
set -euo pipefail

WORKFLOW="${1:-.github/workflows/release.yml}"

bad=$(grep -nE '^\s*(-\s+)?uses:' "$WORKFLOW" \
  | grep -vE 'uses:\s*actions/' \
  | grep -vE 'uses:\s*[^@[:space:]]+@[0-9a-f]{40}(\s|$)' || true)

if [ -n "$bad" ]; then
  echo "Third-party actions in $WORKFLOW not pinned to a 40-char commit SHA:"
  echo "$bad"
  exit 1
fi
echo "All third-party actions in $WORKFLOW are pinned to commit SHAs."
