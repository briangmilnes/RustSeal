---
round: r0005
from: orchestrator
to: orchestrator
subject: faithful-real-bench-port
date: 2026-0704-09:30
status: active
---

# Faithfully duplicate Rust's real alloc benchmarks in Divan; classify bogus / needs-black_box; get our time

## Goal

Reproduce **every** real Rust standard-library alloc benchmark for `Vec`,
`VecDeque`, and `BinaryHeap` in Divan, at their **real element types**, running
against the rustseal extracted collections. For each bench determine whether it is
**bogus** (measures nothing / the optimizer deletes it) or **needs a `black_box`**
to stop that. Measure our times. Then update the docs to match.

Fixing docs is not the goal — a correct, complete, faithful benchmark port that we
can run is the goal. Docs follow.

## Source of truth

`~/projects/rust/library/alloctests/benches/` (Rust **1.97.0**):
- `vec.rs` — 101 `#[bench]`
- `vec_deque.rs` — 15 `#[bench]`
- `binary_heap.rs` — 6 `#[bench]`

(Our extraction is rust-libs 1.96.0; note any bench that uses a 1.97-only API.)

## Porting rules (libtest `#[bench]` → Divan)

1. `#[bench] fn bench_X(b: &mut Bencher)` → `#[divan::bench] fn X<V>(bencher: divan::Bencher)`,
   generic over the variant type so the same body runs against both our baseline and
   fixed variant. Real element type preserved (e.g. `into_sorted_vec` is `i32`,
   `find_smallest` is `u32`).
2. `b.iter(|| EXPR)` → `bencher.bench_local(|| EXPR)` (both black-box the closure's
   return). Per-iteration setup that must not be timed → `bencher.with_inputs(..)
   .bench_values(..)`.
3. `test::black_box` → `divan::black_box`, kept exactly where the original had it.
4. The **type under test** (`Vec`/`VecDeque`/`BinaryHeap`) becomes the rustseal
   extracted type via a `BenchVec`/`BenchDeque`/`BenchHeap` trait impl'd for both
   variants; helper/input `Vec`s stay `std::vec::Vec`.
5. Faithful body — do NOT restructure the workload (keep `push;clear`, `extend;pop`,
   the exact sizes, the exact iterators). This is the fidelity the current
   `compare.rs` family port lost.

## Per-bench audit (the deliverable classification)

For each ported bench, record: real type · what it times · **verdict** ∈
{ real · needs-black_box (and where) · bogus (measures nothing / DCE) }. A bench is
bogus only if, after a correct `black_box`, it still measures ~nothing (e.g. a
trivial constructor whose result is unused, or a dead loop the optimizer removes).

## Steps

1. **binary_heap (6)** — faithful port `benches/binary_heap/real_binary_heap.rs`;
   add `[[bench]]`; build; run through `scripts/bench.sh`; capture times; classify
   all 6. → verify: compiles 0 warnings, run emits 6×2 rows, each classified.
2. **vec_deque (15)** — same. → verify: 15×2 rows, classified.
3. **vec (101)** — same, `benches/vec/real_vec.rs`. Port in file-section order; may
   fan out the mechanical porting to subagents, but every ported bench is
   compiled + run here. → verify: 101×2 rows, classified.
4. Retire the old family `compare.rs` benches only after the faithful ports cover
   them (or keep both; note which is canonical).
5. **Update `docs/Existing{Vec,VecDeque,BinaryHeap}Benchmarks.md`** — replace the
   family list with the complete faithful bench list (real type + verdict + our
   time, both variants).
6. **Update `docs/Leakage.md`** cross-collection results with the faithful numbers.
7. **Update `README.md`** benchmark-results section.
8. Build + test green; commit + push.

## Success criteria (the goal is hit when)

- All 122 real benches are faithfully ported and compile (0 warnings).
- `scripts/bench.sh` runs them and we have our times for each (both variants).
- Every bench has a verdict (real / needs-black_box / bogus) backed by the code.
- The three Existing docs, Leakage.md, and README reflect the complete faithful set.
