// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! exp018 — THE ATTACK. A copy of exp017 whose only change is wrapping the guard
//! in `ManuallyDrop`, so its destructor is SKIPPED — at runtime `x` stays 5, not
//! 0. The question: does verus still prove `x == 0`? If it does, a skipped drop
//! has proven something FALSE — a soundness bug. If verus refuses, it correctly
//! knows `ManuallyDrop` suppresses the destructor and withholds its `ensures`.
//!
//! RESULT: FAILS — verus does NOT prove `x == 0` (does NOT verify). SOUND.
//!   verification results: 1 verified, 1 errors
//!   error: [V017] assertion failed  (at `assert(x == 0)`)
//!   (verus 0.2026.07.07.109c8e0, via validate.sh). DATE: 2026-0708.
//!
//! The attack does NOT succeed: verus refuses to prove the (runtime-false)
//! `x == 0`, exactly as in exp017. `exp009` showed `ManuallyDrop::new` is the one
//! README leak API verus accepts, but this experiment shows that acceptance opens
//! NO proof hole — the downstream result is identical to exp017 because verus
//! never had a drop-derived fact about `x` to begin with (it does not thread a
//! destructor's `ensures` into the continuation, run or skipped). This corpse is
//! the evidence that verus is sound against skipped-destructor reasoning
//! (rule 5 — leave the corpse).

use vstd::prelude::*;
use core::mem::ManuallyDrop;

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
        let _m = ManuallyDrop::new(Zeroer { r: &mut x });  // destructor SKIPPED
    }
    assert(x == 0);   // FALSE at runtime — verus must NOT prove it
}

} // verus!

fn main() {}
