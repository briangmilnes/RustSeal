---
round: r0001
from: orchestrator
to: orchestrator
subject: session-restart
date: 2026-0703-13:22
status: pending
---

# Session restart — Rust leakage research + RustSeal corpus + LOC-counter fix

Written for a fresh agent resuming after the user swaps in a new `CLAUDE.md`.
Captures branch state, in-flight work, open decisions, and resume steps.

## What this session did (arc)

1. **Rust "leakage" research** → drafted into `~/projects/rust-leakage/README.md`
   (UNCOMMITTED). Covers: where std documents leaking (`mem::forget`, the
   `leak` family `Box::leak`/`Vec::leak`/`String::leak`, `ManuallyDrop`, the
   collection leak notes on `Vec::drain`/`VecDeque::drain`/`BinaryHeap::peek_mut`),
   RFC 1066 (safe `mem::forget`), the 2015 "leakpocalypse" (JoinGuard/`thread::scoped`
   unsoundness, issue #24292), and the panic/stack-unwinding docs (Reference,
   Nomicon, `std::panic`, RFC 1513).
2. **Built the RustSeal corpus** — top ~1000 Rust GitHub projects, source-only.
3. **Fixed the CST LOC counter's O(n²) defect** in CSTs (round r0477).

## Current state by project (git)

| project | branch | state |
|---------|--------|-------|
| `~/projects/rust-leakage` (cwd) | main | **nothing committed.** Untracked: `CLAUDE.md` (being replaced), `README.md` (the leak research), `.claude/`, `analyses/`, `logs/`, `plans/` (this file). Emacs cruft to delete: `#README.md#`, `.#README.md`, `README.md~`. |
| `~/projects/RustSeal` | main | **NO commits yet.** Untracked: `projects/` (3.3 GB, 1000 crates, source-only), `scripts/`, `data/`. Committing 3.3 GB / ~297K files is an open decision (see below). |
| `~/projects/CSTs` | main | **r0477 committed, NOT pushed** (5 commits, HEAD `120b890a9`). Unrelated untracked grase/vercoll logs present — leave them. |

## RustSeal corpus — the durable deliverable

- **Location:** `~/projects/RustSeal/projects/<owner>__<repo>/` — 1000 projects
  (+1 stray `mvdnes__spin-rs`, see below). ~3.3 GB.
- **Contents per repo:** ONLY `*.rs` source, `Cargo.toml`, and `GITLOG.txt`
  (1-commit shallow log). No `.git`, no libs/benches/tests/archives/media — pruned
  by the clone script's whitelist. Census: 297,433 `.rs`, 5,820 `Cargo.toml`,
  1,001 `GITLOG.txt`.
- **List:** `data/top1000rustgithubs.txt` — 1000 unique GitHub URLs, ranked by
  all-time downloads (pulled fresh from the crates.io API, scanning 1604 crates to
  accumulate 1000 unique repos; crates collapse to repos — dtolnay's many crates,
  rand_*, serde+serde_derive, etc.).
- **Scripts:**
  - `scripts/clonetop1000rustgithubs.sh [N]` — shallow clone → save 1-commit
    `GITLOG.txt` → strip `.git` → whitelist-prune to `*.rs` + `Cargo.toml`.
    No arg = all 1000; `[N]` = top N.
  - `scripts/count-loc.sh` — grand-total LOC via the CSTs counter over `projects/`
    (writes `analyses/loc-grand-total.md`). **Never produced a number** (see below).
- **Stray dir:** `mvdnes__spin-rs` — the `spin` crate migrated off GitHub to
  Codeberg (`codeberg.org/zesterer/spin`), so it dropped out of the fresh list;
  the on-disk dir is a stale clone of the old GitHub location. User's call:
  **keep it** ("if it's out there we're fine with it").

## The LOC counter (CSTs `csts-analyze-loc`) — fixed in r0477

- **Binary:** `~/projects/CSTs/datastructs/target/release/csts-analyze-loc`
  (rebuild: `cargo build --release --manifest-path datastructs/Cargo.toml -p vst-lib`).
