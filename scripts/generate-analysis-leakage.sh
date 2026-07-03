#!/bin/bash
set -euo pipefail
# RustSeal-owned wrapper for CSTs' csts-analyze-leakage (GRASE rule 5.1: always
# wrap the tool, never invoke it raw). Produces the parse-based leak/forget census
# (five leaf categories forget / forget_unsized / ManuallyDrop / into_raw* / leak,
# a per-collected-repo table, and a corpus-wide qualified-name breakdown) over the
# collected RustProjects/ corpus. The categories are read off the vcst CST
# structure (never a regex / wc -l, no symbol table, no type resolution).
#
# The binary lives in ~/projects/CSTs and is built from there (RustSeal ships no
# Rust crate for the analysis tools — CLAUDE.md: "analysis tools are built using
# the ~/projects/CSTs code"). This wrapper builds it in CSTs, then runs it with
# RustSeal as the cwd so every artifact lands in RustSeal's logs/ and analyses/.
#
# Logging conforms to standards/LoggingStandard.md: the binary self-stamps its own
# timing log (no CSTS_ANALYSIS_TS bridge); this wrapper tees its build/exec output
# to logs/, and the analytical markdown lands in analyses/.

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

CSTS_ROOT="${CSTS_ROOT:-$HOME/projects/CSTs}"
BIN="$CSTS_ROOT/datastructs/target/release/csts-analyze-leakage"

# Agent slot from the worktree path (rule 0.6 / 1.3).
case "$ROOT" in
    *-agent[0-9]*) ROLE="agent$(echo "$ROOT" | sed -E 's/.*-agent([0-9]+).*/\1/')" ;;
    *)             ROLE="orchestrator" ;;
esac

STAMP="$(date +%Y%m%d-%H%M%S)"
LOG="logs/generate-analysis-leakage-${STAMP}.${ROLE}.log"
OUT="analyses/csts-analyze-leakage-${STAMP}.${ROLE}.md"

mkdir -p logs analyses

# Default target: the whole collected corpus, one row per immediate subdirectory
# (each collected repo). Any explicit args (-d DIR / -f FILE) override and pass
# through verbatim.
if [ "$#" -eq 0 ]; then
    set -- -d RustProjects
fi

{
    echo "# generate-analysis-leakage ${STAMP} (${ROLE})"
    echo "## build vst-lib (in ${CSTS_ROOT})"
    cargo build --release --manifest-path "$CSTS_ROOT/datastructs/Cargo.toml" -p vst-lib 2>&1
    echo "## run csts-analyze-leakage -o ${OUT} $*"
    "$BIN" -o "$OUT" "$@" 2>&1
    echo "## wrote ${OUT}"
} 2>&1 | sed 's/\x1b\[[0-9;]*[mGKHABCDEFJST]//g' | tee "$LOG"
