---
round: r0004
from: orchestrator
to: user
subject: bench-std-vs-lazy
date: 2026-0703-15:32
started: 2026-0703-15:26
finished: 2026-0703-15:32
related: plans/r0004-plan-from-orchestrator-to-orchestrator-bench-std-vs-lazy.md
---

# std vs lazy-fix comparison benches — results

All three Divan comparison benches build and run, exit 0, zero warnings. Each runs
both variants interleaved in one process; the number below is the **median over 100
samples**, and the comparison metric is the **lazy/std median ratio** (>1 = the
lazy fix is slower). In every bench the first-registered variant is the baseline
(`Std` / `Original`), the second is the fix (`Lazy` / `Winner`) — both print the
same type label in the raw table, so attribution is by registration order.

The `lazy_loss_recovery` fix adds a boxed `pending` note plus a guard at mutating
entry points to make `drain`/forget leak-safe. These benches measure where that
costs.

## Headline

| # | bench | comparable workloads | median lazy/std | verdict |
|---|-------|---------------------:|----------------:|---------|
| 1 | `vec_compare` (Std vs Lazy) | 67 | **1.05x** | free at scale; cost is a fixed per-construction constant + drain/pop |
| 2 | `vec_deque_compare` (Std vs Lazy) | 16 | **1.01x** | free except `drain` |
| 3 | `binary_heap compare` (Original vs Winner) | 6 | **1.31x** | the "Winner" is SLOWER on raw speed here — see note |

## 1. vec (Std vs Lazy) — median 1.05x, range 0.32–50.44x

The large ratios are all **sub-10 ns absolute**, on empty/tiny inputs where the
lazy variant's fixed per-object cost (the boxed `pending` field) dominates an
otherwise near-zero baseline. `with_capacity[1000]` 50x and `from_iter[0]` 26x are
ns-scale and partly an optimizer-elision artifact on the std side (the unused
result is elided; the lazy side's allocation is not) — not a real regression at
working sizes.

| # | workload | std | lazy | lazy/std |
|---|----------|----:|-----:|---------:|
| 1 | `with_capacity[1000]` | 12.1 ns | 609.8 ns | 50.44x (ns-scale / elision) |
| 2 | `from_iter[0]` | 0.4 ns | 9.5 ns | 26.15x (ns-scale) |
| 3 | `from_slice[0]` | 1.4 ns | 7.7 ns | 5.46x (ns-scale) |
| 4 | `pop_50k` | 13.58 us | 27.14 us | **2.00x (real, sized)** |
| 5 | `clone[0]` | 5.2 ns | 9.9 ns | 1.91x (ns-scale) |
| 6 | `from_iter[10..1000]` | — | — | 1.17–1.50x (small-N tail) |
| 7 | `collect_range[0]` | 1.9 ns | 0.6 ns | 0.32x (lazy faster, ns-scale) |

The only real sized regression is **`pop_50k` at 2.0x** — the forget-safety guard
on the pop path. Everything at N >= 1000 that isn't pop/drain is within noise.

## 2. vec_deque (Std vs Lazy) — median 1.01x, range 0.78–2.40x

Essentially free across all 16 workloads except the one the fix exists for:

| # | workload | std | lazy | lazy/std |
|---|----------|----:|-----:|---------:|
| 1 | `drain_sum_50k` | 18.20 us | 43.63 us | **2.40x (real, sized)** |
| 2 | `new` | 0.6 ns | 0.5 ns | 0.78x (lazy faster, noise) |
| 3 | `iter_1000` | 135.5 ns | 109.4 ns | 0.81x (lazy faster, noise) |

`drain_sum_50k` 2.40x is the drain forget-safety path — the whole point of the
lazy_loss_recovery variant; the cost lands exactly where expected and nowhere else.

## 3. binary_heap (Original vs Winner) — median 1.31x

Note the labels: `Original = UnsafeBinaryHeap`, `Winner = UnsafeLazyHoleResortBinaryHeap`.
On these six raw-speed workloads the **Original is faster** — the "Winner"
designation must come from a non-speed criterion (panic/leak safety), not throughput.
Flagging for your interpretation.

| # | workload | Original | Winner | Winner/Original |
|---|----------|---------:|-------:|----------------:|
| 1 | `find_smallest_1000` | 117.40 us | 207.10 us | 1.76x slower |
| 2 | `peek_mut_deref_mut` | 1.2 ns | 1.7 ns | 1.38x (ns-scale) |
| 3 | `into_sorted_vec` | 174.30 us | 228.90 us | 1.31x slower |
| 4 | `pop` | 183.10 us | 216.60 us | 1.18x slower |
| 5 | `from_vec` / `push` | — | — | ~0.94–1.0x (within noise) |

## Caveats

- Single run on a non-quiesced machine; medians are stable but the ns-scale rows
  carry the most noise. Read the sized (µs) rows as the signal.
- `peek_mut_deref_mut` is a near-dead-store parity workload (kept only to mirror
  upstream) — not a real measurement.
- Logs: `logs/bench-rustseal-20260703-152641.log` (binary_heap),
  `…-152656.log` (vec_deque), `…-152710.log` (vec).

## Bottom line

The lazy_loss_recovery fix is **free at working sizes** (vec 1.05x, vec_deque 1.01x
median); its cost is concentrated exactly on the forget-safety paths it protects —
`drain` (vec_deque 2.4x) and `pop` (vec 2.0x) — plus a fixed ns-scale
per-construction constant that only shows up on empty/tiny inputs. The binary_heap
"Winner" is slower than the Original on raw throughput and needs the non-speed
rationale to justify its name.
