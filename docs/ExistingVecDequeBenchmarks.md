<style>
body, main, .markdown-body { max-width: 95vw !important; width: 95vw !important; }
table { width: 100% !important; border-collapse: collapse; }
td, th { padding: 3px 7px; vertical-align: top; }
</style>

# Existing VecDeque Benchmarks

Original: 15 benches in the Rust standard library's own `library/alloctests/benches/vec_deque.rs`.
Used here: 16 comparison benches in `benches/vec_deque/compare.rs`.

Scope — read before the numbers:

- **One element type only: `u32`.** The originals' u8/u16 element types are represented
  by `u32`, not run at their own types.
- We cover all 15 original workload families plus the `drain` forget-safety op (16 total).
- Two variants per run: the `std` baseline vs `lazy_loss_recovery`.

The type has **65 public methods**; the 16 benches time **9** distinct ones.

`verdict`: `keep` = real work; `near-floor` = real but sub-ns, noise-dominated on one
run; `unstable` explained below.

## The 16 benches

| # | bench | measures | times (method) | verdict |
|--:|-------|----------|----------------|---------|
| 1 | `new` | `VecDeque::new()` (empty, no alloc) | `new` | near-floor |
| 2 | `grow_1025` | `push_front` 1025 times | `push_front` | keep |
| 3 | `from_array_1000` | build from a 1000-element array | `From<[T;N]>` | keep |
| 4 | `iter_1000` | sum via `iter()` | `iter` | keep |
| 5 | `mut_iter_1000` | sum via `iter_mut()` | `iter_mut` | keep |
| 6 | `try_fold_1000` | sum via `iter().try_fold()` | `iter` | keep |
| 7 | `into_iter_fold_1024` | sum via `into_iter().fold()` | `into_iter` | keep |
| 8 | `into_iter_try_fold_1024` | `into_iter().any()`/last | `into_iter` | keep |
| 9 | `into_iter_next_chunk_1024` | `into_iter().next_chunk()` | `into_iter` | keep |
| 10 | `extend_bytes` | extend 512 from a slice iterator | `extend` | keep |
| 11 | `extend_vec` | extend 512 from a `Vec` | `extend` | keep |
| 12 | `extend_trustedlen` | extend 512 from a range | `extend` | keep |
| 13 | `extend_chained_trustedlen` | extend from chained ranges | `extend` | keep |
| 14 | `extend_chained_bytes` | extend from chained slice iterators | `extend` | keep |
| 15 | `pop_front_50k` | `pop_front` 50k empty | `pop_front` | keep |
| 16 | `drain_sum_50k` | sum via `drain(..)` over 50k | `drain` | **unstable** |

## Method coverage (65 public methods, 9 timed)

Timed methods (by which bench): `new` (#1), `push_front` (#2), `From<[T;N]>` (#3),
`iter` (#4,6), `iter_mut` (#5), `into_iter` (#7–9), `extend` (#10–14),
`pop_front` (#15), `drain` (#16).

Not benched — the other ~56 methods, notably: `push_back`, `pop_back`, `insert`,
`remove`, `swap`, `swap_remove_front/back`, `rotate_left/right`, `get`/`get_mut`,
`front`/`back`, `binary_search`, `make_contiguous`, `truncate`, `split_off`,
`retain`, `extract_if`, `resize`, the `reserve`/`shrink` family, and the whole
`_back` half of the API. `extract_if` and `split_off` are forget-safety-relevant and
are **unbenched**.

## Notes

- **`new` (#1) is not bogus** — it measures the fixed cost of constructing an empty
  deque, i.e. the `pending`-field init the whole study is about. It caught the
  inline-note regression (2.5× → 0.76× after boxing). It is sub-nanosecond, so on a
  single loaded-box run its ratio (0.83×) is just noise; read it as "no construction
  tax," not as a real speedup.
- **`drain_sum_50k` (#16)** is real but the least reproducible number on this hardware
  (0.97× ↔ 2.40× across runs); treat its ratio as unmeasured until the quiescent
  re-run.
- Coverage is **9 of 65** — the benches are `push_front`/`iter`/`extend`/`drain`-heavy
  and never touch the `_back` half or `extract_if`/`split_off`. Those are new benches
  to add if we want them measured.
