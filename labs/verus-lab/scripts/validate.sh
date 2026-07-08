#!/bin/bash
# Validate every verus experiment in src/experiments/ (or files passed as args)
# with the verus verifier. Logs to logs/.
#
# verus emits a compiled binary per run; we run it from a throwaway temp dir so
# those artifacts never land in the source tree. verus binary: $VERUS, else
# `verus` on PATH, else the local release build. Override with
#   VERUS=/path/to/verus scripts/validate.sh
set -uo pipefail
ulimit -c 0   # no core dumps -> no apport crash reports on a verus panic
CRATE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_DIR="$CRATE_ROOT/logs"
mkdir -p "$LOG_DIR" "$CRATE_ROOT/analyses"
STAMP="$(date +%Y%m%d-%H%M%S)"
LOG_FILE="$LOG_DIR/$(basename "$0" .sh)-$STAMP.log"
exec > >(tee -a "$LOG_FILE") 2>&1
echo "=== $(basename "$0") $STAMP ==="

VERUS="${VERUS:-$(command -v verus 2>/dev/null || echo /home/milnes/projects/verus/source/target-verus/release/verus)}"
if [ ! -x "$VERUS" ]; then
    echo "verus not found at '$VERUS' — set VERUS=/path/to/verus" >&2
    exit 127
fi
echo "verus: $VERUS"
"$VERUS" --version | head -2

# Extra verus flags forwarded verbatim, e.g.
#   VERUS_FLAGS="--multiple-errors 10 --expand-errors" scripts/validate.sh <file>
VERUS_FLAGS="${VERUS_FLAGS:-}"
[ -n "$VERUS_FLAGS" ] && echo "verus flags: $VERUS_FLAGS"

if [ "$#" -gt 0 ]; then
    files=("$@")
else
    shopt -s nullglob
    files=("$CRATE_ROOT"/src/experiments/*.rs)
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
rc=0
for f in "${files[@]}"; do
    [ -e "$f" ] || continue
    abs="$(cd "$(dirname "$f")" && pwd)/$(basename "$f")"
    echo "=== verus $(basename "$f") ==="
    ( cd "$WORK" && "$VERUS" $VERUS_FLAGS "$abs" ) || rc=1
done
exit "$rc"
