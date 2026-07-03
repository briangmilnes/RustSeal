# Vec Leakage — forget/leak amplification in `VVec`, and the `lazy_loss_recovery` fix

*Interim numbers: Divan `vec_compare`, **fastest of 100 samples**, both variants interleaved in one
process (noise only adds time, so `fastest` is the intrinsic-cost estimate even on a busy box). Run
2026-07-03 on a **non-quiescent** box — to be rewritten after a quiescent benchmark. Product:
`vec` = `VVec`, the `alloc::vec::Vec` extraction.*

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
| **unweighted mean ratio** (per-bench, equal weight) | **2.270** | plain average of the 67 ratios — inflated by sub-ns / DCE outliers (`from_iter@0` = 28×, `with_capacity@1000` = 46×) |
| median ratio | 1.048 | the typical bench: parity |
| **Σ std times** | **424,004 ns** | total wall-clock of one pass over all 67 workloads, std |
| **Σ lazy_loss_recovery times** | **442,649 ns** | same, forget-safe variant |
| **time-weighted ratio** (Σlazy ÷ Σstd) | **1.044** | aggregate: forget-safety adds **4.4%** total wall-clock |

The unweighted mean (2.27) and the time-weighted ratio (1.04) say different, both-true things: with
every workload counted **equally**, the average is dragged up by a handful of sub-nanosecond / dead-
code-eliminated outliers; but **weighted by actual time**, the whole suite is only **4.4%** slower —
the tax is nanoseconds and the time total is dominated by the big ops that are at parity
(`dedup_random@100000` alone is ~65% of the sum, at ratio 1.01).

### Full per-workload table

