#!/usr/bin/env bash
# Fetches a pinned, checksum-verified borgbackup Linux static binary for
# running the real-borg e2e tests (crates/borg-core/tests/e2e_backup_restore.rs)
# in a sandbox with no `borg` on $PATH and no package-manager install access.
#
# Usage:
#   scripts/fetch-borg-linux.sh
#   BORG_TEST_BIN=$(pwd)/.cache/borg-test-bin/borg \
#     cargo test -p borg-core --test e2e_backup_restore -- --nocapture
#
# Re-run is a no-op if the pinned binary is already present and verified.
set -euo pipefail

# Pin: borgbackup 1.4.4, linux-glibc231-x86_64 static build.
# Source: https://github.com/borgbackup/borg/releases/tag/1.4.4
BORG_VERSION="1.4.4"
BORG_ASSET="borg-linux-glibc231-x86_64"
BORG_URL="https://github.com/borgbackup/borg/releases/download/${BORG_VERSION}/${BORG_ASSET}"
BORG_SHA256="28d8053626bd375837ed4fdb4dda5ef29b2271dbe71a2c6a5749d8f8f0021c6d"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CACHE_DIR="${REPO_ROOT}/.cache/borg-test-bin"
DEST="${CACHE_DIR}/borg"

mkdir -p "${CACHE_DIR}"

verify_checksum() {
    local file="$1"
    local actual
    actual="$(sha256sum "${file}" | awk '{print $1}')"
    [ "${actual}" = "${BORG_SHA256}" ]
}

if [ -x "${DEST}" ] && verify_checksum "${DEST}"; then
    echo "borg ${BORG_VERSION} already present and verified at ${DEST}"
    exit 0
fi

echo "Fetching borg ${BORG_VERSION} (${BORG_ASSET}) from ${BORG_URL} ..."
TMP_FILE="$(mktemp "${CACHE_DIR}/.borg-download.XXXXXX")"
trap 'rm -f "${TMP_FILE}"' EXIT

if command -v curl >/dev/null 2>&1; then
    curl -fL --retry 3 -o "${TMP_FILE}" "${BORG_URL}"
elif command -v wget >/dev/null 2>&1; then
    wget -O "${TMP_FILE}" "${BORG_URL}"
else
    echo "error: need curl or wget to fetch borg" >&2
    exit 1
fi

if ! verify_checksum "${TMP_FILE}"; then
    echo "error: sha256 mismatch for downloaded borg binary" >&2
    echo "  expected: ${BORG_SHA256}" >&2
    echo "  actual:   $(sha256sum "${TMP_FILE}" | awk '{print $1}')" >&2
    exit 1
fi

chmod +x "${TMP_FILE}"
mv "${TMP_FILE}" "${DEST}"
trap - EXIT

echo "borg ${BORG_VERSION} installed and verified at ${DEST}"
"${DEST}" --version
