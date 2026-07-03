# VecDeque Leakage — forget/leak amplification in `VVecDeque`, and the `lazy_loss_recovery` fix

*Interim numbers: Divan `vec_deque_compare`, **fastest of 100 samples**, both variants interleaved in
one process. Run 2026-07-03 on a **non-quiescent** box — to be rewritten after a quiescent benchmark.
See the drain-variance note. Product: `vec_deque` = `VVecDeque`, the growable ring-buffer deque
extracted from `alloc::collections::vec_deque`.*

## 1. What "VecDeque leakage" is

The same three-op leak-amplification family as `Vec` (see `VecLeakage.md`): each returns an iterator
that **parks the deque's `len` short** at construction and relies **solely on the iterator's `Drop`**
to restore it. A `mem::forget` skips the `Drop`, so elements are lost — leaking *more* than the
iterator owns. Deliberate, RFC-1066-licensed `std` behavior.

| op | parks | `forget` leaks (std) |
|----|-------|----------------------|
| `drain(range)` | `len = range.start` | un-yielded range **+ the entire tail** |
| `extract_if(range, pred)` | `len = 0` | the **whole deque** |
| `splice(range, repl)` | via its held `Drain` | range + tail + the un-inserted replacement (`splice` is a nightly-only `VecDeque` method, `feature(deque_extend_front)`) |

## 2. The fix — the `lazy_loss_recovery` variant

Identical mechanism to `Vec`: a `pending` note records the finish work, the iterator mirrors
progress, `Drop` clears it, and a forgotten iterator is finished by the next op via
`restore_wf_wo_data_loss` (guarded entry points + `VVecDeque::Drop`); `len()` is pending-aware;
`splice` inherits the tracked drain (deque elements safe, only `replace_with` leaks). The note is
**boxed** (`Option<Box<Pending>>`, 8 B pointer, not 40 B inline) so it does not tax cheap ops —
boxing dropped `new` from **2.5× to ~0.8×** and the `extend`s from 1.26–1.30× to parity.

**Proven** (counted-drop forget tests): `drain`/`extract_if`/`splice` lose **zero** deque elements on
`forget`. Both variants: 109 tests pass.

## 3. Cost — all 15 upstream families + the forget-safety ops

The 15 upstream `alloctests/benches/vec_deque.rs` benches (u8/u16 element types represented by `u32`)
plus `drain`/`pop_front`. `ratio = lazy ÷ std`.

### Aggregate statistics (all 16 comparisons)

| statistic | value | what it means |
|-----------|------:|---------------|
| **median ratio** | **1.019** | the typical bench: parity (the headline — robust to the drain outlier) |
| unweighted mean ratio | 1.109 | per-bench average, pulled up by the `drain` variance below |
| time-weighted ratio (Σlazy ÷ Σstd) | 1.368 \* | **do not read** — dominated by `drain_sum_50k` (2.40 this run) on a loaded box |

**Drain variance caveat:** the two 50k ops (`drain`, `pop`) dominate the wall-clock, and `drain` is
badly non-reproducible on this non-quiescent machine — so the aggregate wall-clock ratio is a
measurement artifact this session, not signal. `pop_front_50k` is stable at **1.01**, and there is
no reproducible macro-scale regression; the reliable read is the **median at parity**.

### Full per-workload table

| # | workload | std | lazy_loss_recovery | ratio | |
|--:|----------|----:|-------------------:|------:|--|
| 1 | `drain_sum_50k` | 18.16 µs | 43.59 µs | 2.40 | high variance |
| 2 | `extend_bytes` | 28.55 ns | 28.93 ns | 1.01 | — |
| 3 | `extend_chained_bytes` | 28.32 ns | 31.37 ns | 1.11 | — |
| 4 | `extend_chained_trustedlen` | 66.50 ns | 68.41 ns | 1.03 | — |
| 5 | `extend_trustedlen` | 35.30 ns | 35.33 ns | 1.00 | — |
| 6 | `extend_vec` | 44.77 ns | 45.42 ns | 1.01 | — |
| 7 | `from_array_1000` | 150.10 ns | 149.40 ns | 1.00 | — |
| 8 | `grow_1025` | 925.20 ns | 983.70 ns | 1.06 | — |
| 9 | `into_iter_fold_1024` | 109.30 ns | 127.00 ns | 1.16 | slower |
| 10 | `into_iter_next_chunk_1024` | 106.10 ns | 109.10 ns | 1.03 | — |
| 11 | `into_iter_try_fold_1024` | 276.20 ns | 276.30 ns | 1.00 | — |
| 12 | `iter_1000` | 106.00 ns | 108.50 ns | 1.02 | — |
| 13 | `mut_iter_1000` | 106.30 ns | 107.80 ns | 1.01 | — |
| 14 | `new` | 0.59 ns | 0.49 ns | 0.83 | faster |
| 15 | `pop_front_50k` | 50.38 µs | 50.84 µs | 1.01 | — |
| 16 | `try_fold_1000` | 105.80 ns | 110.60 ns | 1.05 | — |

## 4. Reading the table

- **Boxing removed the small-op tax.** `new` 0.83 (was 2.51 inline), `extend_bytes` 1.01,
  `from_array_1000` 1.00, iteration (`iter`/`mut_iter`/`try_fold`) 1.01–1.05. The typical bench is now
  parity (median 1.019).
- **Residual small costs:** `into_iter_fold` 1.16 (the note rides inside `IntoIter`),
  `extend_chained_bytes` 1.11 (one noisy extend shape this run), `grow_1025` 1.06 (the push_front
  guard/reconcile branch, not struct size). All small absolute.
- **`drain_sum_50k` is unreliable this session** (2.40 here, has swung 0.97× ↔ 2.4× across runs) — a
  µs-scale op on a loaded box. Needs a quiescent machine to pin; `pop_front_50k` (1.01, stable) shows
  the big-op path is not regressed.

## 5. Verdict

With the note boxed, `VVecDeque` matches `Vec`: forget-safety is **free on the hot and typical paths**
(median parity), and boxing erased the small-construct tax that the 40-B inline note had caused. The
only unresolved number is `drain` wall-clock, which this non-quiescent box refuses to measure
stably — a re-run when the machine is idle would settle it. Nothing here is a reproducible macro
regression; the `Vec`/`VecDeque` forget-safety cost, once the note is a pointer, is essentially the
fixed field-init on the very smallest ops — which is still what the speed-demons reject, and why `std`
keeps the zero-field leak-amplification design (RFC 1066).

## Reproduce

```
cd ~/projects/RustSeal
scripts/bench.sh --bench vec_deque_compare              # this table (best on a quiescent box)
scripts/test.sh  --test vec_deque_lazy_loss_recovery    # 109 tests incl. forget/no-loss proofs
```
