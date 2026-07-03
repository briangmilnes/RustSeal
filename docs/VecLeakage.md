# Vec Leakage — forget/leak amplification in `VVec`, and the `lazy_loss_recovery` fix

*Round r0466. Products: `products/rustseal` (`vec` = `VVec`, the `alloc::vec::Vec` extraction).*
*Numbers: Divan `vec_compare`, fastest of 100 samples, both variants interleaved in one process
(noise only adds time, so `fastest` is the intrinsic-cost estimate even on a busy box).*

## 1. What "Vec leakage" is

Three `Vec` operations return an iterator that **parks the vec's `len` short** at construction and
relies **solely on the iterator's `Drop`** to put it back (restore the tail, drop the un-yielded
elements). A `mem::forget` of that iterator skips the `Drop`, so the parked `len` is never restored
and elements are lost — **leak amplification** (you leak *more* than the iterator logically owns).
This is `std`'s documented, deliberate behavior, licensed by **RFC 1066** ("safe `mem::forget`" —
leaking is safe, and it is the type's problem, not `forget`'s):

| op | parks | `forget` leaks (std) | documented |
|----|-------|----------------------|------------|
| `drain(range)` | `len = range.start` | the un-yielded range **+ the entire tail** | yes (`# Leaking`) |
| `extract_if(range, pred)` | `len = 0` | the **whole vec** | no — doc only covers the drop path |
| `splice(range, repl)` | via its held `Drain` | range + tail + the un-inserted replacement | "unspecified if leaked" |

`into_iter` (owns the vec by value — ordinary forget-leaks-owned, not amplification), `retain`
(in-place, no forgettable iterator), and `VVecSetLenOnDrop` (a shrink-abort guard) are **not** in the
leak-amp family and are left alone.

## 2. The fix — the `lazy_loss_recovery` variant

`src/vec/` is split into two variants behind `vec/vec.rs`: `std` (faithful baseline, leaks) and
`lazy_loss_recovery` (forget-safe). The fix ports the `vec_deque` mechanism:

- **`pending: Option<Box<Pending>>`** on `VVec` — a note recording the finish work
  (`Pending::Drain { tail_start, tail_len, drop_offset, drop_len }` / `Pending::ExtractIf { old_len,
  idx, del }`). Boxed so the always-present field is one 8-byte pointer, not 40 bytes inline.
- The constructor **records** the note; the iterator **mirrors** its progress into it each step; the
  iterator's `Drop` **clears** it. If the iterator is **forgotten**, the note survives and the **next
  vec operation** finishes the removal via `restore_wf_wo_data_loss` (an `is_some` fast-path + a
  `#[cold]` reconstruct-and-drop), run at every mutating entry point and in `VVec::Drop`.
- `len()` is **pending-aware** (reports the post-removal length while parked). `splice` inherits the
  tracked drain, so the vec's own elements survive a forgotten Splice; only the un-inserted
  `replace_with` leaks (unavoidable without an `I: 'static` bound — its borrows can't outlive the
  splice — and the signature is kept identical to `std`).

**Proven** (counted-drop tests, both variants — `std` asserts the leak, `lazy_loss_recovery` asserts
no loss): `drain` (from-0 and mid-range), `extract_if`, and `splice` all lose **zero** vec elements
on `forget` in `lazy_loss_recovery`. Both variants: 145 tests pass.

**Known limitation:** `&self` element reads (`iter`/`get`/`PartialEq`) show the *parked* view until a
`&mut` op reconciles; only `len()` is pending-aware. Not unsound (the parked view is a valid, shorter
vec) — just stale until the next `&mut` op.

## 3. Cost — every upstream bench family, std vs lazy_loss_recovery

The 101 upstream `alloctests/benches/vec.rs` benches collapse into these workload families (the
type-variant `in_place` u8/u32/u128 benches are represented by `u32`). Divan runs each against both
variants across the upstream size range. `ratio = lazy ÷ std` (>1 = forget-safe variant slower).

### Aggregate statistics (all 67 comparisons)

