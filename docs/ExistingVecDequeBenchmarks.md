<style>
body, main, .markdown-body { max-width: 95vw !important; width: 95vw !important; }
table { width: 100% !important; border-collapse: collapse; }
td, th { padding: 3px 7px; vertical-align: top; }
</style>

# Existing VecDeque Benchmarks (faithful port)

Faithful 1:1 Divan port of the real Rust suite `library/alloctests/benches/vec_deque.rs`
(Rust 1.97.0) — `benches/vec_deque/real_vec_deque.rs`, run 2026-07-04. All 15 real `#[bench]`,
at the **real element types**: i32 (new/grow/iter/mut_iter/try_fold), usize (into_iter family,
from_array), u8 (extend_bytes/extend_vec), u16 (the three trustedlen/chained extends).
`std::VVecDeque` vs `lazy_loss_recovery::VVecDeque`; `ratio = lazy / std`, `fastest` of 100 samples.

Aggregates over the 14 real benches: **median 1.02 · average 1.02 · time-weighted 1.04.**

The real suite has **no `drain` bench** (the earlier family port had invented `drain_sum_50k`).
The 3 `into_iter*` benches whose upstream form used an `unsafe` `Vec::from_raw_parts` buffer-reuse
helper (only to keep allocation out of libtest timing) are ported with Divan's native
`with_inputs().bench_values()` untimed setup — identical measured op, no `unsafe`.

| # | bench | elem | times (method) | lazy/std | verdict |
|--:|-------|------|----------------|---------:|---------|
| 1 | `new` | i32 | `VecDeque::new()` (empty) | ~10 (sub-ns) | **BOGUS** — empty construct, below the ~19 ns timer floor |
| 2 | `grow_1025` | i32 | 1025 `push_front` incl. growth | 1.12 | real |
| 3 | `iter_1000` | i32 | `iter()` sum | 1.00 | real |
| 4 | `mut_iter_1000` | i32 | `iter_mut()` sum | 1.00 | real |
| 5 | `try_fold` | i32 | `iter().try_fold` sum | 1.10 | real |
| 6 | `into_iter` | usize | `into_iter` sum, plain + rotate_left | 1.02 | real |
| 7 | `into_iter_fold` | usize | `into_iter().fold` (rebuilds each iter) | 1.01 | real |
| 8 | `into_iter_try_fold` | usize | `into_iter().any(==1023)` | 1.00 | real |
| 9 | `into_iter_next_chunk` | usize | `next_chunk::<64>()` drain | 1.02 | real |
| 10 | `from_array_1000` | usize | `From<[usize;1000]>` | 1.08 | real |
| 11 | `extend_bytes` | u8 | `extend(&[u8;512])` into cleared ring | 0.91 | real |
| 12 | `extend_vec` | u8 | `extend(vec.clone())` (clone timed) | 0.92 | real |
| 13 | `extend_trustedlen` | u16 | `extend(0..512)` | 1.03 | real |
| 14 | `extend_chained_trustedlen` | u16 | `extend((0..256).chain(768..1024))` | 1.06 | real |
| 15 | `extend_chained_bytes` | u16 | `extend(a.iter().chain(b.iter()))` | 1.04 | real |

## Notes

- **`new` is bogus** — an empty deque allocates nothing; Std runs at ~0.07 ns (below the timer
  floor), so its 10× ratio is pure sub-nanosecond noise. It is the only non-measurement.
- Everything else is parity or a small residual (`grow_1025` 1.12×, `try_fold` 1.10×,
  `from_array_1000` 1.08×) — the fixed cost of the lazy variant's `pending` field on small ops.