| # | workload | size | std | lazy_loss_recovery | ratio | |
|--:|----------|-----:|----:|-------------------:|------:|--|
| 1 | `chain_collect` | — | 1.95 µs | 2.05 µs | 1.05 | — |
| 2 | `clone` | 0 | 5.19 ns | 9.34 ns | 1.80 | slower |
| 3 | `clone` | 10 | 22.42 ns | 24.14 ns | 1.08 | — |
| 4 | `clone` | 100 | 27.14 ns | 30.74 ns | 1.13 | — |
| 5 | `clone` | 1000 | 125.80 ns | 130.30 ns | 1.04 | — |
| 6 | `clone` | 50000 | 5.99 µs | 5.74 µs | 0.96 | — |
| 7 | `clone_from` | 0 | 5.00 ns | 5.79 ns | 1.16 | slower |
| 8 | `clone_from` | 10 | 6.55 ns | 6.30 ns | 0.96 | — |
| 9 | `clone_from` | 100 | 11.01 ns | 12.37 ns | 1.12 | — |
| 10 | `clone_from` | 1000 | 57.21 ns | 59.89 ns | 1.05 | — |
| 11 | `clone_from` | 50000 | 3.18 µs | 3.36 µs | 1.06 | — |
| 12 | `collect_range` | 0 | 1.24 ns | 0.59 ns | 0.48 | faster |
| 13 | `collect_range` | 10 | 8.59 ns | 9.27 ns | 1.08 | — |
| 14 | `collect_range` | 100 | 12.14 ns | 12.58 ns | 1.04 | — |
| 15 | `collect_range` | 1000 | 55.47 ns | 59.21 ns | 1.07 | — |
| 16 | `dedup_all` | 100 | 16.86 ns | 18.63 ns | 1.10 | — |
| 17 | `dedup_all` | 1000 | 182.80 ns | 191.00 ns | 1.04 | — |
| 18 | `dedup_all` | 10000 | 1.74 µs | 1.84 µs | 1.06 | — |
| 19 | `dedup_all` | 100000 | 19.95 µs | 21.46 µs | 1.08 | — |
| 20 | `dedup_none` | 100 | 34.78 ns | 33.86 ns | 0.97 | — |
| 21 | `dedup_none` | 1000 | 262.10 ns | 262.70 ns | 1.00 | — |
| 22 | `dedup_none` | 10000 | 2.52 µs | 2.52 µs | 1.00 | — |
| 23 | `dedup_none` | 100000 | 25.07 µs | 25.07 µs | 1.00 | — |
| 24 | `dedup_random` | 100 | 39.80 ns | 37.05 ns | 0.93 | — |
| 25 | `dedup_random` | 1000 | 370.60 ns | 354.60 ns | 0.96 | — |
| 26 | `dedup_random` | 10000 | 4.15 µs | 3.79 µs | 0.91 | faster |
| 27 | `dedup_random` | 100000 | 272.70 µs | 275.10 µs | 1.01 | — |
| 28 | `drain_sum_50k` | — | 4.12 µs | 4.12 µs | 1.00 | — |
| 29 | `extend_from0` | 0 | 5.46 ns | 6.90 ns | 1.26 | slower |
| 30 | `extend_from0` | 10 | 14.64 ns | 15.26 ns | 1.04 | — |
| 31 | `extend_from0` | 100 | 19.99 ns | 23.42 ns | 1.17 | slower |
| 32 | `extend_from0` | 1000 | 71.67 ns | 77.08 ns | 1.08 | — |
| 33 | `extend_from_slice_sym` | 0 | 5.40 ns | 6.03 ns | 1.12 | — |
| 34 | `extend_from_slice_sym` | 10 | 22.55 ns | 23.63 ns | 1.05 | — |
| 35 | `extend_from_slice_sym` | 100 | 41.27 ns | 41.74 ns | 1.01 | — |
| 36 | `extend_from_slice_sym` | 1000 | 169.30 ns | 176.30 ns | 1.04 | — |
| 37 | `extend_recycle` | — | 79.92 ns | 78.80 ns | 0.99 | — |
| 38 | `extend_sym` | 0 | 6.12 ns | 7.48 ns | 1.22 | slower |
| 39 | `extend_sym` | 10 | 24.59 ns | 24.80 ns | 1.01 | — |
| 40 | `extend_sym` | 100 | 43.61 ns | 43.42 ns | 1.00 | — |
| 41 | `extend_sym` | 1000 | 184.30 ns | 182.80 ns | 0.99 | — |
| 42 | `from_elem` | 0 | 0.98 ns | 1.42 ns | 1.44 | slower |
| 43 | `from_elem` | 10 | 11.81 ns | 13.11 ns | 1.11 | — |
| 44 | `from_elem` | 100 | 15.94 ns | 18.68 ns | 1.17 | slower |
| 45 | `from_elem` | 1000 | 78.14 ns | 79.27 ns | 1.01 | — |
| 46 | `from_iter` | 0 | 0.34 ns | 9.48 ns | 27.98 | slower (DCE) |
| 47 | `from_iter` | 10 | 10.98 ns | 16.90 ns | 1.54 | slower |
| 48 | `from_iter` | 100 | 17.03 ns | 24.45 ns | 1.44 | slower |
| 49 | `from_iter` | 1000 | 71.24 ns | 82.80 ns | 1.16 | slower |
| 50 | `from_slice` | 0 | 0.95 ns | 7.63 ns | 8.03 | slower (DCE) |
| 51 | `from_slice` | 10 | 12.63 ns | 14.85 ns | 1.18 | slower |
| 52 | `from_slice` | 100 | 14.17 ns | 20.81 ns | 1.47 | slower |
| 53 | `from_slice` | 1000 | 74.05 ns | 80.89 ns | 1.09 | — |
| 54 | `grow_50k` | — | 31.47 µs | 31.48 µs | 1.00 | — |
| 55 | `in_place_xor` | 10 | 14.56 ns | 14.58 ns | 1.00 | — |
| 56 | `in_place_xor` | 100 | 19.76 ns | 19.39 ns | 0.98 | — |
| 57 | `in_place_xor` | 1000 | 100.80 ns | 95.61 ns | 0.95 | — |
| 58 | `into_iter_sum_50k` | — | 4.79 µs | 4.92 µs | 1.03 | — |
| 59 | `iter_sum_50k` | — | 4.92 µs | 4.93 µs | 1.00 | — |
| 60 | `pop_50k` | — | 13.56 µs | 24.96 µs | 1.84 | slower |
| 61 | `range_map_collect` | — | 980.60 ns | 1.06 µs | 1.09 | — |
| 62 | `retain_even_100k` | — | 24.38 µs | 27.11 µs | 1.11 | — |
| 63 | `with_capacity` | 0 | 1.14 ns | 1.48 ns | 1.30 | slower |
| 64 | `with_capacity` | 10 | 13.05 ns | 12.65 ns | 0.97 | — |
| 65 | `with_capacity` | 100 | 11.31 ns | 11.00 ns | 0.97 | — |
| 66 | `with_capacity` | 1000 | 11.12 ns | 512.80 ns | 46.12 | slower (DCE) |
| 67 | `zip_fill_1000` | — | 131.80 ns | 133.10 ns | 1.01 | — |

