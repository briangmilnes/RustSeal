#!/bin/bash
# scripts/validate.sh — formal verification of rustseal via
# `cargo verus verify` (rule 5.6), the LIVE verus checkout binary (the same one
# build-verified-algorithms-alloc / validate-rust-libs-crate use). `verify` runs the
# prover over the opted-in crate without producing a compiled artifact.
#
# rustseal is staged toward verus: the V-renamed extraction has no `verus!{}` specs yet,
# so the crate opts into verus (`[package.metadata.verus] verify = true`) but marks the
# whole extraction `#[verifier::external]` — verus verifies 0 items, 0 errors. The
# `--features verus` flag pulls in the optional vstd dep (needed so verus_builtin is
# linked; without it verify=true is V078) and turns on the cfg-gated vstd import +
# external marking. The stable build/test/bench leave that feature OFF, so they never see
# vstd — rustseal stays dual-mode. Spec work plugs straight in by moving items out of the
# external module into `verus!{}`.
#
# RUSTC_BOOTSTRAP=1 unlocks the crate's perma-unstable feature gates (allocator_api, …)
# on the version-matched stable verus toolchain; it forces nothing. ANSI-stripped log
# committed under ../logs (rule 4.4).
set -uo pipefail
PROD="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERUS_DIR="${CSTS_VERUS_DIR:-/home/milnes/projects/verus/source/target-verus/release}"
mkdir -p "$PROD/logs"
log="$PROD/logs/validate-rustseal-$(date +%Y%m%d-%H%M%S).log"
strip() { sed 's/\x1b\[[0-9;]*[mGKHABCDEFJST]//g'; }

if [ ! -x "$VERUS_DIR/cargo-verus" ]; then
    echo "validate-rustseal: no cargo-verus at $VERUS_DIR — build the verus checkout (or set CSTS_VERUS_DIR)" \
        | tee "$log" >&2
    exit 2
fi
export PATH="$VERUS_DIR:$PATH"
export RUSTC_BOOTSTRAP=1

# vstd (a dependency we do not own and must not edit — it lives in the verus checkout)
# references `cfg(feature = "nonzero_internals")`, a feature it does not declare in this
# packaging, so rustc's `unexpected_cfgs` lint warns 3x while compiling vstd. Declare that
# cfg as EXPECTED with the documented `--check-cfg` flag (stable since Rust 1.80) so the
# build is warning-clean (rule 5.8) without disabling the lint or touching vstd. Append so
# any caller-set RUSTFLAGS survive; the flag has NO spaces (cargo splits RUSTFLAGS on
# whitespace).
checkcfg='--check-cfg=cfg(feature,values("nonzero_internals"))'
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }$checkcfg"

# Force a cache-independent re-run (the touch trick the bound-unit validate.sh uses).
find "$PROD/src" -name '*.rs' -exec touch {} + 2>/dev/null || true
( cd "$PROD" && cargo verus verify --features verus "$@" 2>&1 ) | strip | tee "$log"
exit "${PIPESTATUS[0]}"
