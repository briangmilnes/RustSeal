#!/bin/bash
# scripts/build.sh — `cargo build` of rustseal (pure Rust collections
# algorithms extracted from rust-libs, V-renamed; faithful incl. the allocator generic).
# RUSTC_BOOTSTRAP=1 unlocks the unstable library features the source relies on
# (allocator_api, trusted_len, exact_size_is_empty, extend_one) on the stable toolchain —
# it forces nothing. Detached crate. ANSI-stripped log committed under ../logs.
set -euo pipefail
PROD="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mkdir -p "$PROD/logs"
log="$PROD/logs/build-rustseal-$(date +%Y%m%d-%H%M%S).log"
RUSTC_BOOTSTRAP=1 cargo build --manifest-path "$PROD/Cargo.toml" "$@" 2>&1 \
  | sed 's/\x1b\[[0-9;]*[mGKHABCDEFJST]//g' | tee "$log"
exit "${PIPESTATUS[0]}"