- **Switches (standard, per `docs/SwitchStyles.md`):** `-c/--codebase <ROOT>`
  (scans `src`/`tests`/`benches`), `-d/--directory <DIR>` (recursive, `target/`
  skipped), `-f/--file <FILE>`, `-o/--output <PATH>`, `-v/--verbose`
  (per-file + per-crate timing log, flushed/tailable).
- **The r0477 fix:** the old `classify_file` was O(n²) — a `line_of` closure
  rescanned newlines from byte 0 for every token (~N²/file), which made the
  multi-MB generated files (`windows-rs` 4 MB `mod.rs`, `aws-sdk-rust`) take
  minutes each. Fixed by adding a shared parse-layer primitive
  **`vcst_lib::LineIndex`** (one O(N) pass, O(log N) offset→line via
  `partition_point`) and rewriting `classify_file` to use it. LOC numbers
  byte-identical; RM dashboard now 1.39 s.
- **Post-fix field run** (this session, on the largest project): `aws-sdk-rust`
  2.2 GB / 232,672 files → **69.4 s** wall, **0.30 ms/file mean**, slowest single
  file 34 ms (947 KB; end-to-end incl. parse — the LOC-classification-only budget
  ≤10 ms is met per the r0477 gate). LOC = 31,895,962.

## In-flight / open work (todo)

1. **Re-run the grand-total LOC** over the full corpus with the FIXED binary —
   `~/projects/RustSeal/scripts/count-loc.sh`. The pre-fix run was killed at 62 min
   and produced no number; with the fix the whole corpus should finish in a few
   minutes. This is the missing baseline.
2. **Re-run the `unsafe`/`leak`/`forget` survey without tokio-org crates.** The
   earlier 10-project grep survey still included `tokio-rs__bytes` and
   `tokio-rs__mio` (tokio ecosystem — user flagged them). Re-pick 10 clean,
   non-tokio, hand-written low-level crates. Optionally do CST-exact counts (only
   the `unsafe` keyword and real `::forget`/`::leak` call sites) now that the
   counter/`LineIndex` is fast — grep counts include comments/strings.
3. **(Optional) Append real-corpus field numbers to `r0477` report** —
   `~/projects/CSTs/reports/r0477-report-...md` (the aws 2.2 GB / 69 s / 0.30 ms
   run). Offered, not yet done.
4. **Commit/clean rust-leakage** — delete emacs cruft (`#README.md#`,
   `.#README.md`, `README.md~`), then commit `README.md` (leak research) once the
   new `CLAUDE.md` is in place.

## Open decisions (need user input)

1. **Commit RustSeal or not?** 3.3 GB / ~297K files. Options: (a) `.gitignore
   projects/`, commit only `scripts/` + `data/` (the reproducible recipe);
   (b) commit everything; (c) leave uncommitted. Recommend (a).
2. **Exclude the generated mega-crates from analyses?** `aws-sdk-rust` (2.2 GB,
   31.9M LOC) + `windows-rs` (142 MB) are machine-generated and dwarf/ skew every
   count. Decide whether corpus analyses run with or without them (or report both).
3. **Push CSTs r0477?** Committed to `main`, not pushed.

## Resume steps

1. Confirm the new `CLAUDE.md` is in place; skim it for changed conventions.
2. `git -C ~/projects/rust-leakage status` — delete emacs cruft; decide on
   committing `README.md`.
3. Resolve open decision #1 (RustSeal commit strategy) before touching its git.
4. Re-run the grand-total LOC (todo 1) to get the corpus baseline number.
5. Re-run the cleaned `unsafe`/`leak`/`forget` survey (todo 2).
6. Rebuild the LOC binary only if CSTs was touched; otherwise the release build
   is current.

## Key facts a fresh agent needs

- The corpus is **source-only** (`.rs` + `Cargo.toml` + `GITLOG.txt`); no build
  artifacts exist, so nothing compiles — it's a read/parse corpus.
- The LOC metric is **parse-based** (full `vcst` CST, code/comment/blank off
  trivia), never `wc -l`/regex — use `csts-analyze-loc`, not a hand-rolled counter.
- `csts-analyze-loc` is **single-target** (one path per invocation → one table);
  a per-repo table over the corpus needs a loop wrapper. `count-loc.sh` currently
  does a single grand-total (`-d projects/`).
