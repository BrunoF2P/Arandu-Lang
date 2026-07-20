#!/usr/bin/env bash
# Isolated install smoke (Camada D gold).
#
# Proves:
#   1. Versioned atomic install works outside monorepo cwd
#   2. ARANDU_STDLIB is unset (no env rescue)
#   3. PATH uses only the install prefix + system bins
#   4. PATH symlink → real versioned bin still finds stdlib (canonicalize)
#   5. doctor / new / check / run work from a clean temp project dir
#
# Usage (from monorepo root, or any cwd):
#   ./scripts/smoke-install.sh
#
# CI: github job install-smoke runs this on a clean runner.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SMOKE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/arandu-smoke.XXXXXX")"
PREFIX="$SMOKE_ROOT/prefix"
WORKDIR="$SMOKE_ROOT/work"
EXTRA_PATH="$SMOKE_ROOT/path-bin"

cleanup() {
  rm -rf "$SMOKE_ROOT"
}
trap cleanup EXIT

echo "==> smoke-install root=$SMOKE_ROOT"
mkdir -p "$WORKDIR" "$EXTRA_PATH"

# Build once, install with SKIP_BUILD for speed on re-runs if desired.
echo "==> release build"
cargo build -p arandu_cli --release --manifest-path "$ROOT/Cargo.toml"

echo "==> atomic versioned install → $PREFIX"
SKIP_BUILD=1 PREFIX="$PREFIX" "$ROOT/scripts/install-local.sh" "$PREFIX"

# Deliberate PATH symlink outside the prefix (classic /usr/local/bin case).
REAL_BIN="$(readlink -f "$PREFIX/bin/arandu" 2>/dev/null || python3 -c "import os; print(os.path.realpath(r'''$PREFIX/bin/arandu'''))")"
ln -sfn "$REAL_BIN" "$EXTRA_PATH/arandu"
ln -sfn "$REAL_BIN" "$EXTRA_PATH/arandu_cli"

# Run a command with: clean cwd (not monorepo), no ARANDU_STDLIB, PATH=prefix only.
# Usage: run_clean <path-dir> <cmd> [args...]
run_clean() {
  local path_dir="$1"
  shift
  (
    cd "$WORKDIR"
    env -u ARANDU_STDLIB -u CARGO_HOME \
      PATH="${path_dir}:/usr/bin:/bin" \
      HOME="$SMOKE_ROOT/home" \
      "$@"
  )
}

echo "==> doctor via prefix/bin (install symlink)"
OUT="$(run_clean "$PREFIX/bin" arandu doctor -v)"
echo "$OUT"
echo "$OUT" | grep -q "No issues found"
echo "$OUT" | grep -qi "Stdlib"
echo "$OUT" | grep -q "share/arandu/stdlib\|resolved via relative to binary"
# cwd must not be the monorepo (project skip or package only under workdir)
echo "$OUT" | grep -q "$WORKDIR\|no package found"

echo "==> doctor via EXTRA path symlink (must canonicalize)"
OUT2="$(run_clean "$EXTRA_PATH" arandu doctor -v)"
echo "$OUT2"
echo "$OUT2" | grep -q "No issues found"
# Must mention resolved/symlink path or still find stdlib under versioned tree.
echo "$OUT2" | grep -E -q "arandu-[0-9]|share/arandu/stdlib|symlink followed|resolved path"

echo "==> new + check + run outside monorepo"
run_clean "$PREFIX/bin" arandu new smoke_app
(
  cd "$WORKDIR/smoke_app"
  env -u ARANDU_STDLIB PATH="$PREFIX/bin:/usr/bin:/bin" HOME="$SMOKE_ROOT/home" \
    arandu check
  env -u ARANDU_STDLIB PATH="$PREFIX/bin:/usr/bin:/bin" HOME="$SMOKE_ROOT/home" \
    arandu run
)

echo
echo "SMOKE OK — install layout, symlink canonicalize, package CLI"
