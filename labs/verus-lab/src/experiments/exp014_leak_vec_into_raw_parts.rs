// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! exp014 — README "Explicit leak/forget APIs in std" item 10:
//! `Vec::into_raw_parts` (unstable), which decomposes the vec into raw parts and
//! leaks unless reclaimed with `from_raw_parts`. Does verus reject a call to it?
//!
//! RESULT: FAILS — verus REJECTS the program (does NOT verify).
//!   error [V713] `alloc::vec::impl&%0::into_raw_parts` is not supported
//!   (verus 0.2026.07.07.109c8e0, via validate.sh). DATE: 2026-0707.
//!
//! `v.into_raw_parts()` returns `(*mut u32, usize, usize)` (pointer, length,
//! capacity), transferring ownership of the buffer to the caller with no
//! destructor — leaks unless reclaimed. The `#![feature(vec_into_raw_parts)]`
//! gate is accepted by verus's nightly toolchain, but verus has no specification
//! for `into_raw_parts`, so it rejects the call (V713). Expected-failure corpse
//! (rule 16.3).

#![feature(vec_into_raw_parts)]

use vstd::prelude::*;

verus! {

fn leak(v: Vec<u32>) -> (*mut u32, usize, usize) {
    v.into_raw_parts()
}

} // verus!

fn main() {}
