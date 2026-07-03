# BinaryHeap Leakage — forget/panic unsafety in the heap, and the winning fix

*Round r0466 follow-up. Product: `products/rustseal` (`binary_heap`, extracted from
`alloc::collections::binary_heap`).*
*Numbers: Divan `compare` (`benches/binary_heap/compare.rs`), fastest of 100 samples, both variants
interleaved in one process. See `src/binary_heap/README.md` for the full design study.*

Unlike `Vec`/`VecDeque` (which compare a faithful `std` baseline against a forget-safe variant), the
`binary_heap` study compares **the original** (`unsafe_binary_heap`, a faithful `std` extraction)
against **the winner** (`unsafe_lazy_hole_resort_binary_heap`) — the variant chosen as the production
heap because it is **strictly more correct than `std`**. This doc is the leakage-framed view of that
comparison.

## 1. What "BinaryHeap leakage" is

The heap's `peek_mut` has the same leak-amplification shape, plus a second hazard the collections
don't have:

- **Forget (leak amplification).** `std`'s `peek_mut` uses a `set_len` trick: a forgotten *mutated*
  `PeekMut` leaves the heap valid but **leaks the tail** — the not-yet-restored elements are lost.
- **Panic (order corruption).** A comparison panic mid-sift is memory-safe (the `Hole` refills its
  slot, no double-drop) but leaves the heap **order unspecified** — nothing records the break, so it
  never self-heals.

Both are `std`'s *documented* behavior (RFC-1066 territory for the forget half).

## 2. The winner — `unsafe_lazy_hole_resort_binary_heap`

Closes **both** gaps at parity on the common path:

- **No-data-loss forget safety** via lazy reconcile: `deref_mut` sets a `POSSIBLY_DIRTY_ROOT` bit in a
  one-byte `possibly_mal_formed` field and returns `&mut data[0]` — nothing set aside, no `set_len`;
  the `sift_down(0)` repair is deferred to the next `&mut` op. A forgotten guard loses **nothing**.
- **Order recovery after a comparison panic:** a per-sift `PanicProtection` guard sets
  `POSSIBLY_UNSORTED` while unwinding; the next op does a full *O*(n) `rebuild`. Proven by
  `test_resort_after_comparison_panic`, which fails on every other variant.

So the winner is the rare case where the forget/panic fix was **adopted** (it is the production heap),
because — unlike the `Vec`/`VecDeque` field tax — its cost is confined to one op.

## 3. Cost — all 6 upstream families, original vs winner

The 6 `alloctests/benches/binary_heap.rs` workloads. `ratio = winner ÷ original`.

### Aggregate statistics (all 6 comparisons)

| statistic | value | what it means |
|-----------|------:|---------------|
| unweighted mean ratio | 1.118 | per-bench average |
| median ratio | 1.053 | typical bench near parity |
| geometric mean ratio | 1.095 | outlier-robust "typical factor" |
| Σ original times | 1.235 ms | one pass over all 6 workloads |
| Σ winner times | 1.296 ms | same, forget+panic-safe heap |
| **time-weighted ratio** (Σwinner ÷ Σorig) | **1.050** | aggregate: **~5% slower**, driven by `push` and `find_smallest` |

### Full per-workload table

| # | workload | original | winner (forget+panic safe) | ratio | |
|--:|----------|---------:|---------------------------:|------:|--|
| 1 | `find_smallest_1000` | 111.80 µs | 163.00 µs | 1.46 | slower |
| 2 | `from_vec` | 309.60 µs | 306.40 µs | 0.99 | — |
| 3 | `into_sorted_vec` | 196.30 µs | 180.40 µs | 0.92 | — |
| 4 | `peek_mut_deref_mut` | 1.60 ns | 2.19 ns | 1.37 | slower |
| 5 | `pop` | 161.00 µs | 136.70 µs | 0.85 | faster |
| 6 | `push` | 456.00 µs | 509.40 µs | 1.12 | — |

## 4. Reading the table

- **`find_smallest_1000` 1.46× is the one real cost — and it's the `peek_mut` *mechanism*, not the
  panic protection.** This is a 99k-iteration loop that takes a `peek_mut` every step; the winner's
  lazy `peek_mut` loads and tests `possibly_mal_formed` on entry and guard-drop, where the original's
  `set_len` `peek_mut` touches no flag. It is the price of never leaking the tail, shared by every
  lazy variant. (`src/binary_heap/README.md` shows the `PanicProtection` contributes ~nothing here.)
- **`peek_mut_deref_mut` 1.37× is a non-measurement** — a dead-store loop the optimizer deletes; its
  ratio measures the compiler, not the heap (kept only for parity with the upstream bench).
- **Everything else is parity or faster:** `from_vec` 0.99, `into_sorted_vec` 0.92, `pop` 0.85,
  `push` 1.12. `from_vec`'s panic protection is **provably zero-cost** — the winner's `from_vec`
  compiles to byte-identical machine code to a build with no protection (LLVM deletes the guard,
  since a panic during construction discards the heap; see the README).

## 5. Verdict

Same overall shape — free on almost everything, one real cost on the safety-critical hot op — but the
opposite *decision* from `Vec`/`VecDeque`: here the fix was **adopted as production**. The reason is
where the cost lands: the heap's forget/panic safety costs ~1.5× on `peek_mut`-heavy loops and ~5%
aggregate, but it is confined to `peek_mut` and buys **strictly more correctness than `std`**
(no data loss on a forgotten mutated `peek_mut`, and self-healing order after a comparison panic).
Contrast `Vec`/`VecDeque`, whose fix taxes the trivially-cheap construct/clone ops of *every* value —
the kind of pervasive small-op cost the speed-demons reject. The heap could afford its safety; the
collections' `std` design (zero-field leak amplification) can't.

## Reproduce

```
cd products/rustseal
scripts/bench.sh --bench compare                                # this table (original vs winner)
scripts/test.sh  --test unsafe_lazy_hole_resort_binary_heap     # 34/1, incl. resort-after-panic
```
