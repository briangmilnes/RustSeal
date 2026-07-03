---
round: r0003
from: orchestrator
to: user
subject: import-vercoll-as-rustseal-crate
date: 2026-0703-15:01
status: done
related: reports/r0003-report-from-orchestrator-to-user-import-vercoll-as-rustseal-crate.md
---

# Import `vercoll` into RustSeal as the root crate `rustseal`

Bring the `vercoll` crate from CSTs (`~/projects/CSTs/products/vercoll/`) into
RustSeal and make it RustSeal's default/root Rust crate, renamed `rustseal`. This
is a **copy** — vercoll stays in CSTs unchanged.

## Decisions (from the user)

1. **Layout** — RustSeal root IS the crate: `Cargo.toml` + `src/` + `benches/` +
   `tests/` at `~/projects/RustSeal/` top level. Merge the crate's `docs/` and
   wrapper `scripts/` alongside RustSeal's existing ones; keep RustSeal's own
   `README.md`, `CLAUDE.md`, `data/`, `plans/`, `analyses/`, `logs/` intact.
2. **Name** — `rustseal`. No `vercoll` naming anywhere in the copied source.
3. **Copy, not move** — vercoll remains in CSTs.

## Steps

1. Copy `src/`, `tests/`, `benches/`, `docs/`, `Cargo.toml`, `Cargo.lock`,
   `.cargo/config.toml`, and the crate wrappers (`build.sh` / `test.sh` /
   `bench.sh` / `validate.sh` / `compare-safe-unsafe` / `compare_safe_unsafe.py`)
   into RustSeal. Do NOT copy vercoll's `logs/` or `analyses/` (CSTs-round
   artifacts — they stay in CSTs). Fold vercoll's `README.md` design note into
   `docs/collection-extraction-design-note.md` (do not overwrite RustSeal's README).
2. Case-preserving token rename `vercoll`→`rustseal` across every copied text file
   (`use vercoll::` → `use rustseal::`, script log prefixes, `Cargo.toml` name +
   description). Zero remaining hits.
3. Guard the workspace: add `exclude = ["RustProjects", "RustProjects/*"]` to the
   `[workspace]` table so cargo never scans the gitignored 3.3 GB corpus.
4. Merge `.gitignore` (`/target/`).

## Done when

- `git grep -i vercoll` over the copied crate trees returns zero hits.
- `scripts/build.sh` — zero errors, zero warnings.
- `scripts/test.sh` — all tests pass, none fail.
- The changes are committed in RustSeal (not pushed) with a matching r0003 report.
