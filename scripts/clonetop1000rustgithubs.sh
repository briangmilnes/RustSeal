#!/bin/sh
# Clone the top Rust GitHub projects' sources + configs only, kept small.
#   - shallow: --depth 1, single branch, no tags (no branches, no history)
#   - save the top-level 1-commit git log to GITLOG.txt, then drop .git
#   - prune: compiled libraries, benches, tests, and tar/gzip archives
# Usage: clonetop1000rustgithubs.sh [N]   (N = how many from the top; default: all)
set -eu

ROOT="$HOME/projects/RustSeal"
LIST="$ROOT/data/top1000rustgithubs.txt"
DEST="$ROOT/RustProjects"
N="${1:-0}"   # 0 = all

mkdir -p "$DEST"

# Keep only the source tree: *.rs files, Cargo.toml, and GITLOG.txt.
# Everything else (libs, benches, tests, archives, media, docs, configs) goes.
prune() {
    dir="$1"
    find "$dir" -type f ! -name '*.rs' ! -name 'Cargo.toml' ! -name 'GITLOG.txt' -delete
    find "$dir" -depth -type d -empty -delete
}

count=0; cloned=0; skipped=0; failed=0
while IFS= read -r url; do
    [ -z "$url" ] && continue
    count=$((count + 1))
    [ "$N" -gt 0 ] && [ "$count" -gt "$N" ] && break

    owner=$(basename "$(dirname "$url")")
    repo=$(basename "$url")
    target="$DEST/${owner}__${repo}"

    if [ -d "$target" ]; then
        skipped=$((skipped + 1)); continue
    fi

    echo "[$count] $url"
    if git clone --depth 1 --single-branch --no-tags --quiet "$url" "$target"; then
        git -C "$target" log > "$target/GITLOG.txt"   # top-level log, 1 commit
        rm -rf "$target/.git"
        prune "$target"
        cloned=$((cloned + 1))
    else
        echo "  FAILED: $url" >&2
        rm -rf "$target"
        failed=$((failed + 1))
    fi
done < "$LIST"

echo "Done. cloned=$cloned skipped=$skipped failed=$failed"
