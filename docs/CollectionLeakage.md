# Collection Leakage — forget/leak-amplification across `Vec`, `VecDeque`, `BinaryHeap`

*Summary of the three per-collection studies (`VecLeakage.md`, `VecDequeLeakage.md`,
`BinaryHeapLeakage.md`). Product: `products/rustseal`. Numbers below are a **single fresh run of all
three** on a **non-quiescent** machine (fastest of 100 samples, both variants interleaved per Divan
compare) — read the medians; treat the `drain` wall-clock as noise this session.*

## The study

Three `std` collections park a `len`/order state short during a bulk op and rely **solely on an
iterator's `Drop`** to restore it. `mem::forget` skips that `Drop` → **leak amplification** (leaking
more than the iterator owns), and for the heap also **order corruption** after a comparison panic.
This is deliberate, RFC-1066-licensed `std` behavior. Each product carries a faithful `std` baseline
plus a fixed variant that closes the holes with no data loss:

| collection | baseline | fixed variant | fix mechanism |
|------------|----------|---------------|---------------|
| `Vec` | `std` | `lazy_loss_recovery` | boxed `pending` note + lazy `restore_wf_wo_data_loss` |
| `VecDeque` | `std` | `lazy_loss_recovery` | same (note boxed) |
| `BinaryHeap` | `unsafe_binary_heap` (original) | `unsafe_lazy_hole_resort` (**winner**, production) | lazy-reconcile `peek_mut` + `PanicProtection` resort |

All fixes are **proven** by counted-drop forget tests (baseline asserts the leak, fixed asserts zero
loss). Tests pass: Vec 145, VecDeque 109, BinaryHeap 34/1.

## Cross-collection results (this run)

`ratio = fixed ÷ baseline` (>1 = the forget-safe variant is slower).

| collection | benches | **median** | geomean | mean | time-weighted | headline cost |
|------------|--------:|-----------:|--------:|-----:|--------------:|---------------|
| `Vec` | 67 | **1.018** | 1.143 | 1.414 | 1.029 | tiny-`clone`/`construct` tax; `pop` micro-artifact |
| `VecDeque` | 16 | **1.014** | 1.084 | 1.118 | 1.367 \* | `into_iter_fold` 1.19; `drain` unstable \* |
| `BinaryHeap` | 6 | **1.008** | 1.040 | 1.065 | 1.003 | `find_smallest` 1.41 (the `peek_mut` mechanism) |

\* `VecDeque`'s time-weighted ratio is dominated by `drain_sum_50k`, which swung 0.99× ↔ 2.47× across
runs on this loaded box (std `drain` alone ranged 20–35 µs). It is a measurement artifact this
session, not a regression — `pop_front_50k` is stable at 0.99. Use the median.

**The typical bench in every collection is at parity** (median 1.01–1.02). The cost is never on the
hot macro path — it is a small, fixed per-op tax, and it shows up in exactly one place per collection:

- **`Vec`** — fixed ~1–3 ns on tiny `clone`/`construct` (e.g. `clone@0` 1.75×, `from_slice@0` 6.9× on
  a no-op; parity by ~1000 elements). `pop_50k` 1.83× is a micro-artifact (~0.2 ns/op — codegen
  wobble doubling the ratio while the absolute cost is trivial). Macro ops (`grow` 1.00, `drain`,
  `iter`, `dedup@100k`) parity.
- **`VecDeque`** — after boxing the note, the tiny-op tax is gone (`new` 0.98×, `extend_bytes` 1.04×,
  down from 2.5× / 1.26× inline). Residual: `into_iter_fold` 1.19× (the note rides in `IntoIter`),
  `grow_1025` 1.08×. Macro ops parity.
- **`BinaryHeap`** — `find_smallest_1000` 1.41× is the one real cost, and it is the lazy `peek_mut`
  *mechanism* (flag load/test on a 99k-iteration peek loop), **not** the panic protection (which is
  provably zero-cost on `from_vec`). Everything else parity or faster (`pop` 0.71×, `into_sorted_vec`
  0.96×). `peek_mut_deref_mut` 1.29× is a non-measurement (dead-store loop).

## Verdict

The same shape holds across all three: **forget/panic safety is free on the hot and typical paths**;
the cost is always a **small, localized tax**. But the *decision* diverges on where that tax lands:

- **`BinaryHeap` adopted its fix** as the production heap. Its cost is confined to `peek_mut` and it
  buys **strictly more correctness than `std`** (no tail loss on a forgotten mutated `peek_mut`,
  self-healing order after a comparison panic). It could afford its safety.
- **`Vec`/`VecDeque` did not.** Their fix taxes the trivially-cheap `construct`/`clone` of *every*
  value — you cannot add even one field to `Vec` without paying on its sub-nanosecond ops. That
  pervasive small-op tax is exactly what upstream rejects, which is why `std` ships the **zero-field
  leak-amplification** design (RFC 1066): it will not pay a fixed 2 ns on `clone` to make `forget`
  lossless. `lazy_loss_recovery` is a **study variant** — it proves the leaks are closable with no
  macro-path cost, and measures the small-op tax that closing them costs.

## Details / reproduce

- Per-collection detail (methodology, full per-workload tables, the leak/fix mechanics):
  `VecLeakage.md`, `VecDequeLeakage.md`, `BinaryHeapLeakage.md` (this directory).
- Rerun (best on a quiescent box):
  ```
  cd products/rustseal
  scripts/bench.sh --bench vec_compare
  scripts/bench.sh --bench vec_deque_compare
  scripts/bench.sh --bench compare            # binary_heap: original vs winner
  ```
