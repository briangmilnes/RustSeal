// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! exp005 — README "Explicit leak/forget APIs in std" item 1: `std::mem::forget`,
//! the canonical safe-code leak primitive. Does verus reject a call to it?
//!
//! RESULT: FAILS — verus REJECTS the program (does NOT verify).
//!   error [V713] `core::mem::forget` is not supported
//!   (verus 0.2026.07.07.109c8e0, via validate.sh). DATE: 2026-0707.
//!
//! `leak` moves the `Vec<u32>` into `core::mem::forget`, which runs no
//! destructor — the heap buffer is never freed. verus has no specification for
//! `mem::forget`, so it rejects the call outright (V713); the leak cannot be
//! expressed in verified exec code without an `assume_specification`. This is
//! the minimal, standalone counterpart to exp004 (which forgets a mutable
//! cursor inside the mutation arc). Expected-failure corpse (rule 16.3).

use vstd::prelude::*;

verus! {

fn leak(x: Vec<u32>) {
    core::mem::forget(x);
}

} // verus!

fn main() {}