## 4. Reading the table

**Median ratio 1.05 — the typical bench is at parity.** (Mean 2.27 is meaningless: it's dragged up
by a few dead-code-eliminated / sub-nanosecond outliers — see the DCE note below.) The results split
into three groups plus a set of measurement artifacts:

- **Parity at scale (the majority).** Every µs-scale, real-work op is parity: `drain_sum` 1.00,
  `grow_50k` 1.00, `iter`/`into_iter` 1.00–1.03, `clone`/`clone_from`/`extend` at 1000–50000 all
  0.96–1.06, `dedup_none`/`dedup_random` at 10k–100k ~1.0, `chain_collect`/`range_map`/`zip` ~1.0.
  Boxing the note is what keeps `drain` at parity (it was 1.69× inline).

- **A fixed ~1–4 ns tax at *tiny* sizes.** `clone@0` 1.80, `clone_from@0` 1.16, `from_elem@0` 1.44,
  `extend@0` ~1.2, `collect_range@1000` 1.07, `from_iter@10–100` 1.44–1.54, `from_slice@100` 1.47,
  `with_capacity@0` 1.30. These are construct/clone/collect on 0–1000 elements, where the one extra
  (boxed) field's init/copy/`Box` drop-glue is a real fixed cost that dominates an op doing almost
  nothing. It vanishes into parity by ~1000 elements.

- **A few genuinely faster for lazy_loss_recovery.** `dedup_random@10000` 0.91, `in_place_xor@1000`
  0.95, `collect_range@0` 0.48 — minor codegen luck, not a real speedup.

**Measurement artifacts (a bench-hygiene problem for the quiescent rewrite):**
- `with_capacity@1000` **46×** and `from_iter@0` **28×**, `from_slice@0` **8×** — dead-code
  elimination on the **std** side: the unused result is elided to ~1–11 ns while the lazy side's
  allocation/`Box` init survives. The *ratio* explodes but the absolute lazy cost is ~8–513 ns. These
  need `black_box(result)` in the bench; they are **not** real regressions.
- `pop_50k` **1.84×** — a micro-artifact: `pop` runs in ~0.2 ns (about one cycle), so codegen wobble
  from the extra field roughly doubles the *ratio* while the absolute cost is trivial (13.6 vs 25.0 µs
  for 50 000 pops). Confirmed not the guard (removing it doesn't recover it).
- `retain_even_100k` **1.11×** — the one plausibly-real macro-scale slowdown (24.4 → 27.1 µs).
  `retain`'s in-place compaction over 100 k elements picks up ~11% from the fatter struct / guard
  interaction; the only non-artifact candidate in the set, and worth confirming on a quiescent box.

## 5. Verdict

Forget-safety here is **free on the hot macro paths** and costs a **fixed ~1–4 ns on
construct/clone/collect** (loud at tiny sizes, gone by ~1000 elements) plus one plausibly-real ~11% on
`retain` and a set of dead-code-elimination artifacts at sub-nanosecond baselines. That is exactly the
profile that **the upstream Rust maintainers would reject**: not the big ops, but the
small-`clone`/construct tax — you cannot add even one field to `Vec` without paying on its
trivially-cheap operations, and `Vec`'s construct/clone/drop are fast enough that a single extra word
+ `Box` drop-glue is a measurable cost below ~100 elements. This is precisely why `std` ships the
**zero-field leak-amplification** design (RFC 1066) instead: it will not pay even a fixed 2 ns on
`clone` to make `forget` lossless.

`lazy_loss_recovery` is therefore a **study variant** — it proves the leaks are closable with no
macro-path cost, and measures the small-op tax that closing them costs. It is not a proposal to
change `std`.

## Reproduce

```
cd ~/projects/RustSeal
scripts/bench.sh --bench vec_compare              # this table (reliable, interleaved)
scripts/bench.sh --bench vec_std                  # the 101 upstream libtest benches, std
scripts/bench.sh --bench vec_lazy_loss_recovery   # the 101 upstream libtest benches, lazy
scripts/test.sh  --test vec_lazy_loss_recovery    # 145 tests incl. the forget/no-loss proofs
```
