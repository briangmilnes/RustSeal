// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! exp011 — README "Explicit leak/forget APIs in std" item 7: `Box::into_raw`,
//! which gives up ownership as a raw pointer and leaks unless reclaimed with
//! `from_raw`. Does verus reject a call to it?
//!
//! RESULT: FAILS — verus REJECTS the program (does NOT verify).
//!   error [V713] `alloc::boxed::impl&%8::into_raw` is not supported
//!   (verus 0.2026.07.07.109c8e0, via validate.sh). DATE: 2026-0707.
//!
//! `Box::into_raw(b)` converts the box into a `*mut u32`, transferring ownership
//! to the caller with no destructor — leaks unless reclaimed. verus has no
//! specification for `Box::into_raw`, so it rejects the call (V713).
//! Expected-failure corpse (rule 16.3).

use vstd::prelude::*;

verus! {

fn leak(b: Box<u32>) -> *mut u32 {
    Box::into_raw(b)
}

} // verus!

fn main() {}
