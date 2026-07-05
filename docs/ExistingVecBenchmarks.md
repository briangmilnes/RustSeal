<style>
body, main, .markdown-body { max-width: 95vw !important; width: 95vw !important; }
table { width: 100% !important; border-collapse: collapse; }
td, th { padding: 3px 7px; vertical-align: top; }
</style>

# Existing Vec Benchmarks

Original: 101 benches in the Rust standard library's own `library/alloctests/benches/vec.rs`.
Used here: 67 comparison rows across 24 bench families in `benches/vec/compare.rs`.

Scope — read before the numbers:

- **One element type only: `u32`.** The originals' type-variant benches (u8/u32/u128,
  e.g. `in_place`) are represented by `u32`, not run at their own types.
- We cover every original *workload family*, but grouped — the 101 originals collapse
  into 24 families, each run across our own size sweep — not the 101 verbatim.
- Two variants per run: the `std` baseline vs `lazy_loss_recovery`.

The type has **65 public methods**; the 24 families time **14** distinct ones.

`sizes` = the `args` the family runs at. `verdict`: `keep` = real work; `size-0` = the
size-0 row is a near-floor construction measurement (sub-ns, noise-dominated, but it
measures the `pending`-field init cost — like VecDeque `new`); `BOGUS`/`noise`/`elided`
explained in the summary.

## The 24 bench families

| # | bench | measures | times (method) | sizes | verdict |
|--:|-------|----------|----------------|-------|---------|
| 1 | `with_capacity` | alloc cap-n vec, ptr escapes | `with_capacity` | 0,10,100,1000 | elided (patched) |
| 2 | `collect_range` | `(0..n).collect()` | `FromIterator` | 0,10,100,1000 | keep / size-0 |
| 3 | `from_elem` | `repeat(5).take(n).collect()` | `FromIterator` | 0,10,100,1000 | keep / size-0 |
| 4 | `from_slice` | `VVec::from(&[0..n])` | `From<&[T]>` | 0,10,100,1000 | keep / size-0 |
| 5 | `from_iter` | `src.clone()` (not FromIterator) | `clone` | 0,10,100,1000 | **BOGUS** |
| 6 | `extend_from0` | extend empty with n | `extend` | 0,10,100,1000 | keep / size-0 |
| 7 | `extend_sym` | extend n-vec with n | `extend` | 0,10,100,1000 | keep / size-0 |
| 8 | `extend_from_slice_sym` | `extend_from_slice` on n-vec | `extend_from_slice` | 0,10,100,1000 | keep / size-0 |
| 9 | `clone` | clone an n-vec | `clone` | 0,10,100,1k,50k | keep / size-0 |
| 10 | `clone_from` | `clone_from` into n-vec | `clone_from` | 0,10,100,1k,50k | keep / size-0 |
| 11 | `in_place_xor` | in-place `enumerate().map().collect()` | `into_iter`+`FromIterator` | 10,100,1000 | keep |
| 12 | `dedup_random` | `dedup` a sorted random vec | `dedup` | 100..100k | keep |
| 13 | `dedup_none` | `dedup`, no adjacent dups | `dedup` | 100..100k | keep |
| 14 | `dedup_all` | `dedup` all-equal | `dedup` | 100..100k | keep |
| 15 | `retain_even_100k` | `retain(even)` over 100k | `retain` | 100k | keep |
| 16 | `grow_50k` | push 50k into reserved vec | `push` | 50k | keep |
| 17 | `pop_50k` | pop 50k empty | `pop` | 50k | **noise** |
| 18 | `iter_sum_50k` | sum via `iter()` | `iter` | 50k | keep |
| 19 | `into_iter_sum_50k` | sum via `into_iter()` | `into_iter` | 50k | keep |
| 20 | `drain_sum_50k` | sum via `drain(..)` | `drain` | 50k | keep |
| 21 | `chain_collect` | `iter.chain([1]).collect()` | `FromIterator` | 16384 | keep |
| 22 | `range_map_collect` | `(0..16384).map().collect()` | `FromIterator` | 16384 | keep |
| 23 | `extend_recycle` | extend empty with 1000 | `extend` | 1000 | keep |
| 24 | `zip_fill_1000` | `zip(subst).collect()` | `FromIterator` | 1000 | keep |

## Method coverage (65 public methods, 14 timed)

Timed methods: `with_capacity`, `FromIterator`, `From<&[T]>`, `extend`,
`extend_from_slice`, `clone`, `clone_from`, `dedup`, `retain`, `push`, `pop`, `iter`,
`into_iter`, `drain`.

Not benched — the other ~51 methods, notably: `insert`, `remove`, `swap_remove`,
`truncate`, `resize`, `split_off`, `splice`, `extract_if`, `get`/`get_mut`,
`as_slice`/`as_mut_slice`, `contains`, the `reserve`/`try_reserve`/`shrink` family,
`dedup_by`/`dedup_by_key`, `rotate_left`/`right`, `fill`. **`splice` and `extract_if`
are forget-safety-relevant and unbenched** — only `drain` (#20) covers the
leak-amplification family here.

## Bogus summary

- **`from_iter` (#5)** — bogus: its body is `src.clone()`, so it times `clone`, not
  `FromIterator` (and duplicates `clone`, #9). The real collect workloads are
  `collect_range` (#2) and `from_elem` (#3). Rename/repurpose or delete.
- **`with_capacity` (#1)** — was optimized away on `std` (allocation elided → 46×);
  now patched with `black_box(v.as_ptr())`, needs a re-run to confirm parity.
- **`pop_50k` (#17)** — a per-op ~0.2 ns operation; its 1.84× ratio is codegen wobble.
- **Size-0 rows** of families #1–#10 are near-floor: they measure the fixed
  `pending`-field cost on an empty vec (the study's subject, same as VecDeque `new`),
  but at sub-ns baselines a single run's ratio is noise. Real per-op cost appears at
  n ≥ ~100.
