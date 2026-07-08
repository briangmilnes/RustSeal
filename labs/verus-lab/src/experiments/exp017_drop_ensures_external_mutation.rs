// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! exp017 — SETUP for the drop-soundness attack. A guard `Zeroer` holds a
//! `&mut u64`; its destructor writes 0 through it and `ensures` the pointee is 0.
//! After the guard leaves scope (implicit drop) the borrow ends and the caller
//! reads the integer. Does verus assume the destructor ran and prove `x == 0`?
//!
//! RESULT: FAILS — verus does NOT prove `x == 0` (does NOT verify).
//!   verification results: 1 verified, 1 errors
//!   error: [V017] assertion failed  (at `assert(x == 0)`)
//!   (verus 0.2026.07.07.109c8e0, via validate.sh). DATE: 2026-0708.
//!
//! The drop BODY verifies (the `1 verified` — verus proves `*final(self).r == 0`
//! locally), but the post-scope `assert(x == 0)` FAILS. Even though an implicit
//! scope-end drop is GUARANTEED by Rust to run, verus does not thread the
//! destructor's `ensures` about the external `&mut` back onto `x` in the
//! continuation — there is simply no drop-derived fact available to the caller.
//! This is the third designed defense: a proof cannot depend on a destructor's
//! effect at all, so nothing is left for a skipped drop (exp018) to falsify.
//! Expected-failure corpse (rule 5 — leave the corpse).

use vstd::prelude::*;

verus! {

pub struct Zeroer<'a> { pub r: &'a mut u64 }

impl<'a> Drop for Zeroer<'a> {
    fn drop(&mut self)
        ensures *final(self).r == 0
        opens_invariants none
        no_unwind
    {
        *self.r = 0;
    }
}

fn go() {
    let mut x: u64 = 5;
    {
        let _z = Zeroer { r: &mut x };   // dropped at block end -> writes x = 0
    }
    assert(x == 0);                       // relies on the destructor having run
}

} // verus!

fn main() {}
