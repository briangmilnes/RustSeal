#!/bin/sh
# Grand-total CST-based lines-of-code over the whole collected corpus.
# Uses CSTs' csts-analyze-loc (parse-based: code/comment/blank off the vcst CST,
# never wc -l / regex). One aggregate row over RustProjects/ (target/ skipped).
set -eu

ROOT="$HOME/projects/RustSeal"
CORPUS="$ROOT/RustProjects"
BIN="$HOME/projects/CSTs/datastructs/target/release/csts-analyze-loc"
OUT="$ROOT/analyses/loc-grand-total.md"

mkdir -p "$ROOT/analyses"
[ -x "$BIN" ] || { echo "build csts-analyze-loc first (cargo build --release -p vst-lib in ~/projects/CSTs)" >&2; exit 1; }

"$BIN" -d "$CORPUS" -o "$OUT"
echo "wrote $OUT"