| statistic | value | what it means |
|-----------|------:|---------------|
| **unweighted mean ratio** (per-bench, equal weight) | **1.426** | plain average of the 67 ratios — inflated by sub-ns outliers (`from_iter@0` = 18×) |
| median ratio | 1.022 | the typical bench: parity |
| geometric mean ratio | 1.143 | the outlier-robust "central" multiplicative ratio |
| **Σ std times** | **388,958 ns** | total wall-clock of one pass over all 67 workloads, std |
| **Σ lazy_loss_recovery times** | **409,251 ns** | same, forget-safe variant |
| **time-weighted ratio** (Σlazy ÷ Σstd) | **1.052** | aggregate: forget-safety adds **5.2%** total wall-clock |

The unweighted mean (1.43) and the time-weighted ratio (1.05) say different, both-true things: with
every workload counted **equally**, the average op is ~1.4× (the many tiny-size ops each carry the
fixed field tax); but **weighted by actual time**, the whole suite is only **5.2%** slower — the tax
is nanoseconds and the time total is dominated by the big ops that are at parity (`dedup_random@100000`
alone is ~65% of the sum, at ratio 1.02). The geometric mean (1.14) sits between as the
outlier-robust "typical factor."

### Full per-workload table

| # | workload | size | std | lazy_loss_recovery | ratio | |
|--:|----------|-----:|----:|-------------------:|------:|--|
| 1 | `chain_collect` | — | 1.03 µs | 1.02 µs | 0.99 | — |
| 2 | `clone` | 0 | 4.86 ns | 8.16 ns | 1.68 | slower |
| 3 | `clone` | 10 | 11.53 ns | 14.12 ns | 1.23 | slower |
| 4 | `clone` | 100 | 17.30 ns | 17.88 ns | 1.03 | — |
| 5 | `clone` | 1000 | 65.91 ns | 67.60 ns | 1.03 | — |
| 6 | `clone` | 50000 | 3.15 µs | 3.15 µs | 1.00 | — |
| 7 | `clone_from` | 0 | 4.90 ns | 6.37 ns | 1.30 | slower |
| 8 | `clone_from` | 10 | 5.80 ns | 6.87 ns | 1.19 | slower |
| 9 | `clone_from` | 100 | 10.61 ns | 13.77 ns | 1.30 | slower |
| 10 | `clone_from` | 1000 | 59.10 ns | 60.41 ns | 1.02 | — |
| 11 | `clone_from` | 50000 | 3.21 µs | 3.25 µs | 1.01 | — |
| 12 | `collect_range` | 0 | 0.95 ns | 1.03 ns | 1.08 | — |
| 13 | `collect_range` | 10 | 9.93 ns | 6.95 ns | 0.70 | faster |
| 14 | `collect_range` | 100 | 13.19 ns | 16.83 ns | 1.28 | slower |
| 15 | `collect_range` | 1000 | 62.44 ns | 74.57 ns | 1.19 | slower |
| 16 | `dedup_all` | 100 | 20.71 ns | 18.50 ns | 0.89 | faster |
| 17 | `dedup_all` | 1000 | 204.00 ns | 177.00 ns | 0.87 | faster |
| 18 | `dedup_all` | 10000 | 1.95 µs | 1.74 µs | 0.89 | faster |
| 19 | `dedup_all` | 100000 | 19.63 µs | 18.49 µs | 0.94 | — |
| 20 | `dedup_none` | 100 | 29.76 ns | 38.19 ns | 1.28 | slower |
| 21 | `dedup_none` | 1000 | 237.90 ns | 224.10 ns | 0.94 | — |
| 22 | `dedup_none` | 10000 | 2.29 µs | 2.15 µs | 0.94 | — |
| 23 | `dedup_none` | 100000 | 21.33 µs | 21.33 µs | 1.00 | — |
| 24 | `dedup_random` | 100 | 34.21 ns | 34.02 ns | 0.99 | — |
| 25 | `dedup_random` | 1000 | 313.80 ns | 324.80 ns | 1.03 | — |
| 26 | `dedup_random` | 10000 | 3.67 µs | 3.48 µs | 0.95 | — |
| 27 | `dedup_random` | 100000 | 252.70 µs | 257.50 µs | 1.02 | — |
| 28 | `drain_sum_50k` | — | 3.93 µs | 3.94 µs | 1.00 | — |
| 29 | `extend_from0` | 0 | 5.45 ns | 6.40 ns | 1.18 | slower |
| 30 | `extend_from0` | 10 | 13.92 ns | 14.26 ns | 1.02 | — |
| 31 | `extend_from0` | 100 | 19.22 ns | 18.35 ns | 0.95 | — |
| 32 | `extend_from0` | 1000 | 67.73 ns | 81.66 ns | 1.21 | slower |
| 33 | `extend_from_slice_sym` | 0 | 6.11 ns | 7.51 ns | 1.23 | slower |
| 34 | `extend_from_slice_sym` | 10 | 23.79 ns | 26.40 ns | 1.11 | — |
| 35 | `extend_from_slice_sym` | 100 | 37.88 ns | 37.13 ns | 0.98 | — |
| 36 | `extend_from_slice_sym` | 1000 | 175.10 ns | 166.40 ns | 0.95 | — |
| 37 | `extend_recycle` | — | 89.57 ns | 91.13 ns | 1.02 | — |
| 38 | `extend_sym` | 0 | 6.11 ns | 7.28 ns | 1.19 | slower |
| 39 | `extend_sym` | 10 | 23.15 ns | 24.08 ns | 1.04 | — |
| 40 | `extend_sym` | 100 | 36.16 ns | 38.07 ns | 1.05 | — |
| 41 | `extend_sym` | 1000 | 173.80 ns | 172.50 ns | 0.99 | — |
| 42 | `from_elem` | 0 | 1.12 ns | 1.04 ns | 0.93 | — |
| 43 | `from_elem` | 10 | 12.12 ns | 12.99 ns | 1.07 | — |
| 44 | `from_elem` | 100 | 15.29 ns | 17.19 ns | 1.12 | — |
| 45 | `from_elem` | 1000 | 73.01 ns | 72.88 ns | 1.00 | — |
| 46 | `from_iter` | 0 | 0.50 ns | 9.18 ns | 18.37 | slower |
| 47 | `from_iter` | 10 | 11.26 ns | 16.85 ns | 1.50 | slower |
| 48 | `from_iter` | 100 | 16.76 ns | 19.98 ns | 1.19 | slower |
| 49 | `from_iter` | 1000 | 68.63 ns | 77.85 ns | 1.13 | — |
| 50 | `from_slice` | 0 | 1.06 ns | 7.42 ns | 7.01 | slower |
| 51 | `from_slice` | 10 | 12.15 ns | 14.30 ns | 1.18 | slower |
| 52 | `from_slice` | 100 | 13.37 ns | 17.48 ns | 1.31 | slower |
| 53 | `from_slice` | 1000 | 70.41 ns | 77.79 ns | 1.10 | — |
| 54 | `grow_50k` | — | 30.19 µs | 30.00 µs | 0.99 | — |
| 55 | `in_place_xor` | 10 | 13.58 ns | 14.38 ns | 1.06 | — |
| 56 | `in_place_xor` | 100 | 18.24 ns | 18.31 ns | 1.00 | — |
| 57 | `in_place_xor` | 1000 | 99.07 ns | 90.63 ns | 0.92 | — |
| 58 | `into_iter_sum_50k` | — | 4.54 µs | 4.54 µs | 1.00 | — |
| 59 | `iter_sum_50k` | — | 4.54 µs | 4.54 µs | 1.00 | — |
| 60 | `pop_50k` | — | 12.54 µs | 25.65 µs | 2.04 | slower |
| 61 | `range_map_collect` | — | 962.30 ns | 960.30 ns | 1.00 | — |
| 62 | `retain_even_100k` | — | 20.92 µs | 25.07 µs | 1.20 | slower |
| 63 | `with_capacity` | 0 | 1.23 ns | 0.94 ns | 0.77 | faster |
| 64 | `with_capacity` | 10 | 11.83 ns | 11.72 ns | 0.99 | — |
| 65 | `with_capacity` | 100 | 10.02 ns | 9.75 ns | 0.97 | — |
| 66 | `with_capacity` | 1000 | 10.40 ns | 10.27 ns | 0.99 | — |
| 67 | `zip_fill_1000` | — | 128.60 ns | 129.40 ns | 1.01 | — |

