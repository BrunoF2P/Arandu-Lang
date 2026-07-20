#!/usr/bin/env bash
# Regression gate: diagnostic-related test output must not depend on harness
# parallelism (RFC A1 finalize/determinism).
#
# Runs the same package tests with --test-threads=1 and --test-threads=N and
# requires identical exit status + normalized stdout/stderr.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PKG="${1:-arandu_typeck}"
THREADS_N="${2:-8}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

normalize() {
  # Drop cargo progress / timing noise that differs across runs (cache, order).
  # Keep pass/fail summary only — both runs must succeed with same suite outcome.
  grep -E '^(test result:|error\[|error:)' \
    | sed -E \
      -e 's/[0-9]+\.[0-9]+s//g' \
      -e 's/test result: ok\. [0-9]+ passed.*/test result: ok./' \
      -e 's/test result: FAILED\. .*/test result: FAILED./' \
    | sort
}

echo "==> determinism: cargo test -p ${PKG} --lib -- --test-threads=1"
set +e
cargo test -p "$PKG" --lib -- --test-threads=1 --quiet >"$TMP/t1.raw" 2>&1
EC1=$?
set -e
normalize <"$TMP/t1.raw" >"$TMP/t1.norm"

echo "==> determinism: cargo test -p ${PKG} --lib -- --test-threads=${THREADS_N}"
set +e
cargo test -p "$PKG" --lib -- --test-threads="$THREADS_N" --quiet >"$TMP/tN.raw" 2>&1
ECN=$?
set -e
normalize <"$TMP/tN.raw" >"$TMP/tN.norm"

if [[ "$EC1" -ne 0 || "$ECN" -ne 0 ]]; then
  echo "error: test package failed (threads=1 exit=$EC1, threads=$THREADS_N exit=$ECN)" >&2
  tail -40 "$TMP/t1.raw" >&2 || true
  tail -40 "$TMP/tN.raw" >&2 || true
  exit 1
fi

if ! diff -u "$TMP/t1.norm" "$TMP/tN.norm" >"$TMP/diff.txt"; then
  echo "error: diagnostic/test output diverged under different --test-threads" >&2
  cat "$TMP/diff.txt" >&2
  exit 1
fi

echo "check-diag-determinism: ok (${PKG}, threads 1 vs ${THREADS_N})"
