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

 We ran the Rust standard library's own benchmark suites, ported ONE-TO-ONE and faithfully:
one Divan bench per real `#[bench]` in library/alloctests/benches/{vec,vec_deque,binary_heap}.rs
(Rust 1.97.0), each reproducing the real body verbatim — the timed region includes whatever
setup the real bench times (the `clone`/`extend`/`clear`), state persists across iterations where
the real bench declares it outside `b.iter`, and every real `black_box` is kept. The real element
types are preserved (vec: u8/u32/u128/usize/i32 and a `Droppable`; vec_deque: i32/usize/u8/u16;
binary_heap: u32). Counts: BinaryHeap 6 benches, VecDeque 15, Vec 101 (the `in_place` type-variant
family expands to 118 Divan functions across u8/u32/u128). See docs/ExistingVecBenchmarks.md,
ExistingVecDequeBenchmarks.md, ExistingBinaryHeapBenchmarks.md for the per-bench list and verdicts.

 Where the real bench used libtest's `unsafe` buffer-reuse helper only to exclude per-iteration
allocation from timing, we use Divan's native untimed setup (`with_inputs().bench_values()`) — the
identical measured operation, no `unsafe`. We ran each suite on 2026-07-04. Divan took 100 timing
samples per workload per variant and reported the fastest sample. The per-workload number is
ratio = fixed / baseline; the tables aggregate those ratios three ways: median, unweighted mean
(average), and a time-weighted sum (total fixed time / total baseline time). BOGUS benches (a
trivial empty constructor, or a loop the optimizer deletes) are excluded from the REAL aggregates.

 The machine used is a 32 GB Lenovo Thinkpad with an Intel Core i7-12700H (12th Gen, "Alder Lake" mobile)
 which uses 14 cores at 4.7 GHz running Ubuntu 24.04.4 LTS.
 
## Results

`ratio = fixed ÷ baseline`

