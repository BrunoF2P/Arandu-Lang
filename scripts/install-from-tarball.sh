#!/usr/bin/env bash
# Install from a package-release tarball with BLAKE3 verify + atomic publish.
#
# Usage:
#   ./scripts/install-from-tarball.sh dist/arandu-0.0.1-x86_64-unknown-linux-gnu.tar.gz
#   PREFIX=/opt/arandu ./scripts/install-from-tarball.sh ./arandu-….tar.gz
#
# Expects optional sidecar:
#   <archive>.blake3        # single hex line, preferred
#   <archive>.blake3sum     # "hex  filename" form
#
# If no sidecar is present, installs with a warning (dev only).

set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <arandu-VERSION-TARGET.tar.gz>" >&2
  exit 2
fi

ARCHIVE="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
PREFIX="${PREFIX:-$HOME/.local/arandu}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ ! -f "$ARCHIVE" ]]; then
  echo "error: archive not found: $ARCHIVE" >&2
  exit 1
fi

# Prefer monorepo release binary (known to support hash-file); then PATH.
hash_file() {
  local f="$1"
  if [[ -x "$ROOT/target/release/arandu_cli" ]]; then
    "$ROOT/target/release/arandu_cli" hash-file "$f"
  elif [[ -x "$ROOT/target/debug/arandu_cli" ]]; then
    "$ROOT/target/debug/arandu_cli" hash-file "$f"
  elif command -v arandu_cli >/dev/null 2>&1; then
    arandu_cli hash-file "$f"
  elif command -v arandu >/dev/null 2>&1; then
    arandu hash-file "$f"
  else
    echo "error: need arandu_cli hash-file to verify BLAKE3 (build monorepo or install once)" >&2
    exit 1
  fi
}

EXPECTED=""
if [[ -f "${ARCHIVE}.blake3" ]]; then
  EXPECTED="$(tr -d '[:space:]' <"${ARCHIVE}.blake3")"
elif [[ -f "${ARCHIVE}.blake3sum" ]]; then
  EXPECTED="$(awk '{print $1; exit}' "${ARCHIVE}.blake3sum")"
fi

if [[ -n "$EXPECTED" ]]; then
  ACTUAL="$(hash_file "$ARCHIVE")"
  if [[ "$EXPECTED" != "$ACTUAL" ]]; then
    echo "error: BLAKE3 mismatch for $(basename "$ARCHIVE")" >&2
    echo "  expected: $EXPECTED" >&2
    echo "  actual:   $ACTUAL" >&2
    echo "  archive corrupt or tampered — aborting" >&2
    exit 1
  fi
  echo "==> BLAKE3 ok ($ACTUAL)"
else
  echo "warning: no ${ARCHIVE}.blake3 sidecar — skipping integrity check" >&2
fi

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

echo "==> extracting (staging)"
tar -xzf "$ARCHIVE" -C "$STAGE"

# Expect single top-level arandu-VERSION/
# Portable: no mapfile/process-substitution (macOS /bin/bash is 3.2).
TOP_COUNT=0
TREE=""
for d in "$STAGE"/*; do
  [[ -d "$d" ]] || continue
  TOP_COUNT=$((TOP_COUNT + 1))
  TREE="$d"
done
if [[ "$TOP_COUNT" -ne 1 || -z "$TREE" ]]; then
  echo "error: archive must contain exactly one top-level directory" >&2
  exit 1
fi
VERSION_NAME="$(basename "$TREE")"
VERSION_DIR="$PREFIX/$VERSION_NAME"

if [[ ! -x "$TREE/bin/arandu_cli" && ! -x "$TREE/bin/arandu" ]]; then
  echo "error: archive missing bin/arandu_cli" >&2
  exit 1
fi
if [[ ! -d "$TREE/share/arandu/stdlib" ]]; then
  echo "error: archive missing share/arandu/stdlib" >&2
  exit 1
fi

# Optional: verify in-tree BLAKE3SUMS against extracted files.
if [[ -f "$TREE/BLAKE3SUMS" ]]; then
  echo "==> verifying in-tree BLAKE3SUMS"
  while read -r hash path; do
    [[ -z "${hash:-}" || "$hash" =~ ^# ]] && continue
    actual="$(hash_file "$TREE/$path")"
    if [[ "$hash" != "$actual" ]]; then
      echo "error: BLAKE3SUMS mismatch for $path" >&2
      echo "  expected $hash" >&2
      echo "  actual   $actual" >&2
      exit 1
    fi
  done <"$TREE/BLAKE3SUMS"
fi

echo "==> atomic publish → $VERSION_DIR"
mkdir -p "$PREFIX" "$PREFIX/bin"
if [[ -e "$VERSION_DIR" || -L "$VERSION_DIR" ]]; then
  BACKUP="${VERSION_DIR}.old.$$"
  rm -rf "$BACKUP"
  mv "$VERSION_DIR" "$BACKUP"
  rm -rf "$BACKUP"
fi
mv "$TREE" "$VERSION_DIR"

ln -sfn "$VERSION_NAME" "$PREFIX/current"
ln -sfn "../current/bin/arandu" "$PREFIX/bin/arandu"
ln -sfn "../current/bin/arandu_cli" "$PREFIX/bin/arandu_cli"

echo "==> doctor"
env -u ARANDU_STDLIB PATH="$PREFIX/bin:/usr/bin:/bin" \
  "$PREFIX/bin/arandu" doctor

echo "installed $VERSION_NAME under $PREFIX"
