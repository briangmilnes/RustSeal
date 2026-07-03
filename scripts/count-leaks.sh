#!/bin/bash
set -euo pipefail
# scripts/count-leaks.sh — gross ripgrep census of forget/leak calls across the
# RustProjects/ corpus (~1000 cloned crates, source-only, git-ignored). Counts two
# call families and writes a markdown report to analyses/LeakLines.md.
#
# Deliberate choices:
#   * `--no-ignore`  — RustProjects/ is in RustSeal's .gitignore, so rg would skip
#     it otherwise. This searches everything under it regardless of ignore files.
#   * `-g '*.rs' -g '!target'` — Rust source only; any dir named `target` excluded.
#   * `into_raw` / `into_raw_parts` are DELIBERATELY SKIPPED this pass (they are
#     ownership-transfer, not a leak per se). Add them to PATTERNS to include them.
#
# This is a GROSS count: matches in comments/strings and user-defined `forget(`/
# `leak(` methods are included. It is a first-pass magnitude estimate, not precise.

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
CORPUS="RustProjects"
OUT="analyses/LeakLines.md"
mkdir -p analyses

command -v rg >/dev/null 2>&1 || { echo "count-leaks: ripgrep (rg) not found on PATH" >&2; exit 2; }
[ -d "$CORPUS" ] || { echo "count-leaks: $CORPUS not present (run scripts/clonetop1000rustgithubs.sh)" >&2; exit 2; }

# Counted call families: label|regex. into_raw is intentionally absent.
FORGET_RE='\bforget\s*\('
LEAK_RE='\bleak\s*\('

RG=(rg --no-ignore --no-heading -g '*.rs' -g '!target')

# Per-family totals: match count (--count-matches, summed) and distinct file count.
count_matches() { "${RG[@]}" --count-matches -e "$1" "$CORPUS" | awk -F: '{s+=$NF} END{print s+0}'; }
count_files()   { "${RG[@]}" -l -e "$1" "$CORPUS" | wc -l | tr -d ' '; }

echo "count-leaks: scanning $CORPUS/ (.rs only, target/ excluded, into_raw skipped) ..." >&2
FORGET_M=$(count_matches "$FORGET_RE"); FORGET_F=$(count_files "$FORGET_RE")
LEAK_M=$(count_matches "$LEAK_RE");     LEAK_F=$(count_files "$LEAK_RE")
TOTAL_M=$((FORGET_M + LEAK_M))

# Per-project breakdown. rg emits `RustProjects/<proj>/...:<n>`; bucket by <proj>,
# summing forget and leak match counts, then keep the top 40 by combined total.
per_project() {
  { "${RG[@]}" --count-matches -e "$FORGET_RE" "$CORPUS" | sed 's/$/\tforget/'
    "${RG[@]}" --count-matches -e "$LEAK_RE"   "$CORPUS" | sed 's/$/\tleak/'; } \
  | awk -F'\t' '
      { line=$1; fam=$2;
        n=split(line, byc, ":"); cnt=byc[n];         # count after last colon
        path=line; sub(/:[0-9]+$/, "", path);
        split(path, seg, "/"); proj=seg[2];          # RustProjects/<proj>/...
        if (fam=="forget") f[proj]+=cnt; else l[proj]+=cnt;
        tot[proj]+=cnt; seen[proj]=1 }
      END { for (p in seen) printf "%d\t%d\t%d\t%s\n", tot[p], f[p], l[p], p }' \
  | sort -rn
}

TS="$(date '+%Y-%m-%d %H:%M %Z')"
NPROJ=$(find "$CORPUS" -maxdepth 1 -mindepth 1 -type d | wc -l | tr -d ' ')

{
  echo "# LeakLines — forget/leak call census over the RustProjects corpus"
  echo
  echo "*Generated ${TS}. Gross \`rg\` census of \`forget(\` and \`leak(\` calls across every"
  echo "\`.rs\` file in \`${CORPUS}/\` (${NPROJ} cloned crates, source-only). \`target/\` excluded;"
  echo "\`into_raw\`/\`into_raw_parts\` deliberately skipped this pass. Counts include comments,"
  echo "strings, and user-defined \`forget\`/\`leak\` methods — a magnitude estimate, not precise.*"
  echo
  echo "## Call families"
  echo
  echo "| # | family | ripgrep pattern | matches | files |"
  echo "|--:|--------|-----------------|--------:|------:|"
  echo "| 1 | forget | \`${FORGET_RE}\` | ${FORGET_M} | ${FORGET_F} |"
  echo "| 2 | leak | \`${LEAK_RE}\` | ${LEAK_M} | ${LEAK_F} |"
  echo "| — | **TOTAL** | | **${TOTAL_M}** | |"
  echo
  echo "Skipped this pass: \`into_raw\`, \`into_raw_parts\`."
  echo
  echo "## Top 40 projects by forget+leak calls"
  echo
  echo "| # | project | forget | leak | total |"
  echo "|--:|---------|-------:|-----:|------:|"
  i=0
  per_project | head -40 | while IFS=$'\t' read -r tot f l proj; do
    i=$((i+1)); echo "| ${i} | \`${proj}\` | ${f} | ${l} | ${tot} |"
  done
  echo
  echo "## Method"
  echo
  echo '```'
  echo "rg --no-ignore -g '*.rs' -g '!target' --count-matches -e '${FORGET_RE}' ${CORPUS}"
  echo "rg --no-ignore -g '*.rs' -g '!target' --count-matches -e '${LEAK_RE}'   ${CORPUS}"
  echo '```'
} > "$OUT"

echo "count-leaks: forget=${FORGET_M} (in ${FORGET_F} files), leak=${LEAK_M} (in ${LEAK_F} files), total=${TOTAL_M}" >&2
echo "count-leaks: wrote ${OUT}" >&2
