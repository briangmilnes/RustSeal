<style>
body, main, .markdown-body { max-width: 95vw !important; width: 95vw !important; }
table { width: 100% !important; border-collapse: collapse; }
td, th { padding: 3px 7px; vertical-align: top; }
</style>

# Existing BinaryHeap Benchmarks

Original: 6 benches in the Rust standard library's own `library/alloctests/benches/binary_heap.rs`
— the entire std suite for `BinaryHeap` (std benches workloads, not methods, so 6 is all there is).
Used here: all 6, in `benches/binary_heap/compare.rs`, comparing the **original heap**
(`unsafe_binary_heap`) against the **winner** (`lazy_hole_resort_binary_heap`);
`ratio = winner / original`.

Scope — read before the numbers:

- **One element type only: `u32`.** All 6 original workloads run at `u32`.
- Two heaps per run: the original heap vs the winner.

The type has **32 public methods** (see coverage table); the 6 benches time only
**5** of them.

## The 6 benches

| # | bench | measures | times (method) | verdict |
|--:|-------|----------|----------------|---------|
| 1 | `find_smallest_1000` | keep the 1000 smallest of a 100k stream | `peek_mut` (+ `FromIterator`) | keep |
| 2 | `from_vec` | `From<Vec>` of 100k (wrap + O(n) rebuild) | `From` / `from_raw_vec` | keep |
| 3 | `into_sorted_vec` | `into_sorted_vec` of a 10k heap | `into_sorted_vec` | keep |
| 4 | `peek_mut_deref_mut` | 1M-value dead-store loop through a `peek_mut` guard | `peek_mut` | **BOGUS** |
| 5 | `pop` | pop a 10k heap empty | `pop` | keep |
| 6 | `push` | push a shuffled 50k stream | `push` | keep |

## Method coverage (32 public methods, 5 timed)

`benched?` = a bench's timed region exercises this method. `setup` = used only in a
bench's untimed input construction.

| # | method | benched? | by |
|--:|--------|----------|----|
| 1 | `push` | ✓ | `push` |
| 2 | `pop` | ✓ | `pop` |
| 3 | `peek_mut` | ✓ | `find_smallest_1000`, `peek_mut_deref_mut` |
| 4 | `into_sorted_vec` | ✓ | `into_sorted_vec` |
| 5 | `from_raw_vec` | ✓ | `from_vec` (via `From`) |
| 6 | `with_capacity` | setup | `pop`, `push` (untimed) |
| 7 | `new` | — | |
| 8 | `new_in` | — | |
| 9 | `with_capacity_in` | — | |
| 10 | `peek` | — | |
| 11 | `pop_if` | — | |
| 12 | `append` | — | |
| 13 | `drain` | — | forget-safety op, **unbenched** |
| 14 | `drain_sorted` | — | forget-safety op, **unbenched** |
| 15 | `retain` | — | |
| 16 | `refresh` | — | the lazy-reconcile op, **unbenched** |
| 17 | `into_iter_sorted` | — | |
| 18 | `into_vec` | — | |
| 19 | `iter` | — | |
| 20 | `as_slice` | — | |
| 21 | `as_mut_slice` | — | |
| 22 | `len` | — | |
| 23 | `is_empty` | — | |
| 24 | `capacity` | — | |
| 25 | `clear` | — | |
| 26 | `reserve` | — | |
| 27 | `reserve_exact` | — | |
| 28 | `try_reserve` | — | |
| 29 | `try_reserve_exact` | — | |
| 30 | `shrink_to` | — | |
| 31 | `shrink_to_fit` | — | |
| 32 | `allocator` | — | |

## Notes

- Coverage is **5 of 32** timed. That is not our omission — the std suite
  only benches these six workloads; we mirror it exactly.
- The gap that matters for this project: `drain`, `drain_sorted`, and `refresh` (the
  lazy-reconcile op) are **not** benched, though the forget-safety cost is captured
  via `peek_mut` in `find_smallest_1000`. If we want the heap's drain/reconcile cost
  measured, those are new benches to add.
- `peek_mut_deref_mut` (#4) is bogus by design — a dead-store loop the optimizer
  removes (labeled NON-MEASUREMENT in the source), kept only to mirror the original.
