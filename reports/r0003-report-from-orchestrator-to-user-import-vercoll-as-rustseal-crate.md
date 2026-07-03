---
round: r0003
from: orchestrator
to: user
subject: import-vercoll-as-rustseal-crate
date: 2026-0703-15:01
started: 2026-0703-14:58
finished: 2026-0703-15:03
related: plans/r0003-plan-from-orchestrator-to-user-import-vercoll-as-rustseal-crate.md
---

# Report — `vercoll` imported into RustSeal as the root crate `rustseal`

Executed plan r0003. `vercoll` is now RustSeal's root Rust crate `rustseal`. It is
a copy — `~/projects/CSTs/products/vercoll/` is unchanged.

## What was copied (vercoll → RustSeal root)

- Directories: `src/`, `tests/`, `benches/`, `docs/`.
- Files: `Cargo.toml`, `Cargo.lock`, `.cargo/config.toml`.
- Wrappers merged into `scripts/`: `build.sh`, `test.sh`, `bench.sh`,
  `validate.sh`, `compare-safe-unsafe`, `compare_safe_unsafe.py` (alongside
  RustSeal's existing `clonetop1000rustgithubs.sh` / `count-loc.sh` /
  `generate-analysis-loc.sh` — no name collisions).
- vercoll's `README.md` design note → `docs/collection-extraction-design-note.md`.

Deliberately **not** copied: vercoll's `logs/` and `analyses/` (CSTs-round run
artifacts — they remain in CSTs). RustSeal's own `README.md`, `CLAUDE.md`,
`data/`, `plans/`, `analyses/`, `logs/` were left untouched.

## Rename

Case-preserving token replacement `vercoll`→`rustseal` (`Vercoll`→`Rustseal`,
`VERCOLL`→`RUSTSEAL`) across all copied text files — `use vercoll::` →
`use rustseal::`, the wrapper log prefixes (`build-vercoll` → `build-rustseal`,
`bench-vercoll` → `bench-rustseal`), and `Cargo.toml` (`name = "rustseal"`,
description updated). `git grep -i vercoll` over `src/ tests/ benches/ docs/
scripts/ Cargo.toml Cargo.lock .cargo/` returns **zero** hits.

## Workspace guard

`Cargo.toml`'s `[workspace]` table now carries
`exclude = ["RustProjects", "RustProjects/*"]` so cargo never scans the gitignored
3.3 GB `RustProjects/` corpus into the workspace. Its comment was updated to
describe the RustSeal home (was a stale CSTs reference).

## Build / test (concrete)

- `scripts/build.sh`: **compilation — zero errors, zero warnings**.
- `scripts/test.sh`: **575 passed, 0 failed** across 7 test targets (unittests +
  binary_heap ×2 + vec_deque ×2 + vec ×2), plus 0 doc-tests.
  - **5 tests are `#[ignore]`** (1 each in the two binary_heap targets and the two
    vec_deque targets, plus lib). These `#[ignore]` markers are carried over
    verbatim from the rust-libs-derived test suites — pre-existing, not skips
    introduced here.
- `scripts/validate.sh` (verus) was NOT run — heavy, and the crate's baseline is
  0-proof `#[verifier::external]`; out of scope for this import.

## Decisions made during execution (surfaced for the user)

1. **`Cargo.lock` is tracked.** vercoll's `.gitignore` ignored `/Cargo.lock`; the
   plan listed `Cargo.lock` as a file to copy. Since `rustseal` is now a root
   application-style crate (benches/tests), I kept `Cargo.lock` **tracked** for
   reproducible builds rather than re-ignoring it. Say the word if you'd prefer it
   gitignored.
2. **Script header path comments** were updated from `products/rustseal/scripts/…`
   to `scripts/…` to match the new root layout (cosmetic; the script logic is
   path-relative and unaffected).

## Handed back / follow-ups

- vercoll remains in CSTs (`products/vercoll/`) as decided.
- RustSeal's `scripts/count-loc.sh` and `scripts/generate-analysis-loc.sh` still
  reference the CSTs `csts-analyze-loc` tooling (r0002 follow-ups) — untouched here.
