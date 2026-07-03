# `binary_heap` — the original, the winner, and the study variants

Eight re-implementations of `alloc::collections::binary_heap` (rust-libs 1.96.0), built to answer
two questions: **how much does "being safe" cost in the heap algorithm**, and **what is the right
design for a `peek_mut`/panic-robust heap**. Two are kept at the front of this directory; the six
study variants that lost on one axis or another live in [`losers/`](losers/).

The full Rust-semantics reasoning (leak amplification, the leakpocalypse, what leaks in safe Rust)
is in [`../../docs/leak-amplification-and-forget-safety.md`](../../docs/leak-amplification-and-forget-safety.md).
The project-wide rust-libs audit of the pattern is in [`/docs/LeakAmplification.md`](../../../../docs/LeakAmplification.md).

## The original — `unsafe_binary_heap`

A faithful extraction of std: `set_len` leak-amplification `peek_mut`, the move-once `Hole` sift,
`get_unchecked`. It is the reference point. Its two robustness gaps are both std's *documented*
behavior:

- **Forget:** a forgotten mutated `peek_mut` keeps the heap valid but **leaks the tail** — the
  not-yet-restored elements are lost (data loss, in the leak sense).
- **Panic:** a comparison panic mid-sift is memory-safe (the `Hole` refills its slot, no double-drop)
  but leaves the heap **order unspecified** — nothing records the break, so it never self-heals.

## The winner — `unsafe_lazy_hole_resort_binary_heap`

The same move-once `Hole` sift and unchecked indexing, but it closes **both** gaps — and the bench
below shows it does so at parity on the common path:

- **No-data-loss forget safety**, via lazy reconcile: `deref_mut` only sets `POSSIBLY_DIRTY_ROOT` in
  a one-byte `possibly_mal_formed` field and returns `&mut data[0]` (*O*(1), nothing moved, nothing
  set aside); the `sift_down(0)` repair is deferred to the next `&mut` op. A forgotten guard loses
  **nothing** — no `set_len`, no leaked tail, no `A: Clone`.
- **Order recovery after a comparison panic:** a per-sift `PanicProtection` guard sets
  `POSSIBLY_UNSORTED` while unwinding (the `Hole` still refills first, so no data loss); the next op
  does a full *O*(n) `rebuild()` (`repair_possibly_unsorted`). A dirty root alone costs only
  `sift_down(0)` (`repair_possibly_dirty_root`). Proven by `test_resort_after_comparison_panic`,
  which fails on every other variant.
- The well-formedness gate `repair_possibly_mal_formed`, run at every `&mut` entry point, is a
  single test-against-zero (`0` == well-formed); only the cold path discriminates which repair.

So it is **strictly more correct than std**: it loses no data on a forgotten mutated `peek_mut`, and
it self-heals its order after a comparison panic — neither of which std does. It is the production
choice.

## Performance — original vs winner

Divan (`scripts/bench.sh --bench compare`), both variants measured **interleaved in one process**,
**quiescent machine, 2000 samples**; winner ÷ original (lower = winner faster). `median` is Divan's
headline; `fastest` is the intrinsic cost (noise only adds time).

| # | workload | orig median | winner median | ratio (median) | ratio (fastest) |
| --- | --- | --: | --: | --: | --: |
| 1 | `from_vec` (build by rebuild) | 277 µs | 263 µs | 0.95 | 1.06 |
| 2 | `into_sorted_vec` | 194 µs | 209 µs | 1.08 | 1.00 |
| 3 | `push` | 502 µs | 503 µs | 1.00 | 1.08 |
| 4 | `pop` | 228 µs | 232 µs | 1.01 | 1.08 |
| 5 | `find_smallest_1000` (peek_mut-heavy) | 119 µs | 179 µs | **1.50** | **1.60** |
| 6 | `peek_mut_deref_mut` | 2.18 ns | 2.92 ns | 1.34 | 1.65 *(non-measurement)* |

What it says:

- **`find_smallest` (~1.5–1.6×) is the only real cost — and it's the `peek_mut` *mechanism*, not the
  panic protection.** The winner's lazy `peek_mut` loads and tests `possibly_mal_formed` twice per
  iteration (at `peek_mut()` entry and guard `Drop`) across the 99k-iteration loop, where the
  original's `set_len` `peek_mut` touches no flag on its read path. It is the price of never leaking
  the tail (no-data-loss), shared by every lazy variant; the `PanicProtection` contributes nothing
  here (it's in the sift, run on only ~1% of iterations).
- **Everything else is parity.** `from_vec` is *provably* byte-identical machine code (below), yet
  still reads 0.95–1.06 — that ±5–8% is the residual noise floor even on a quiet box (the original's
  `from_vec` alone spans 240 µs fastest / 277 µs median / 816 µs slowest). `into_sorted_vec`, `pop`,
  `push` all sit inside that band with no consistent direction. No measurable cost outside the peek
  loop.
- **`peek_mut_deref_mut` is a non-measurement** — a dead-store loop the optimizer deletes; its ratio
  measures the compiler.

### `from_vec`'s panic protection is free — proven, not assumed

`PanicProtection` lives in the sift, so `rebuild` (hence `from_vec`) constructs it `n/2` times. It is
provably **zero-cost** there: in the optimized build the winner's `from_vec` compiles to
**byte-identical machine code** to a build with no protection at all (an instruction-for-instruction
disassembly diff is empty modulo addresses). LLVM eliminates the guard — *including* the flag-write
on the unwind path — because a panic during construction **discards** the heap, making that write a
dead store. So the bookkeeping the winner adds is genuinely off the `from_vec` cost; no special-cased
construction path is needed.

## The losers (in [`losers/`](losers/))

Six study variants, each dropping a guarantee to isolate a cost axis, or simply beaten by the winner:

| # | variant | what it is, and why it lost |
| --- | --- | --- |
| 1 | `safe_binary_heap` | zero `unsafe` blocks (tail-split `peek_mut`, swap sift). Correct and forget-safe, but ~3× (*O*(n) `peek_mut` + swap-moves-twice) and needs `A: Clone`. |
| 2 | `safe_opt_binary_heap` | `safe` with sift-on-drop `peek_mut` — *O*(log n), but **drops the forget guarantee** (forget → broken heap). |
| 3 | `safe_but_for_index_binary_heap` | `safe_opt` + `get_unchecked` — isolates the bounds-check cost (≈ 8%). |
| 4 | `unsafe_nopanic_binary_heap` | bare-pointer hole, **no panic guard** — isolates swap-vs-`Hole`; ~2× (raw pointer loses the slice aliasing info) and not panic-safe. |
| 5 | `unsafe_lazy_binary_heap` | lazy reconcile on the **swap** sift — forget-safe + no-data-loss, but ~2.3× on sift-heavy ops (swap moves each element twice). |
| 6 | `unsafe_lazy_hole_binary_heap` | lazy reconcile on the `Hole` sift — **the winner minus panic order-recovery** (memory-safe on a panic but leaves the order broken). The winner's direct predecessor. |

What they isolated: bounds checks ≈ 8%; swap-vs-`Hole` ≈ 10% geomean (up to ~2× sift-heavy); the
struct `Hole` optimizes better than a bare-pointer hole (the slice reference gives LLVM aliasing
info the raw pointer loses); and `set_len` leak amplification is **not needed** for an *O*(1)
forget-safe `peek_mut` — lazy reconcile gets it without leaking the tail.

## Tests & benches

The shared suite (rust-libs `alloctests/tests/collections/binary_heap.rs`, V-renamed) is copied per
variant. Mirroring the source layout, **only the original and the winner run**: their copies sit at
`../../tests/binary_heap/<variant>.rs` and are declared as `[[test]]` targets (Cargo doesn't
auto-discover subdirs). The six loser copies live in `../../tests/binary_heap/losers/` and are **not
wired as targets — they don't run**; they are kept as the record (the scores they produced are
below). Run the live ones with `scripts/test.sh`. The scores encode the guarantees:

| score | meaning |
| --- | --- |
| 34 passed / 1 ignored | full guarantees + `test_resort_after_comparison_panic` — **`unsafe_lazy_hole_resort`** (winner, runs) |
| 33 / 1 | full guarantees (`test_peek_mut_leek` passes) — `unsafe` (original, runs); losers `safe`, `unsafe_lazy`, `unsafe_lazy_hole` |
| 32 / 2 | forget guarantee dropped (`test_peek_mut_leek` ignored) — losers `safe_opt`, `safe_but_for_index` |
| 31 / 3 | + panic guarantee dropped (`panic_safe` ignored) — loser `unsafe_nopanic` |

(The always-ignored test is `test_in_place_iterator_specialization`, a corpse for a std-internal
specialization no variant implements.)

Benches: same split. The **Divan `compare` bench** (`benches/binary_heap/compare.rs`) is the
original-vs-winner comparison above — both variants in one process, statistical. The original's and
winner's per-variant libtest `#[bench]` files stay at `../../benches/binary_heap/`; the six loser
bench copies are in `../../benches/binary_heap/losers/`, also not wired as targets. Run
`scripts/bench.sh --bench compare` (best on a quiescent machine).
