<style>
body, main, .markdown-body { max-width: 95vw !important; width: 95vw !important; }
table { width: 100% !important; border-collapse: collapse; }
td, th { padding: 3px 7px; vertical-align: top; }
</style>

# Existing BinaryHeap Benchmarks (faithful port)

Faithful 1:1 Divan port of the real Rust suite `library/alloctests/benches/binary_heap.rs`
(Rust 1.97.0) — `benches/binary_heap/real_binary_heap.rs`, run 2026-07-04. All 6 real
`#[bench]`, at the real element type (u32; `into_sorted_vec`/`pop` are i32 upstream — a
4-byte int, timing-identical). Original heap (`unsafe_binary_heap`) vs winner
(`lazy_hole_resort_binary_heap`); `ratio = winner / original`, `fastest` of 100 samples.

Aggregates over the 5 real benches: **median 1.01 · average 1.09 · time-weighted 1.04.**

| # | bench | times (method) | winner/original | verdict |
|--:|-------|----------------|----------------:|---------|
| 1 | `find_smallest_1000` | `peek_mut` (keep-1000-smallest of 100k) | 1.42 | real — the one cost: the winner's lazy `peek_mut` flag load/test on a 99k-iteration peek loop |
| 2 | `from_vec` | `From<Vec>` of shuffled 100k (clone timed) | 0.98 | real, parity |
| 3 | `into_sorted_vec` | `clone().into_sorted_vec()` of 10k | 1.01 | real, parity |
| 4 | `pop` | `extend(10k rev)` then pop empty | 1.03 | real, parity |
| 5 | `push` | push shuffled 50k then `clear` | 0.99 | real, parity |
| 6 | `peek_mut_deref_mut` | write 1M values through one forgotten `peek_mut` guard | 1.08 | **BOGUS** |

## Notes

- **`peek_mut_deref_mut` is bogus (proven):** it runs in ~1.1 ns for a loop that writes
  1,000,000 values, so the optimizer deleted the loop — the writes go through a `mem::forget`ed
  `PeekMut`, making them dead code. The original's `black_box(&vec)` does not rescue it; to be
  real it would need the heap `black_box`ed after the writes (not forgotten).
- **Faithful setup-timing changed the numbers vs the old family port.** The family port un-timed
  the `clone` in `from_vec`/`into_sorted_vec` and the `extend`/`clear` in `pop`/`push`, reporting
  `from_vec` 0.84× and `push` 1.12×. Timing them as the real bench does, all four are parity;
  `find_smallest_1000` (1.42×) is the only real cost.
