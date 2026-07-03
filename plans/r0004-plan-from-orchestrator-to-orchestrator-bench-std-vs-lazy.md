---
round: r0004
from: orchestrator
to: orchestrator
subject: bench-std-vs-lazy
date: 2026-0703-15:26
status: done
related: reports/r0004-report-from-orchestrator-to-user-bench-std-vs-lazy.md
---

# Run the three std-vs-lazy comparison benches and report

The rustseal crate ships three Divan statistical comparison benches, each running
two variants interleaved in one process (fair comparison, median over 100 samples):

1. `compare` — binary_heap `UnsafeBinaryHeap` (Original) vs
   `UnsafeLazyHoleResortBinaryHeap` (Winner).
2. `vec_deque_compare` — `vec_deque::std::VVecDeque` vs
   `vec_deque::lazy_loss_recovery::VVecDeque`.
3. `vec_compare` — `vec::std::VVec` vs `vec::lazy_loss_recovery::VVec`.

## Steps

1. `scripts/bench.sh --bench compare` → verify: table emits, exit 0.
2. `scripts/bench.sh --bench vec_deque_compare` → verify: 16 workloads, exit 0.
3. `scripts/bench.sh --bench vec_compare` → verify: ~67 comparable workloads, exit 0.
4. Parse each log; pair (std, lazy) by (workload, size-arg) — in every bench the
   registration order is `[Std, Lazy]` / `[Original, Winner]`, so the first row is
   the baseline. Compute the lazy/std median ratio; surface regressions >= 1.15x.
5. Write `reports/r0004-report-...-bench-std-vs-lazy.md`.
