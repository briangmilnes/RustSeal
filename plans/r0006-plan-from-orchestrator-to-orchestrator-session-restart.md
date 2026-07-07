---
round: r0006
from: orchestrator
to: orchestrator
subject: session-restart
date: 2026-0707-11:13
status: active
---

# Session restart — faithful real-bench port complete; results ready to discuss

## Where we are (one line)

The r0005 goal is **done**: all of Rust's real alloc benchmarks are faithfully ported
1:1 into Divan, running against both variants, classified bogus/real, with real
median/average/time-weighted numbers. Results are good enough to discuss with folks.

## Branch state

- Branch `main`, latest commit `01f5eab` (docs cleanup). No in-flight code changes.
- One uncommitted README.md edit (user's) at restart time.

## What was built (r0005)

Faithful 1:1 Divan ports of `~/projects/rust/library/alloctests/benches/{vec,vec_deque,
binary_heap}.rs` (Rust **1.97.0**), run against our two extracted variants each:

| file | benches | Cargo `[[bench]]` |
|------|--------:|-------------------|
| `benches/binary_heap/real_binary_heap.rs` | 6 | `real_binary_heap` |
| `benches/vec_deque/real_vec_deque.rs` | 15 | `real_vec_deque` |
| `benches/vec/real_vec.rs` | 118 (101 `#[bench]`; `in_place` macro → 18) | `real_vec` |

Fidelity: one Divan bench per real `#[bench]`; real bodies (timed region includes the
`clone`/`extend`/`clear` the originals time; persistent state; real `black_box`);
**real element types** (vec u8/u32/u128/usize/i32/`Droppable`; vec_deque i32/usize/u8/u16;
binary_heap u32). The 3 vec_deque `into_iter*` benches whose upstream form used an `unsafe`
`Vec::from_raw_parts` buffer-reuse helper were ported with Divan's native
`with_inputs().bench_values()` untimed setup — identical measured op, no `unsafe`.

## Results (real benches; bogus excluded; fastest of 100 samples, non-quiescent box)

| collection | benches | meaningless | median | average | time-weighted | largest real cost |
|------------|--------:|------------:|-------:|--------:|--------------:|-------------------|
| Vec | 118 | 13 | 1.05 | 1.18 | 1.01 | `from_slice@10` 2.28×, `from_iter@10` 1.99× |
| VecDeque | 15 | 1 | 1.02 | 1.02 | 1.04 | `grow_1025` 1.12× |
| BinaryHeap | 6 | 1 | 1.01 | 1.09 | 1.04 | `find_smallest_1000` 1.42× |

Key findings (written up in `docs/Leakage.md` → "What the faithful port corrected"):
faithful setup-timing flips the old family numbers to parity; `peek_mut_deref_mut` is a
deleted-loop non-measurement; the real VecDeque suite has **no drain bench**; the fix's cost
is a fixed per-op boxed-`pending`-field tax, visible only at small sizes (Vec wall-clock 1%).

## How to reproduce the numbers (for a fresh agent)

```
cd ~/projects/RustSeal
scripts/bench.sh --bench real_binary_heap   # original vs winner
scripts/bench.sh --bench real_vec_deque     # std vs lazy_loss_recovery
scripts/bench.sh --bench real_vec           # heavy: ~10 min
```

Parsing the Divan log to ratios (the pairing rule matters):
- **binary_heap** variants have DIFFERENT type names, so Divan sorts alphabetically →
  `LazyHoleResort…`(winner) prints FIRST, `Unsafe…`(original) SECOND → `ratio = winner/original
  = first/second`.
- **vec / vec_deque** both variants print the SAME name (`VVec<T>` / `VVecDeque<T>`), so Divan
  keeps declaration order `types=[Std, Lazy]` → Std FIRST, Lazy SECOND → `ratio = lazy/std =
  second/first`.
- Bogus sets excluded from aggregates: binary_heap `{peek_mut_deref_mut}`; vec_deque `{new}`;
  vec `{new}` ∪ `{names ending _0000}` (13). Aggregates = median, unweighted mean (average),
  time-weighted (Σfixed/Σbaseline) over the fastest column.
  (A one-off parser was used this session; recreate from these rules — it is not committed.)

## Docs (all committed)

- `docs/Leakage.md` — the summary writeup (methods, Results table, findings, caveats, reproduce).
- `docs/Existing{Vec,VecDeque,BinaryHeap}Benchmarks.md` — the faithful per-bench catalog + verdicts.
- `docs/{Vec,VecDeque,BinaryHeap}Leakage.md` — per-collection deep-dive (mechanism + a "Likely
  meaningless benches" section). NOTE their §3–§4 cost tables still show the OLD family-port
  numbers — see open decision 3.

## Open decisions / remaining work (none blocking the discussion)

1. **Quiescent re-run.** All numbers are a single run on a non-quiescent ThinkPad P1
   (i7-12700H hybrid P/E cores, freq scaling). Firm up the small-N ratios and the sub-ns
   bogus rows with: P-core pinned (`taskset -c 0`), `performance` governor, machine idle.
   The harness is ready; just re-run the three commands above and re-derive the aggregates.
2. **Retire the old family `compare.rs` benches?** `benches/{binary_heap/compare,
   vec_deque/compare,vec/compare}.rs` still exist and are still `[[bench]]` targets, now
   superseded by the `real_*` ports. Decide: delete them, or keep as the interleaved
   "study" comparison. The `real_*` ports are canonical.
3. **Refresh the §3–§4 cost tables in the three `*Leakage.md`** to the faithful numbers
   (they still carry the family-port per-workload tables). Or fold those docs into the
   `Existing*Benchmarks.md` catalogs to remove the overlap.
4. **r0002 (`fix-csts-analyze-loc-logging`) is still pending** — a `to-user` plan for the
   CSTs orchestrator (a different project); unrelated to the bench work.

## Resume in one step

Nothing is broken and nothing is half-done. To continue: pick an open decision above, or
run the quiescent re-run (1) to get discussion-grade numbers. `git log --oneline` from
`044aff4` shows the full r0005 arc.
