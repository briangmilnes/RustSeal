# verus-lab

Scratch lab for **verus** experiments. Each `src/experiments/*.rs` file is a
single-file `verus!` source verified directly by the verus binary (no cargo
build) via `scripts/validate.sh`. A file may declare several items for one
self-contained scenario. Every file carries a `RESULT:` marker recording what
verus actually did (CLAUDE.md rule 16.2); failing experiments are left in place
as corpses (rule 16.3).

verus-lab is a **detached-workspace** cargo crate (own empty `[workspace]`;
excluded from the RustSeal root workspace). `src/lib.rs` is doc-only and does
NOT `mod`-declare the verus sources, so cargo never asks rustc to compile the
`verus!` macro files.

Verus binary: `$VERUS`, else `verus` on `PATH`, else the local release build at
`~/projects/verus/source/target-verus/release/verus`. Override with
`VERUS=/path/to/verus scripts/{validate,build}.sh`.

- `scripts/validate.sh` — verify every `src/experiments/*.rs` (or a file passed
  as an argument) with verus; logs to `logs/`. `build.sh` delegates to it.

## Experiments (`scripts/validate.sh`)

The first three establish an identity/mutation arc over a `Vec<u32>`; the fourth
is the intended failing corpse.

| # | file | question | result |
|---|---|---|---|
| 1 | `exp001_identity_immutable_iter_u32.rs` | prove an identity function over a **non-mutable** iterator (`v.iter()`) over `u32`? | SUCCEEDS — 2 verified, 0 errors |
| 2 | `exp002_set_ends_zero_index_partial_eq.rs` | mutate in place — set element 0 and the last to 0 by **indexing** — and prove the equality property (`final(v)@ == old(v)@.update(0,0).update(len-1,0)`)? | SUCCEEDS — 1 verified, 0 errors |
| 3 | `exp003_set_ends_zero_mut_cursor.rs` | do the same first/last change through a **mutable iterator** into the vector and prove it? | SUCCEEDS — 1 verified, 0 errors |
| 4 | `exp004_set_ends_zero_mut_cursor_forget.rs` | copy exp003 but **`mem::forget` the mutable iterator** — does the proof still work? | FAILS — verus rejects `core::mem::forget` (V713 unsupported) |

Goal met: experiments 1–3 validate; experiment 4 does not (the intended outcome).

## Leak/forget APIs — one experiment per README item (`scripts/validate.sh`)

