#!/bin/bash
# scripts/bench.sh — `cargo bench` of rustseal: the ported binary_heap
# benchmarks (V-renamed from rust-libs alloctests/benches) on the built-in libtest
# Bencher harness. RUSTC_BOOTSTRAP=1 unlocks feature(test) plus the crate's unstable
# library features on the stable toolchain — it forces nothing. Detached crate.
# ANSI-stripped log committed under ../logs.
set -euo pipefail
PROD="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mkdir -p "$PROD/logs"
log="$PROD/logs/bench-rustseal-$(date +%Y%m%d-%H%M%S).log"
RUSTC_BOOTSTRAP=1 cargo bench --manifest-path "$PROD/Cargo.toml" "$@" 2>&1 \
  | sed 's/\x1b\[[0-9;]*[mGKHABCDEFJST]//g' | tee "$log"
exit "${PIPESTATUS[0]}"
