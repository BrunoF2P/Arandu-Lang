#!/usr/bin/env bash
# Atomic, versioned local install of Arandu (rustup-style layout).
#
# Layout:
#   $PREFIX/arandu-$VERSION/
#     bin/arandu_cli
#     bin/arandu          → arandu_cli
#     share/arandu/stdlib/
#     BLAKE3SUMS
#   $PREFIX/current       → arandu-$VERSION   (atomic ln -sfn)
#   $PREFIX/bin/arandu    → ../current/bin/arandu
#   $PREFIX/bin/arandu_cli→ ../current/bin/arandu_cli
#
# Stdlib resolution after install (Camada D):
#   current_exe() → canonicalize() → real bin/ → ../share/arandu/stdlib
#   PATH symlinks are safe because the binary always canonicalizes first.
#
# Usage:
#   ./scripts/install-local.sh                      # PREFIX=$HOME/.local/arandu
#   ./scripts/install-local.sh /opt/arandu
#   PREFIX=/tmp/arandu-test ./scripts/install-local.sh
#   SKIP_BUILD=1 ./scripts/install-local.sh         # reuse target/release/arandu_cli
#
# Env:
#   PREFIX       install root (default: $HOME/.local/arandu)
#   VERSION      override package version (default: crates/arandu_cli Cargo.toml)
#   SKIP_BUILD=1 skip cargo build (require target/release/arandu_cli)

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PREFIX="${1:-${PREFIX:-$HOME/.local/arandu}}"
VERSION="${VERSION:-}"
SKIP_BUILD="${SKIP_BUILD:-0}"

if [[ -z "$VERSION" ]]; then
  VERSION="$(
    sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT/crates/arandu_cli/Cargo.toml" | head -1
  )"
fi
if [[ -z "$VERSION" ]]; then
  echo "error: could not detect VERSION from crates/arandu_cli/Cargo.toml" >&2
  exit 1
fi

VERSION_NAME="arandu-${VERSION}"
VERSION_DIR="$PREFIX/$VERSION_NAME"
STAGE_PARENT="$PREFIX/.staging"
STAGE_DIR=""

cleanup() {
  if [[ -n "${STAGE_DIR:-}" && -d "${STAGE_DIR:-}" ]]; then
    rm -rf "$STAGE_DIR"
  fi
}
trap cleanup EXIT

echo "==> Arandu install-local"
echo "    PREFIX=$PREFIX"
echo "    VERSION=$VERSION"
echo "    layout=$VERSION_DIR"

if [[ "$SKIP_BUILD" != "1" ]]; then
  echo "==> building arandu_cli (release)"
  cargo build -p arandu_cli --release --manifest-path "$ROOT/Cargo.toml"
else
  echo "==> SKIP_BUILD=1 (using existing target/release/arandu_cli)"
fi

BIN_SRC="$ROOT/target/release/arandu_cli"
if [[ ! -x "$BIN_SRC" ]]; then
  echo "error: missing $BIN_SRC — build first or unset SKIP_BUILD" >&2
  exit 1
fi

if [[ ! -d "$ROOT/stdlib/std" && ! -d "$ROOT/stdlib/core" ]]; then
  echo "error: monorepo stdlib not found at $ROOT/stdlib" >&2
  exit 1
fi

mkdir -p "$PREFIX" "$STAGE_PARENT" "$PREFIX/bin"
STAGE_DIR="$(mktemp -d "$STAGE_PARENT/${VERSION_NAME}.XXXXXX")"
STAGE_TREE="$STAGE_DIR/$VERSION_NAME"

echo "==> staging into $STAGE_TREE"
mkdir -p "$STAGE_TREE/bin" "$STAGE_TREE/share/arandu"
install -m 755 "$BIN_SRC" "$STAGE_TREE/bin/arandu_cli"
ln -sfn arandu_cli "$STAGE_TREE/bin/arandu"
cp -a "$ROOT/stdlib" "$STAGE_TREE/share/arandu/stdlib"

# Integrity file for this version tree (BLAKE3 of binary + each .aru).
echo "==> writing BLAKE3SUMS"
{
  (
    cd "$STAGE_TREE"
    # shellcheck disable=SC2044
    for f in bin/arandu_cli $(find share/arandu/stdlib -type f -name '*.aru' | sort); do
      hash="$("$BIN_SRC" hash-file "$STAGE_TREE/$f")"
      printf '%s  %s\n' "$hash" "$f"
    done
  )
} >"$STAGE_TREE/BLAKE3SUMS"

# Atomic publish: move staged tree into place, then re-point symlinks.
echo "==> publishing $VERSION_DIR (atomic)"
if [[ -e "$VERSION_DIR" || -L "$VERSION_DIR" ]]; then
  BACKUP="${VERSION_DIR}.old.$$"
  rm -rf "$BACKUP"
  mv "$VERSION_DIR" "$BACKUP"
  rm -rf "$BACKUP"
fi
mv "$STAGE_TREE" "$VERSION_DIR"
# staging dir now empty / only leftovers
rmdir "$STAGE_DIR" 2>/dev/null || rm -rf "$STAGE_DIR"
STAGE_DIR=""

# Atomic symlink flips (ln -sfn is atomic on rename of the link inode).
ln -sfn "$VERSION_NAME" "$PREFIX/current"
ln -sfn "../current/bin/arandu" "$PREFIX/bin/arandu"
ln -sfn "../current/bin/arandu_cli" "$PREFIX/bin/arandu_cli"

echo "==> verifying install layout"
REAL_BIN="$(readlink -f "$PREFIX/bin/arandu" 2>/dev/null || python3 -c "import os; print(os.path.realpath('$PREFIX/bin/arandu'))")"
EXPECTED_STDLIB="$VERSION_DIR/share/arandu/stdlib"
if [[ ! -d "$EXPECTED_STDLIB/std" && ! -d "$EXPECTED_STDLIB/core" ]]; then
  echo "error: stdlib missing after install at $EXPECTED_STDLIB" >&2
  exit 1
fi

echo "==> doctor (via PATH symlink, ARANDU_STDLIB unset, cwd=/tmp)"
# Use a neutral cwd so monorepo discovery cannot mask install-layout bugs.
env -u ARANDU_STDLIB PATH="$PREFIX/bin:/usr/bin:/bin" \
  bash -c 'cd /tmp && exec "$0" doctor -v' "$PREFIX/bin/arandu"

echo
echo "installed $VERSION_NAME → $PREFIX"
echo "  current   → $VERSION_NAME"
echo "  bin/arandu → ../current/bin/arandu"
echo "  real bin  = $REAL_BIN"
echo "  stdlib    = $EXPECTED_STDLIB"
echo
echo "add to PATH: export PATH=\"$PREFIX/bin:\$PATH\""
echo "next: arandu doctor && arandu new hello"
