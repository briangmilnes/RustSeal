// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! exp004 — a COPY of exp003 that additionally `core::mem::forget`s the mutable
//! cursor (the mutable iterator into the vector) after writing through it, to
//! see whether the equality proof still goes through.
//!
//! RESULT: FAILS — verus rejects the program (does NOT verify).
//!   error [V713] `core::mem::forget` is not supported (note: you may be able to
//!   add a Verus specification to this function with `assume_specification`)
//!   (verus 0.2026.07.07.109c8e0, via validate.sh). DATE: 2026-0707.
//!
//! This is the intended failing corpse (kept as documentation, not edited to
//! pass — CLAUDE.md rule 16.3). The proof does NOT work: verus has no model for
//! `core::mem::forget`, so it cannot reason about a mutable reference that is
//! forgotten rather than dropped. At the source level the write `*f = 0` already
//! happened before the `forget`, so the runtime vector would still be zeroed at
//! index 0 — but verus resolves a `&mut`'s prophesied final value when the borrow
//! ends, and `mem::forget` consuming the cursor is an unspecified operation
//! (V713). Verification aborts at the `forget` call before the postcondition is
//! even attempted. Supplying an `assume_specification` for `mem::forget` (as the
//! error suggests) would be the way to lift this, and is left for a later
//! experiment.
//!
//! Contrast: exp003 (identical minus the `forget`) SUCCEEDS — 1 verified,
//! 0 errors.

use vstd::prelude::*;
use vstd::std_specs::slice::*;

verus! {

fn zero_ends(v: &mut Vec<u32>)
    requires
        old(v).len() >= 1,
    ensures
        final(v)@ == old(v)@.update(0, 0u32).update(old(v).len() - 1, 0u32),
{
    let s = v.as_mut_slice();
    if let Some(f) = s.first_mut() {
        *f = 0;
        core::mem::forget(f);   // forget the mutable cursor -> V713, proof fails
    }
    if let Some(l) = s.last_mut() {
        *l = 0;
    }
}

} // verus!

fn main() {}