## 4. Reading the table

**Median ratio 1.02 — the typical bench is at parity.** (Mean 1.43 is meaningless: it's dragged up
by sub-nanosecond outliers like `from_iter@0` = 18× on a 0.5 ns → 9 ns no-op.) The results split
cleanly into three groups:

- **Parity at scale (the majority).** Every µs-scale, real-work op is parity: `drain_sum` 1.00,
  `grow` 0.99, `iter`/`into_iter` 1.00, `clone`/`clone_from`/`extend` at 1000–50000 all 0.95–1.03,
  `dedup_random`/`dedup_none` at 10k–100k ~1.0, `chain_collect`/`range_map`/`zip` ~1.0. Boxing the
  note is what bought `drain` back to parity (it was 1.69× inline).

- **A fixed ~1–3 ns tax at *tiny* sizes.** `clone@0` 1.68, `clone_from@0–100` 1.19–1.30, `from_iter@0`
  18×, `from_slice@0–100` up to 7×, `extend@0` ~1.2, `collect_range@100–1000` 1.2–1.3. These are
  construct/clone/collect on 0–1000 elements, where the one extra (boxed) field's init/copy/`Box`
  drop-glue is a real fixed cost that dominates an op doing almost nothing. It vanishes into parity by
  ~1000 elements.

