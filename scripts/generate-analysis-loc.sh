#!/bin/bash
set -euo pipefail
# RustSeal-owned wrapper for CSTs' csts-analyze-loc (GRASE rule 5.1/5.3: always
# wrap the tool, never invoke it raw). Produces the parse-based per-package
# lines-of-code table (total/code/comment/blank/files) over the collected
# RustProjects/ corpus; code/comment/blank are read off the vcst CST trivia,
# never a regex / wc -l.
#
# The binary itself lives in ~/projects/CSTs and is built from there (RustSeal
# ships no Rust crate of its own — CLAUDE.md: "analysis tools are built using the
# ~/projects/CSTs code"). This wrapper builds it in CSTs, then runs it with
# RustSeal as the cwd so every artifact lands in RustSeal's logs/ and analyses/.
#
# Logging conforms to standards/LoggingStandard.md (the authoritative form):
#   - ONE second-resolution stamp `%Y%m%d-%H%M%S` for the whole run, so the three
#     artifacts of a single run (this exec log, the analyses .md, and the binary's
#     own timing log) all carry the SAME stamp and correlate.
#   - `-` before the stamp, `.<role>` role slot, then the extension.
#   - Execution telemetry -> logs/ ; analytical output (the markdown table) ->
#     analyses/ (two destinations by data type).
#
# KNOWN CSTs-side deviations (NOT fixable from this wrapper — see
# plans/r0002-...-fix-csts-analyze-loc-logging.md): the binary (a) hard-codes a `.`
# separator in its timing-log filename, and (b) only writes that log when
# CSTS_ANALYSIS_TS is set — logging the program should own, not the wrapper. The
# CSTS_ANALYSIS_TS line below is an INTERIM BRIDGE only: it makes today's env-gated
# binary write a timing log that carries this run's stamp. Once the binary self-
# stamps and always logs (plan B2), delete that line — the wrapper must not control
# the program's logging.

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

CSTS_ROOT="${CSTS_ROOT:-$HOME/projects/CSTs}"
BIN="$CSTS_ROOT/datastructs/target/release/csts-analyze-loc"

# Agent slot from the worktree path (rule 0.6 / 1.3).
case "$ROOT" in
    *-agent[0-9]*) ROLE="agent$(echo "$ROOT" | sed -E 's/.*-agent([0-9]+).*/\1/')" ;;
    *)             ROLE="orchestrator" ;;
esac

STAMP="$(date +%Y%m%d-%H%M%S)"
LOG="logs/generate-analysis-loc-${STAMP}.${ROLE}.log"
OUT="analyses/csts-analyze-loc-${STAMP}.${ROLE}.md"

mkdir -p logs analyses

# Default target: the whole collected corpus as one aggregate directory scan
# (target/ is skipped by the tool). Any explicit args (-c ROOT / -d DIR / -f FILE)
# override this and pass through verbatim.
if [ "$#" -eq 0 ]; then
    set -- -d RustProjects
fi

{
    echo "# generate-analysis-loc ${STAMP} (${ROLE})"
    echo "## build vst-lib (in ${CSTS_ROOT})"
    cargo build --release --manifest-path "$CSTS_ROOT/datastructs/Cargo.toml" -p vst-lib 2>&1
    echo "## run csts-analyze-loc -o ${OUT} $*"
    CSTS_ANALYSIS_TS="$STAMP" "$BIN" -o "$OUT" "$@" 2>&1
    echo "## wrote ${OUT}"
} 2>&1 | sed 's/\x1b\[[0-9;]*[mGKHABCDEFJST]//g' | tee "$LOG"
