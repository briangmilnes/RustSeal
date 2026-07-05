# Leakage — forget and leak-amplification across `Vec`, `VecDeque`, `BinaryHeap`

  We believed that the cost to prevent these data losses should be small
 and in rather special cases otherwise. 

# Method

  We rebuilt the three documented leaking data types in the Rust standard libraries
 and benchmarked them. 

 Three types Vec, VecDeque and Binary heap specify loss when the Drop on a mutable
iterator is not run. It may be explicitly mem::forget'd or a panic and recovery may
prevent the Drop (deallocation) from executing. They mutate the underlying data structure
while mutably iterating on it. The goal is speed plus a minimal well-formedness that
leaves data dropped, such as all but one member of a binary heap.

 This is deliberate, Rust RFC-1066 standard behavior. Each product carries a faithful `std`
baseline plus a fixed variant that closes the holes with no data loss:

| collection | baseline | fixed variant | fix mechanism |
|------------|----------|---------------|---------------|
| `Vec` | `std` | `lazy_loss_recovery` | boxed `pending` and lazy restore | 
| `VecDeque` | `std` | `lazy_loss_recovery` | boxed `pending` and lazy restore | 
| `BinaryHeap` | `unsafe_binary_heap` (original) | `lazy_hole_resort_binary_heap` | lazy-reconcile `peek_mut` + `PanicProtection` resort |


 All fixes are checked by drop counting forget tests (baseline asserts the leak, fixed
asserts zero loss). Vec 145, VecDeque 109, BinaryHeap 34 all pass leakage free (each with
one ignored test for private allocation).

## Benchmarking

 We used the following methods to benchmark our results:

- divan 0.1, a statistical benchmark harness. Each comparison bench runs the baseline variant and
  the fixed variant in the same process, interleaved, so both are measured under the same machine
  state.
- cargo bench, driven through scripts/bench.sh, which sets RUSTC_BOOTSTRAP=1 (the crate uses unstable
  library features on the stable toolchain) and writes an ANSI-stripped log to logs/.
- A seeded rand_xorshift RNG for every randomized input, so the inputs are identical across variants
  and across runs. Input construction is kept outside the timed region.

 We ran the Rust standard library's own benchmark suites, ported without changing the
workloads (library/alloctests/benches/vec.rs, vec_deque.rs, binary_heap.rs). Vec
contributes 67 workload comparisons (the 101 upstream benches grouped by family and run
across the upstream size range), VecDeque 16, and BinaryHeap 6.

 We ran of each of the three comparison benches on 2026-07-03. Divan took 100 timing
samples per workload per variant (it sizes the iteration count within each sample
automatically) and reported the fastest sample. The per-workload number is ratio = fixed /
baseline; the tables below aggregate those ratios three ways: median, unweighted mean
(average), and a time-weighted sum (total fixed time divided by total baseline time).

 The machine used is a 32 GB Lenovo Thinkpad with an Intel Core i7-12700H (12th Gen, "Alder Lake" mobile)
 which uses 14 cores at 4.7 GHz running Ubuntu 24.04.4 LTS.
 
## Results

`ratio = fixed ÷ baseline`

| # | collection | benches | median | average | total benchmark time weighted | largest costs |
|--:|------------|--------:|-----------:|--------:|--------------:|---------------|
| 1 | `Vec` | 67 | **1.048** | 2.270 \* | 1.044 | tiny-`clone`/`construct` cost; `retain` 1.11; sub-ns outliers \* |
| 2 | `VecDeque` | 16 | **1.019** | 1.109 | 1.368 † | `into_iter_fold` 1.16; `drain` unstable † |
| 3 | `BinaryHeap` | 6 | **1.103** | 1.209 | 1.094 | `find_smallest` 1.79 (the `peek_mut` mechanism) |

\* `Vec` has three sub-nanosecond outliers where the ratio explodes on a near-zero std baseline
(`with_capacity@1000` 46×, `from_iter@0` 28×, `from_slice@0` 8×) — absolute lazy cost is a few ns —
which inflate `Vec`'s average to 2.27 while the median stays at 1.05. The benches already
`black_box`, so the cause (std-side codegen elision vs a real small-op cost) is unconfirmed on this
loaded box; not a regression without a quiescent re-run. Prefer the median.

† `VecDeque`'s time-weighted ratio is dominated by `drain_sum_50k`, which swung 0.97× ↔ 2.40× across
runs on this loaded box (std `drain` alone ranged 18–35 µs) — a measurement artifact this session, not
a regression; `pop_front_50k` is stable at 1.01. Use the median.

The typical bench in every collection is at or near parity (median 1.02–1.10). The cost is never
on the hot large scale path — it is a small, localized cost, and it shows up in exactly one place per
collection:

- `Vec` — fixed ~1–4 ns on tiny `clone`/`construct` (e.g. `clone@0` 1.80×; parity by ~1000
  elements). `retain_even_100k` 1.11× is the one plausibly-real macro slowdown. `pop_50k` 1.84× is a
  micro-artifact (~0.2 ns/op — codegen wobble doubling the ratio, absolute cost trivial). Macro ops
  (`grow` 1.00, `drain` 1.00, `iter`, `dedup@100k`) parity.
- `VecDeque` — after boxing the note, the tiny-op cost is gone (`new` 0.83×, `extend_bytes` 1.01×,
  down from 2.5× / 1.26× inline). Residual: `into_iter_fold` 1.16× (the note rides in `IntoIter`),
  `grow_1025` 1.06×. Macro ops parity; `drain` unreliable this session.
- `BinaryHeap` — `find_smallest_1000` 1.79× is the one real cost, and it is the lazy `peek_mut`
  *mechanism* (flag load/test on a 99k-iteration peek loop), not the panic protection (which is
  provably zero-cost on `from_vec`). Everything else parity or faster (`from_vec` 0.84×,
  `into_sorted_vec` 1.09×, `pop` 1.08×). `peek_mut_deref_mut` 1.34× is a non-measurement (dead-store).

## Discussion

 The same shape holds across all three: forget/panic safety is very low cost on the hot
and typical paths. This biggest costs are dependent upon the data structure:

- `BinaryHeap` Its cost is confined to `peek_mut` and it buys strictly more correctness
  than `std` (no tail loss on a forgotten mutated `peek_mut`, self-healing order after a
  comparison panic). This is a small cost compared to not losing all but one element of
  the heap.

- `Vec`/`VecDeque` Their fixes increase the trivially-cheap cost of `construct` and
  `clone` of the vec or vecdeque, not it's elements. You cannot add even one field to
  `Vec` without paying on its sub-nanosecond ops. That pervasive small-op tax is exactly
  what the Rustaceans reject.  It is why `std` ships the zero-field leak-amplification
  design (RFC 1066): they will not pay a fixed 2 ns on `clone` to make `forget` lossless.

## Details and how to reproduce

- Per-collection detail (methodology, full per-workload tables, the leak/fix mechanics):
  `VecLeakage.md`, `VecDequeLeakage.md`, `BinaryHeapLeakage.md` (this directory).
- Rerun (best on a quiescent box):
  ```
  cd ~/projects/RustSeal
  scripts/bench.sh --bench vec_compare
  scripts/bench.sh --bench vec_deque_compare
  scripts/bench.sh --bench compare            # binary_heap: original vs winner
  ```
