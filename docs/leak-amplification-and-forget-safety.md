# `peek_mut`, forget-safety, leak amplification, and `Rc` cycles

Design notes for rustseal's three binary-heap variants, and the Rust-semantics reasoning
behind their `peek_mut` implementations. Captures the r0286–r0438 discussion so the
tradeoff is not re-derived from scratch.

## The three variants

| variant (`src/…`) | `peek_mut` mechanism | forget guarantee | mutated-peek cost | needs `A: Clone` |
| ----------------- | -------------------- | ---------------- | ----------------- | ---------------- |
| `unsafe_binary_heap` | `set_len` leak amplification (faithful std) | yes | *O*(log n) | no |
| `safe_binary_heap`   | tail-split (`split_off`/`append`) | yes | **_O_(n)** | yes (for `deref_mut`) |
| `safe_opt_binary_heap` | `&mut data[0]` + sift on `Drop` | **no (dropped)** | *O*(log n) | no |

`unsafe_*` is the faithful rust-libs extraction. `safe_*` is a zero-`unsafe`-block
re-implementation that keeps every behavioral guarantee. `safe_opt_*` is `safe_*` with one
deliberate change: it trades the forget guarantee for an *O*(log n) `peek_mut`.

## How `peek_mut` works: deferred reorganization

`peek_mut()` hands you a `&mut` to the **real** greatest element, in place at `data[0]`
(not a copy). Mutating it can break the heap invariant (you may have lowered the max below
its children). The fix-up — `sift_down(0)` — is **deferred to the guard's `Drop`**:

```rust
// the reorg fires only if you actually mutated (deref_mut set the flag/tail/original_len)
impl Drop for …PeekMut { fn drop(&mut self) { … self.heap.sift_down(0); } }
```

In `bench_peek_mut_deref_mut` the guard is `mem::forget`-ed, so `Drop` never runs and the
sift is never called — that is *why* the benchmark's mutation triggers no reorg (see
"benchmark evidence").

## The forget problem: `Drop` is optional

A mutated `peek_mut` guard that is **leaked** (its `Drop` skipped) would leave the heap with
a too-small value at the root and the rest in (now invalid) order. The hard constraint:

> In Rust, a destructor is **not guaranteed to run**. Leaking is *safe*. So no safety or
> validity invariant may depend on `Drop` executing.

This is the post-"leakpocalypse" rule (RFC 1066, ~Rust 1.0). `mem::forget` is a **safe**
function, and even without it you can leak in safe code (see "what leaks in safe Rust"). So
`peek_mut` must leave the heap valid *even if the guard is forgotten*.

## Leak amplification (the std technique)

