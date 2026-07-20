#!/usr/bin/env bash
# Convenience wrapper: DiagCode ↔ docs/errors bijection (logic lives in xtask).
#
#   ./scripts/check-diag-docs.sh
#   cargo run -p xtask -- check-diag-docs
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
exec cargo run -q -p xtask -- check-diag-docs "$@"
