# BinaryHeap Leakage — forget/panic unsafety in the heap, and the winning fix

*Interim numbers: Divan `compare` (`benches/binary_heap/compare.rs`), **fastest of 100 samples**,
both variants interleaved in one process. Run 2026-07-03 on a **non-quiescent** box — to be rewritten
after a quiescent benchmark. See `src/binary_heap/README.md` for the full design study.*

Unlike `Vec`/`VecDeque` (which compare a faithful `std` baseline against a forget-safe variant), the
`binary_heap` study compares **the original** (`unsafe_binary_heap`, a faithful `std` extraction)
against **the winner** (`lazy_hole_resort_binary_heap`) — the variant chosen as the production heap
because it is **strictly more correct than `std`**. This doc is the leakage-framed view of that
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

## 2. The winner — `lazy_hole_resort_binary_heap`

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
| median ratio | 1.103 | typical bench near parity |
| Σ original times | 1.112 ms | one pass over all 6 workloads |
| Σ winner times | 1.217 ms | same, forget+panic-safe heap |
| **time-weighted ratio** (Σwinner ÷ Σorig) | **1.094** | aggregate: **~9% slower**, driven by `find_smallest` |

### Full per-workload table

| # | workload | original | winner (forget+panic safe) | ratio | |
|--:|----------|---------:|---------------------------:|------:|--|
| 1 | `find_smallest_1000` | 108.90 µs | 195.20 µs | 1.79 | slower |
| 2 | `from_vec` | 320.10 µs | 268.20 µs | 0.84 | faster |
| 3 | `into_sorted_vec` | 103.50 µs | 112.80 µs | 1.09 | — |
| 4 | `peek_mut_deref_mut` | 1.21 ns | 1.62 ns | 1.34 | slower |
| 5 | `pop` | 158.50 µs | 170.80 µs | 1.08 | — |
| 6 | `push` | 421.40 µs | 470.30 µs | 1.12 | — |

## 4. Reading the table

- **`find_smallest_1000` 1.79× is the one real cost — and it's the `peek_mut` *mechanism*, not the
  panic protection.** This is a 99k-iteration loop that takes a `peek_mut` every step; the winner's
  lazy `peek_mut` loads and tests `possibly_mal_formed` on entry and guard-drop, where the original's
  `set_len` `peek_mut` touches no flag. It is the price of never leaking the tail, shared by every
  lazy variant. (`src/binary_heap/README.md` shows the `PanicProtection` contributes ~nothing here.)
- **`peek_mut_deref_mut` 1.34× is a non-measurement** — a dead-store loop the optimizer deletes; its
  ratio measures the compiler, not the heap (kept only for parity with the upstream bench).
- **Everything else is parity or faster:** `from_vec` 0.84, `into_sorted_vec` 1.09, `pop` 1.08,
  `push` 1.12. `from_vec`'s panic protection is **provably zero-cost** — the winner's `from_vec`
  compiles to byte-identical machine code to a build with no protection (LLVM deletes the guard,
  since a panic during construction discards the heap; see the README).

## Likely meaningless benches

**1 of the 6 is bogus: `peek_mut_deref_mut`.** It writes 1,000,000 values through a single
`peek_mut` guard and then `mem::forget`s it. Because the writes go through a *forgotten* guard they
are dead code, so the optimizer deletes the whole loop — the faithful port measures ~1.1 ns for it,
i.e. one comparison, not a million writes. The number measures the compiler, not the heap. Upstream's
`black_box(&vec)` fences the input slice, not the writes through the guard, so it does not save it;
to be a real measurement the heap would have to be `black_box`ed *after* the writes (not forgotten).
It is excluded from the median/average/time-weighted aggregates.

## 5. Verdict

Same overall shape — free on almost everything, one real cost on the safety-critical hot op — but the
opposite *decision* from `Vec`/`VecDeque`: here the fix was **adopted as production**. The reason is
where the cost lands: the heap's forget/panic safety costs ~1.8× on `peek_mut`-heavy loops and ~9%
aggregate, but it is confined to `peek_mut` and buys **strictly more correctness than `std`**
(no data loss on a forgotten mutated `peek_mut`, and self-healing order after a comparison panic).
Contrast `Vec`/`VecDeque`, whose fix taxes the trivially-cheap construct/clone ops of *every* value —
the kind of pervasive small-op cost the speed-demons reject. The heap could afford its safety; the
collections' `std` design (zero-field leak amplification) can't.

## Reproduce

```
cd ~/projects/RustSeal
scripts/bench.sh --bench compare                       # this table (original vs winner)
scripts/test.sh  --test lazy_hole_resort_binary_heap   # 34/1, incl. resort-after-panic
```
