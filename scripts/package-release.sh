#!/usr/bin/env bash
# Build a versioned release tarball + BLAKE3 checksum (install gold bar #4).
#
# Output:
#   dist/arandu-$VERSION-$TARGET.tar.gz
#   dist/arandu-$VERSION-$TARGET.tar.gz.blake3   # single-line hex
#
# Tarball root:
#   arandu-$VERSION/
#     bin/arandu_cli
#     bin/arandu → arandu_cli
#     share/arandu/stdlib/
#     BLAKE3SUMS
#
# Install with: ./scripts/install-from-tarball.sh dist/arandu-….tar.gz

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${VERSION:-}"
TARGET="${TARGET:-$(rustc -vV | sed -n 's/^host: //p')}"
OUT_DIR="${OUT_DIR:-$ROOT/dist}"

if [[ -z "$VERSION" ]]; then
  VERSION="$(
    sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT/crates/arandu_cli/Cargo.toml" | head -1
  )"
fi

NAME="arandu-${VERSION}"
ARCHIVE_BASE="arandu-${VERSION}-${TARGET}"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

echo "==> package-release VERSION=$VERSION TARGET=$TARGET"

cargo build -p arandu_cli --release --manifest-path "$ROOT/Cargo.toml"
BIN="$ROOT/target/release/arandu_cli"

TREE="$STAGE/$NAME"
mkdir -p "$TREE/bin" "$TREE/share/arandu"
install -m 755 "$BIN" "$TREE/bin/arandu_cli"
ln -sfn arandu_cli "$TREE/bin/arandu"
cp -a "$ROOT/stdlib" "$TREE/share/arandu/stdlib"

{
  cd "$TREE"
  # shellcheck disable=SC2044
  for f in bin/arandu_cli $(find share/arandu/stdlib -type f -name '*.aru' | sort); do
    hash="$("$BIN" hash-file "$TREE/$f")"
    printf '%s  %s\n' "$hash" "$f"
  done
} >"$TREE/BLAKE3SUMS"

mkdir -p "$OUT_DIR"
TAR="$OUT_DIR/${ARCHIVE_BASE}.tar.gz"
(
  cd "$STAGE"
  tar -czf "$TAR" "$NAME"
)

# Tarball integrity (BLAKE3 of the archive bytes).
HASH="$("$BIN" hash-file "$TAR")"
printf '%s\n' "$HASH" >"${TAR}.blake3"
# Also a "hash  filename" form for convenience.
printf '%s  %s\n' "$HASH" "$(basename "$TAR")" >"${TAR}.blake3sum"

echo "==> wrote $TAR"
echo "    blake3 $HASH"
echo "    sidecar ${TAR}.blake3"