One experiment per entry in the README's "Explicit leak/forget APIs in std"
list (RustSeal `README.md`), each calling the API inside `verus!` to check
whether verus rejects it. verus rejects 9 of the 10 (each `V713` "not
supported", i.e. no vstd specification for the leaking function); the single
exception is `ManuallyDrop`, which verus verifies — so a leak IS expressible in
verified-safe exec code through it.

| # | README item | file | result |
|---|---|---|---|
| 1 | `std::mem::forget` | `exp005_leak_mem_forget.rs` | FAILS — V713 `core::mem::forget` |
| 2 | `Box::leak` | `exp006_leak_box_leak.rs` | FAILS — V713 `alloc::boxed::…::leak` |
| 3 | `Vec::leak` | `exp007_leak_vec_leak.rs` | FAILS — V713 `alloc::vec::…::leak` |
| 4 | `String::leak` | `exp008_leak_string_leak.rs` | FAILS — V713 `alloc::string::…::leak` |
| 5 | `std::mem::ManuallyDrop` | `exp009_leak_manuallydrop.rs` | **SUCCEEDS — 1 verified, 0 errors (NOT rejected)** |
| 6 | `CString::into_raw` | `exp010_leak_cstring_into_raw.rs` | FAILS — V713 `…::into_raw` + V712 `CString` |
| 7 | `Box::into_raw` | `exp011_leak_box_into_raw.rs` | FAILS — V713 `alloc::boxed::…::into_raw` |
| 8 | `Rc::into_raw` | `exp012_leak_rc_into_raw.rs` | FAILS — V713 `alloc::rc::…::into_raw` |
| 9 | `Arc::into_raw` | `exp013_leak_arc_into_raw.rs` | FAILS — V713 `alloc::sync::…::into_raw` |
| 10 | `Vec::into_raw_parts` | `exp014_leak_vec_into_raw_parts.rs` | FAILS — V713 `alloc::vec::…::into_raw_parts` |

The nine `V713` rejections mean the leaking *function* has no verus
specification, so the call cannot appear in verified exec code without an
`assume_specification`. `ManuallyDrop::new` is different in kind: it is an
ordinary constructor of a transparent wrapper that verus models directly, so
`let _m = ManuallyDrop::new(x);` verifies while suppressing the inner value's
destructor — a leak reachable from verified-safe code (exp009). This is a
NOT-A-LIMITATION finding (rule 16.4), kept as an honest corpse rather than
forced to reject.

## Drop soundness — can a destructor prove something false? (`scripts/validate.sh`)

The thesis behind the leak experiments: RFC 1066 says a destructor is not
guaranteed to run, so no safe API may rely on `Drop` for memory safety. The
proof-world analogue: can a verus proof be made to depend on a `Drop`, so that
leaking the value (via `ManuallyDrop`, the one accepted leak API, exp009) proves
a runtime-false fact? This series attacks that and finds four independent
defenses — verus is sound.

| # | question | file | result |
|---|---|---|---|
| 1 | can an `impl Drop` carry a `requires`? | `exp015_drop_requires_rejected.rs` | FAILS — V588 "requires are not allowed on the implementation for Drop" |
| 2 | can a `Drop` state a false `ensures`? | `exp016_drop_ensures_false_rejected.rs` | FAILS — V003 postcondition not satisfied (body must prove the ensures) |
| 3 | can the caller rely on a *guaranteed* drop's `ensures` about external `&mut` state? | `exp017_drop_ensures_external_mutation.rs` | FAILS — V017: the drop verifies but the post-scope `assert(x == 0)` is not provable |
| 4 | does `ManuallyDrop`-skipping that drop prove the (false) fact? | `exp018_manuallydrop_skips_drop_effect.rs` | FAILS — V017: identical to exp017; the attack proves nothing (SOUND) |

The chain is airtight: a destructor can take no precondition (exp015), can only
promise facts its own body proves (exp016), and its promise is never threaded
into the continuation even when the drop is guaranteed to run (exp017) — so
there is no drop-derived fact for a skipped drop to falsify (exp018). `Drop`
must also be `opens_invariants none` and `no_unwind` (verus test suite), closing
the invariant- and unwind-based variants. Conclusion: **you cannot get a drop,
run or skipped, to prove something wrong in verus.** The leak amplification the
project documents in `std` is real, but verus refuses to model the reasoning
that would make it unsound — at the cost of not modeling the leaking collection
guards (`Drain`/`PeekMut`/`IterMut`) at all (see the collection-API note below).

### Collection leak guards are unmodeled

The `std` leak-amplification sites — `Vec::drain`, `VecDeque::drain`,
`BinaryHeap::peek_mut` (RustSeal `README.md` "Leak notes on collection APIs") —
cannot be exercised in verus: the guard *types* and their methods are all
unspecified. `BinaryHeap` and `PeekMut` are entirely absent (V712); `Vec`/
`VecDeque` `drain` and their `Drain` guards are unsupported (V713 + V712);
`iter_mut`/`IterMut` likewise (below). Across `vstd/std_specs/`, the strings
`drain`, `iter_mut`, `peek_mut`, `Drain`, `IterMut`, `PeekMut` appear zero times.
So the leaking guard whose `Drop` does the write-back — the mechanism of leak
amplification — is exactly what verus does not model.

## Two verus limitations these experiments pin down

1. **No mutable iterator (`iter_mut`).** `<[T]>::iter_mut` is rejected with
   `error [V713] core::slice::<impl [T]>::iter_mut is not supported`, and
   `core::slice::IterMut` has no `IteratorSpecImpl`, so a
   `for x in v.iter_mut()` loop cannot be verified. exp003 therefore realizes
   "a mutable iterator into the vector" with the verus-supported mutable-cursor
   pair `<[T]>::first_mut` / `<[T]>::last_mut` (from `vstd::std_specs::slice`),
   whose prophetic `final(...)` ensures compose into the exp002 postcondition.
   Contrast exp001, where the **immutable** iterator `<[T]>::iter` *is* fully
   specced (ghost `it.index()` / `it.seq()`), so the `for x in it: v.iter()`
   loop verifies.

2. **No model for `mem::forget`.** `core::mem::forget` is rejected with
   `error [V713] core::mem::forget is not supported`. exp004 aborts at the
   `forget` call before its postcondition is even attempted. Supplying an
   `assume_specification` for `mem::forget` (as verus's error suggests) would be
   the way to lift this — left for a later experiment.

Both were probed against verus `0.2026.07.07.109c8e0`.
