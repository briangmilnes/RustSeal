// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! exp016 — Can an `impl Drop` state a FALSE `ensures`? If a destructor could
//! promise `false` (or any unproven fact), and verus assumed that promise in the
//! continuation after the value's scope, a proof could derive anything.
//!
//! RESULT: FAILS — verus REJECTS the program (does NOT verify).
//!   error: [V003] postcondition not satisfied
//!   (verus 0.2026.07.07.109c8e0, via validate.sh). DATE: 2026-0708.
//!
//! verus DOES allow an `ensures` on `Drop` (unlike `requires`, exp015), but it
//! verifies the drop BODY against that postcondition like any other function —
//! the empty body cannot establish `false`, so verification fails (V003). This
//! is the second designed defense: a destructor can only ever promise facts its
//! own body proves, so whatever verus assumes downstream of a drop is a TRUE
//! fact, never a fabricated one. Expected-failure corpse (rule 5).

use vstd::prelude::*;

verus! {

struct Bomb { v: u64 }

impl Drop for Bomb {
    fn drop(&mut self)
        ensures false
        opens_invariants none
        no_unwind
    { }
}

} // verus!

fn main() {}