"Leak amplification" is a real standard-library term (it appears verbatim in
`BinaryHeap::PeekMut`'s source: *"described throughout … the standard library as 'leak
amplification.'"*). It is **not** language-spec vocabulary — it's stdlib lore.

The technique: put the structure into an **already-valid** state *before* handing out the
guard, so that if the guard is leaked, the only consequence is that **more leaks** (the leak
is "amplified") — never corruption or UB. The canonical example is `Vec::drain`, which sets
`len = 0` immediately; forget the `Drain` and the not-yet-moved elements simply leak, but the
`Vec` is never left counting moved-from slots.

For the heap, two mechanisms achieve the same property:

- **`unsafe_binary_heap` — `set_len` (the genuine technique).** `deref_mut` does
  `set_len(1)`: the heap is logically `[root]`, the other elements sit in the allocation
  *beyond* `len`. *O*(1) — nothing moves. On `Drop`, `set_len(restore) + sift_down`; on
  forget, the tail stays leaked (beyond `len`) and `[root]` is a valid heap.

  ```rust
  self.original_len = Some(NonZero::new_unchecked(len));
  self.heap.data.set_len(1);                 // O(1): just the length field
  ```

- **`safe_binary_heap` — tail-split (a safe analog).** Safe Rust cannot leave initialized
  elements beyond `len`, so `deref_mut` physically **moves** the tail into the guard:
  `split_off(1)`. *O*(n) + an allocation, and `Vec::split_off` clones the allocator (hence
  `A: Clone` on the `DerefMut` impl). On `Drop`, `append` moves it back + sift; on forget,
  the tail (held in the guard) leaks and `[root]` is valid.

  ```rust
  self.tail = Some(self.heap.data.split_off(1));   // O(n) move + alloc
  ```

Same *property* (forget → bigger leak, never corruption); different *mechanism* (a length
tweak vs a real move). Calling the safe version "leak amplification" is borrowing the term by
analogy — the genuine technique is the `unsafe` `set_len` one.

## `safe_opt_binary_heap`: dropping the guarantee for *O*(log n)

`safe_opt`'s `peek_mut` is the textbook design: `deref_mut` just returns `&mut data[0]`
(*O*(1)) and sets a `sift_on_drop` flag; `Drop` does one `sift_down(0)` (*O*(log n)). No
tail-split, no `A: Clone`, works for any allocator.

The cost: it **drops the forget guarantee**. Mutate the max, forget the guard, and the sift
never runs — the heap is left in **broken order**. That is a *logic* error, never UB. The
shared test suite's `test_peek_mut_leek` (which asserts forget leaves a valid heap) is
therefore `#[ignore]`d for `safe_opt` — the invariant it checks is intentionally absent.

This is a legitimate design axis: "don't `forget` a mutated `PeekMut`, or the heap order
breaks" is a documentable contract that buys *O*(log n) and drops the *O*(n) insurance.

## Panic vs leak

A common confusion (worth stating once): **the leak-amplification is for the *leak* case,
not the panic case.**

- A panic that **unwinds** runs every in-scope destructor *during* unwinding — so the
  guard's `Drop` *does* run and re-sorts the heap. Panic-unwind is handled by `Drop`; the
  amplification adds nothing there.
- A panic with `panic = "abort"`, or a double-panic (a destructor panicking mid-unwind),
  **aborts the process** — that's process death, the OS reclaims everything; not an ongoing
  leak.
- `catch_unwind` (the only way to *catch* a panic, and itself not guaranteed — `panic =
  "abort"` makes it impossible) runs destructors as the stack unwinds, so a caught panic
  doesn't leak either.

So the amplification earns its keep **only when `Drop` is skipped and the program keeps
running** — i.e. a genuine leak, below.

## What leaks in safe Rust (no `unsafe`)

Setting panics aside (unwinding *runs* `Drop`), the safe-code leak sources are:

1. **`Rc`/`Arc` reference cycles** — *the* structural one. Two `Rc<RefCell<T>>` pointing at
   each other keep each other's strong count ≥ 1 forever; dropping external handles never
   breaks the cycle, so the nodes are never dropped. Entirely safe, and *accidental* (build a
   graph, forget `Weak` back-edges). This is exactly the "reference counting can't collect
   cycles" weakness that a **tracing GC** (OCaml/SML) *does* collect, and it is the reason
   Rust declared leaking safe (you can't ban it without banning `Rc`).
2. **`mem::forget(x)`** — deliberate, safe, skips `Drop`.
3. **`Box::leak` / `Vec::leak` / `String::leak`** — deliberate; returns `&'static mut`.
4. **`ManuallyDrop<T>` built and abandoned** — safe to construct; never calling the (unsafe)
   `ManuallyDrop::drop` leaks it.
5. **Logical / space leaks** — keeping data *reachable* but unused (an unbounded cache, an
   ever-growing `Vec`). Not a skipped-`Drop` leak, but the same OOM trajectory — and the one
   class even a tracing GC cannot prevent (GC frees the *unreachable*, not the reachable-dead).

Only #1 is the "Rust permits it, GC'd MLs don't" case; #2–#4 are deliberate leaks you ask for
by name; #5 is universal. None require `unsafe`; none are UB.

## Type-soundness framing

Type safety = **progress + preservation** = no well-typed program reaches a *stuck* state (an
operation with no defined transition — i.e. UB). It says **nothing about resources or
termination**:

- *Preservation*: leaking doesn't make a well-typed program ill-typed — it stays well-typed,
  the store just grows.
- *Progress*: a leaking program can always step. When allocation finally fails, that is a
  **defined** transition (the allocator handler aborts, or `try_reserve` returns `Err`).
  Abort is a *terminal* state in the semantics, not a *stuck* one. OOM is well-defined
  failure, not "going wrong."

So **memory safety ≠ memory leak-freedom.** Rust draws its safety boundary around UB and
parks leaks (incl. OOM-and-abort) firmly on the *safe* side, by design. The intent of
bounded memory is real and is what GC (reachability, collects cycles) and region inference
(Tofte–Talpin / MLKit, static scoped deallocation) fulfill in the ML world — but it is *not*
part of Rust's soundness statement, which is why `Drop` is formally optional and every
`Drop`-guard must survive being skipped.

## Benchmark evidence

Three-way benches (`scripts/compare-safe-unsafe`; ns/iter, lower = faster):

| bench | unsafe | safe | safe_opt | safe/unsafe | safe_opt/unsafe |
| ----- | -----: | ---: | -------: | ----------: | --------------: |
| find_smallest_1000 | 111,263 | 811,748 | 258,738 | 7.30 | 2.33 |
| from_vec | 289,703 | 695,481 | 655,115 | 2.40 | 2.26 |
| into_sorted_vec | 198,956 | 413,018 | 421,703 | 2.08 | 2.12 |
| peek_mut_deref_mut | 15,797 | 549,867 | 15,926 | 34.81 | 1.01 |
| pop | 267,458 | 445,053 | 435,740 | 1.66 | 1.63 |
| push | 462,973 | 540,306 | 568,160 | 1.17 | 1.23 |
| **geomean** | | | | **3.67** | **1.68** |

Two findings:

- **The leak-amplification premium is paid every iteration.** `find_smallest` mutates the
  max ~1% of the time; each mutation pays `safe`'s *O*(n) `split_off`+`append` *even though
  the workload never forgets or panics*. Removing that premium (`safe_opt`) is the
  811,748 → 258,738 (3.14×) speedup. The residual `safe_opt`/`unsafe` 2.33× is a *different*
  cost — the swap-vs-hole sift (`Vec::swap` moves each element twice where the `Hole` moves
  it once). So `safe`'s 7.30× factors as ≈ (2.3× swap-vs-hole) × (3.1× leak-amplification).

- **`peek_mut_deref_mut` is a non-measurement.** It writes the same slot 1,000,000 times and
  `forget`s the result — a dead-store loop whose result is never read. `unsafe` (16k) and
  `safe_opt` (16k) let the optimizer **delete the whole loop** (sub-cycle-per-store is
  physically impossible — the stores were erased). `safe` (550k, 34.81×) does *not* — and the
  cause is **not** the bounds-checked `data[0]` (so does `safe_opt`). It is the `split_off`
  branch in `safe`'s `deref_mut`: reallocating code on a (never-taken) path stops LLVM from
  proving `len`/the `data[0]` pointer stay invariant across the loop, so it keeps the check
  and runs the stores. Remove the branch (`safe_opt`) and the loop optimizes away exactly
  like `unsafe`. The 34.81× measures "the optimizer can delete a dead loop," not `peek_mut`.

## Choosing a variant

- Want the faithful std behavior incl. forget-safety, and `unsafe` is acceptable →
  `unsafe_binary_heap`.
- Want forget-safety with **zero unsafe blocks**, and can pay *O*(n) per mutated peek (and
  `A: Clone`) → `safe_binary_heap`.
- Want a zero-unsafe heap with *O*(log n) `peek_mut` for any allocator, and can promise
  "don't `forget` a mutated `PeekMut`" → `safe_opt_binary_heap`.

All three: zero unsafe *blocks* in the two safe variants (the lone `unsafe` tokens are the
`TrustedLen` length assertions), all build clean, validate `0 verified / 0 errors`, and pass
the shared test suite (`safe_opt` `#[ignore]`s the forget-guarantee test by design).
