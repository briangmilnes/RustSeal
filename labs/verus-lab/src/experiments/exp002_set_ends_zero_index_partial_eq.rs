// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! exp002 — mutate a `Vec<u32>` in place: set element 0 and the last element to
//! 0 by direct indexing, and prove the equality property relating the final
//! sequence to the original.
//!
//! RESULT: SUCCEEDS — 1 verified, 0 errors (verus 0.2026.07.07.109c8e0)
//! DATE: 2026-0707
//!
//! `zero_ends` takes `v: &mut Vec<u32>` and calls `v.set(0, 0)` then
//! `v.set(n - 1, 0)`. The postcondition is the exact sequence identity
//!
//!     final(v)@ == old(v)@.update(0, 0).update(old(v).len() - 1, 0)
//!
//! i.e. the final view equals the original view with position 0 overwritten by
//! 0 and then the last position overwritten by 0. `Seq::update(i, x)` is the
//! point-update spec; composing two updates is the "partial equality" property
//! for this mutation — the two vectors agree everywhere except at indices 0 and
//! len-1, where both are 0.
//!
//! Under verus's new mutable-reference model, a `&mut` parameter named in a
//! postcondition must be disambiguated: `old(v)` is the entry value, `final(v)`
//! the exit value (bare `v@` is rejected, V638). The single-element vector case
//! is admitted: when `n == 1`, index 0 and index `n-1` coincide and both writes
//! target the same slot; `update(0,0).update(0,0) == update(0,0)`, consistent
//! with the two `set` calls.
//!
//! Work/span: Theta(1) — two constant-time slot writes; no iteration.

use vstd::prelude::*;

verus! {

fn zero_ends(v: &mut Vec<u32>)
    requires
        old(v).len() >= 1,
    ensures
        final(v)@ == old(v)@.update(0, 0u32).update(old(v).len() - 1, 0u32),
{
    let n = v.len();
    v.set(0, 0);
    v.set(n - 1, 0);
}

} // verus!

fn main() {}