- **A few genuinely faster for lazy_loss_recovery.** `dedup_all` 0.87–0.94 across all sizes,
  `in_place_xor@1000` 0.92, `with_capacity@0` 0.77 — minor codegen luck, not a real speedup.

**Two µs-scale non-parity results worth naming:**
- `pop_50k` **2.04×** — a micro-artifact: `pop` runs in ~0.2 ns (about one cycle), so codegen wobble
  from the extra field doubles the *ratio* while the absolute cost is trivial (12 vs 26 µs for 50 000
  pops). Confirmed not the guard (removing it doesn't recover it).
- `retain_even_100k` **1.20×** — the one real macro-scale slowdown (21 → 25 µs). `retain`'s in-place
  compaction over 100 k elements picks up ~20% from the fatter struct / guard interaction; the only
  non-artifact regression in the set.

## 5. Verdict

Forget-safety here is **free on the hot macro paths** and costs a **fixed ~1–3 ns on
construct/clone/collect** (loud at tiny sizes, gone by ~1000 elements) plus one real ~20% on
`retain` and a sub-nanosecond `pop` blip. That is exactly the profile that **the upstream Rust
maintainers would reject**: not the big ops, but the small-`clone`/construct tax — you cannot add
even one field to `Vec` without paying on its trivially-cheap operations, and `Vec`'s construct/
clone/drop are fast enough that a single extra word + `Box` drop-glue is a measurable 20–68% below
~100 elements. This is precisely why `std` ships the **zero-field leak-amplification** design (RFC
1066) instead: it will not pay even a fixed 2 ns on `clone` to make `forget` lossless.

`lazy_loss_recovery` is therefore a **study variant** — it proves the leaks are closable with no
macro-path cost, and measures the small-op tax that closing them costs. It is not a proposal to
change `std`.

## Reproduce

```
cd products/rustseal
scripts/bench.sh --bench vec_compare              # this table (reliable, interleaved)
scripts/bench.sh --bench vec_std                  # the 101 upstream libtest benches, std
scripts/bench.sh --bench vec_lazy_loss_recovery   # the 101 upstream libtest benches, lazy
scripts/test.sh  --test vec_lazy_loss_recovery    # 145 tests incl. the forget/no-loss proofs
```
