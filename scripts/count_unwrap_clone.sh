#!/usr/bin/env bash
# Inventory unwrap/expect and .clone() density in library sources (excludes tests/).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "=== .unwrap() / .expect( by crate (src, excl tests) ==="
for c in crates/*/src; do
  name=$(basename "$(dirname "$c")")
  n=$(rg -c --type rust '\.unwrap\(\)|\.expect\(' "$c" 2>/dev/null | awk -F: '{s+=$2} END{print s+0}')
  printf '%5s  %s\n' "$n" "$name"
done | sort -rn

echo
echo "=== .clone() by crate (src) ==="
for c in crates/*/src; do
  name=$(basename "$(dirname "$c")")
  n=$(rg -c --type rust '\.clone\(\)' "$c" 2>/dev/null | awk -F: '{s+=$2} END{print s+0}')
  printf '%5s  %s\n' "$n" "$name"
done | sort -rn

echo
echo "=== write!().unwrap residual (should be 0) ==="
rg -n 'write!.*\.unwrap|writeln!.*\.unwrap' crates --type rust || echo "(none)"
