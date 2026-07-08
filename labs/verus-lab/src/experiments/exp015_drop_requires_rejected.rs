// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! exp015 — Can an `impl Drop` carry a `requires`? If it could, a proof could
//! demand that a precondition hold at drop time and rely on the destructor to
//! discharge an obligation — the RFC-1066 hazard moved into the proof world,
//! because Rust does NOT guarantee a destructor runs.
//!
//! RESULT: FAILS — verus REJECTS the program (does NOT verify).
//!   error: requires are not allowed on the implementation for Drop
//!   (verus 0.2026.07.07.109c8e0, via validate.sh). DATE: 2026-0708.
//!
//! This is verus's first designed defense against drop-based unsoundness: a
//! `Drop::drop` may not state a `requires`, so no caller-established
//! precondition can be threaded into the destructor. Expected-failure corpse
//! (rule 5 — leave the corpse).

use vstd::prelude::*;

verus! {

struct A { v: u64 }

impl Drop for A {
    fn drop(&mut self)
        requires self.v == 0
        opens_invariants none
        no_unwind
    { }
}

} // verus!

fn main() {}