Faithful 1:1 port, 2026-07-04. `Rust Lib benches` = real upstream `#[bench]`; `ported to Divan` =
Divan functions (Vec's `in_place` type-variant family expands across u8/u32/u128); `likely
meaningless` = bogus (empty constructor, or a loop the optimizer deletes). The median, average, and
time-weighted ratios are over the real benches — the meaningless ones are excluded.

| # | collection | Rust Lib benches | ported to Divan | likely meaningless | median | average | time-weighted | largest real costs |
|--:|------------|-----------------:|----------------:|-------------------:|-------:|--------:|--------------:|--------------------|
| 1 | `Vec` | 101 | 118 | 13 | **1.05** | 1.18 | **1.01** | `from_slice@10` 2.28×, `from_iter@10` 1.99× |
| 2 | `VecDeque` | 15 | 15 | 1 | **1.02** | 1.02 | **1.04** | `grow_1025` 1.12× |
| 3 | `BinaryHeap` | 6 | 6 | 1 | **1.01** | 1.09 | **1.04** | `find_smallest_1000` 1.42× |

The typical bench in every collection is at or near parity (median 1.01–1.05), and by wall-clock the
whole `Vec` suite is only **1% slower** (time-weighted 1.01) — the large-payload ops are parity; the
cost is a small fixed per-operation tax that shows up only at small sizes:

- `Vec` (105 real of 118) — the large-payload ops are parity (`dedup@100000` 1.00, `flat_map` of 500k
  1.00, `retain_100000` ~1.04, `clone@1000` ~1.02). The cost concentrates on **small-N construct /
  `clone_from`**, where the lazy variant's boxed `pending` field is a fixed per-operation cost:
  `from_slice@10` 2.28×, `from_iter@10` 1.99×, `clone_from ×10` at 10 elements ~1.9× — absolute a
  few–100 ns, gone by ~1000 elements. 13 bogus = the size-0 / empty-payload family members (`new`,
  every `*_0000`), whose ratios are timer/allocator floor.
- `VecDeque` (14 real of 15) — parity throughout (median 1.02, time-weighted 1.04); residuals
  `grow_1025` 1.12×, `try_fold` 1.10×, `from_array_1000` 1.08×. `new` is bogus (empty construct, below
  the ~19 ns timer floor). NOTE: the real suite has **no `drain` bench** — the earlier family port
  had invented one.
- `BinaryHeap` (5 real of 6) — `find_smallest_1000` 1.42× is the one real cost, the winner's lazy
  `peek_mut` flag load/test on a 99k-iteration peek loop; `from_vec` 0.98×, `into_sorted_vec` 1.01×,
  `pop` 1.03×, `push` 0.99× are all parity once the setup is timed faithfully (the family port's
  0.84/1.12 were artifacts of un-timing the `clone`/`clear`). `peek_mut_deref_mut` is bogus — 1.1 ns
  for a 1,000,000-write loop, i.e. the optimizer deleted it (the writes go through a forgotten guard).

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

## What the faithful port corrected

The suites here are a one-to-one reproduction of Rust's own alloc benchmarks
(`library/alloctests/benches/{vec,vec_deque,binary_heap}.rs`, 1.97.0). Building them faithfully
overturned several numbers the earlier family port had reported:

1. **Timing the setup the real benches time flips the "cost" numbers to parity.** The family port
   kept per-iteration setup out of the timed region; the real benches time it. `BinaryHeap`
   `from_vec` 0.84×→**0.98×** and `push` 1.12×→**0.99×** were entirely artifacts of un-timing the
   `clone`/`clear`. Timed faithfully, the only real heap cost is `find_smallest_1000` (1.42×).

2. **`peek_mut_deref_mut` is a non-measurement.** It runs at ~1.1 ns for a loop that writes
   1,000,000 values, so the optimizer deleted the loop — the writes go through a `mem::forget`ed
   `PeekMut` guard and are dead code. Its own `black_box(&vec)` does not prevent this. The upstream
   author intended it to defeat optimization; it does not. Reading it as a real number is a mistake.

3. **The real `VecDeque` suite has no `drain` bench.** The earlier family port had *added*
   `drain_sum_50k`; there is no such upstream benchmark. The drain forget-safety cost is simply not
   in Rust's own suite.

4. **The fix's cost is a fixed per-operation field tax, not a per-element cost.** It is the boxed
   `pending` field's init/copy on the container header, so it appears only where the element payload
   is near-zero — `from_slice`/`from_iter`/`clone_from` at ~10 elements (up to ~2.3×, absolute a
   few–100 ns) — and vanishes by ~1000 elements. Weighted by wall-clock the whole `Vec` suite is
   **1% slower** (time-weighted 1.01); the large-payload ops (`dedup@100000`, `flat_map` of 500k) are
   parity. This is exactly the profile RFC 1066 rejects: not a big-op cost, a pervasive small-op one.

## Caveats

- Single run on a **non-quiescent** ThinkPad P1 (i7-12700H, hybrid P/E cores, frequency scaling,
  Ubuntu 24.04). Sub-nanosecond rows and the small-N (~10-element) ratios are noise-prone. A
  quiescent re-run — one P-core pinned (`taskset -c 0`), `performance` governor — would firm up the
  small-N numbers; the harness is in place to do it.
- Aggregates are over the **real** benches only; bogus rows (empty constructors, the deleted
  `peek_mut_deref_mut` loop) are excluded, and a bogus-inclusive average is meaningless (e.g. `new`'s
  sub-nanosecond 10× ratio pulls the raw `VecDeque` average to 1.66).

## Details and how to reproduce

- Per-collection detail (real bench list, real element types, bogus/real verdict per bench):
  `ExistingVecBenchmarks.md`, `ExistingVecDequeBenchmarks.md`, `ExistingBinaryHeapBenchmarks.md`
  (this directory).
- Rerun (best on a quiescent box):
  ```
  cd ~/projects/RustSeal
  scripts/bench.sh --bench real_binary_heap   # original vs winner
  scripts/bench.sh --bench real_vec_deque     # std vs lazy_loss_recovery
  scripts/bench.sh --bench real_vec
  ```
