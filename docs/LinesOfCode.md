# Lines of Code — `csts-analyze-loc`

The project's lines-of-code measure is **parse-based**: every source file is fully parsed by `vcst`
into a CST and `code` / `comment` / `blank` are read off the CST trivia — **never a regex or
`wc -l`** (rule 1.5). No `tokei` / `cloc` / `scc` is used or installed.

- **Binary:** `csts-analyze-loc` — `datastructs/vst-lib/src/bin/csts-analyze-loc.rs`
  (declared in `datastructs/vst-lib/Cargo.toml`).
- **Wrapper (how you run it):** `scripts/products/generate-analysis-loc`.
- **Output:** `analyses/csts-analyze-loc.<YYYY-MMDD-HH:MM>.orchestrator.md` (committed, per rule 8.4).
- **Lineage:** modeled on veracity's `count_loc.rs` (same metric and table shape), re-implemented in
  CSTs idioms (`vcst_lib::parse_file`, the `vst_lib::pipeline` crate discovery, `unit_crate_name`).

## The metric

For each source file the tool parses the full CST and walks every token; a token's kind classifies
the lines it covers. A line is then exactly one of:

- **code** — at least one non-trivia token touches it (code wins over a trailing comment, so
  `let x = 1; // note` is **code**, the universal convention);
- **comment** — only comment/doc-comment tokens touch it (`LINE_COMMENT`, `BLOCK_COMMENT`, `DOC_*`);
- **blank** — no token touches it (whitespace-only / empty).

`total = code + comment + blank` by construction (every line lands in exactly one bucket). The entire
metric is the single function `classify_file`; swapping it changes nothing else (discovery,
aggregation, the table are independent).

## Scope

This measures the **vendored RM corpus** under `products/RMs/`, discovered by scanning for
`<name>-<version>` dirs (not hardcoded). It is **RM-source LOC**: every `.rs` file under each crate's
source tree — the whole pristine vendored tree, not just the cfg-reachable module subset. Counting the
directory keeps the measure **parse-only** (LOC needs the CST, not the `vast` AST, so no `mod foo;`
module-tree lowering is done).

Per rule 8.6 the two large corpora split below the RM; every other RM is one aggregated row:

| corpus | split into |
|--------|------------|
| `rust-libs` | `core` / `alloc` / `std` / `proc_macro` |
| `asterinas` | `kernel` / `ostd` / `osdk` (each aggregating its member crates) |
| every small RM | one aggregated row |

Not in scope: **product-stage LOC** (the emitted MVRM / VWRM / bound trees) is a separate
measurement, and this tool is aimed at the corpus we are driving to verus — it is not a general
"count this project's own lines" utility (though it does crate-spec discovery, so it can be pointed at
packages).

## Running it

Always through the wrapper, never raw `cargo run` (rule 5.1):

```
scripts/products/generate-analysis-loc                    # RM corpus -> analyses/csts-analyze-loc.<ts>.orchestrator.md
scripts/products/generate-analysis-loc -c <codebase-root> # any other code base (its src/,tests/,benches/)
```

The CLI follows `docs/SwitchStyles.md`: `-c/--codebase <root>` (any codebase),
`-d/--directory <dir>`, `-f/--file <file>` (mutually exclusive), `-o/--output <path>`
(default stdout), `-v/--verbose`, `--help`. With none of `-c`/`-d`/`-f` it measures
the vendored RM corpus (the default table below).

The wrapper:
1. builds `vst-lib` release (`cargo build --release -p vst-lib`);
2. runs `csts-analyze-loc -o analyses/csts-analyze-loc.<ts>.orchestrator.md "$@"` (with no
   `-o/--output` the bin writes the markdown to stdout — the orchestrator redirects into
   `analyses/`, since agents do not write `analyses/`, rule 8.4);
3. logs the whole run (ANSI-stripped) to `logs/generate-analysis-loc.<ts>.<role>.log` (rules 0.6, 4.1,
   4.4); role is detected from the worktree path.

Only the orchestrator runs analyses (rule 8.4). Extra args (e.g. `-c <root>`) pass through verbatim.

## Output table

`analyses/csts-analyze-loc.<ts>.orchestrator.md` — a per-package table following the standard-table
rules (rule 8.7): `#` row id first (15.1), a `TOTALS` row, subtotals under grouped rows, `—` =
not-applicable vs `unknown` = unmeasured. Columns:

| column | meaning |
|--------|---------|
| **#** | continuous row id |
| **package** | rust-libs subcrate, asterinas top-level dir, or a small RM |
| **total** | `code + comment + blank` |
| **code** | lines a non-trivia token touches |
| **comment** | lines only comment/doc tokens touch |
| **blank** | lines no token touches |
| **files** | distinct source files parsed |

Each run is committed as a corpse (rule 4.2), so the LOC of the corpus is tracked over time for
before/after comparison.
