#!/bin/bash
# scripts/test.sh — `cargo test` of rustseal: the ported binary_heap
# tests (V-renamed from rust-libs alloctests). RUSTC_BOOTSTRAP=1 unlocks the unstable
# library features the crate + tests rely on (allocator_api, trusted_len,
# exact_size_is_empty, extend_one) on the stable toolchain — it forces nothing. Detached
# crate. ANSI-stripped log committed under ../logs.
set -euo pipefail
PROD="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mkdir -p "$PROD/logs"
log="$PROD/logs/test-rustseal-$(date +%Y%m%d-%H%M%S).log"
RUSTC_BOOTSTRAP=1 cargo test --manifest-path "$PROD/Cargo.toml" "$@" 2>&1 \
  | sed 's/\x1b\[[0-9;]*[mGKHABCDEFJST]//g' | tee "$log"
exit "${PIPESTATUS[0]}"
