// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! exp007 — README "Explicit leak/forget APIs in std" item 3: `Vec::leak`, which
//! consumes the vec and returns `&'static mut [T]`, never freeing the buffer.
//! Does verus reject a call to it?
//!
//! RESULT: FAILS — verus REJECTS the program (does NOT verify).
//!   error [V713] `alloc::vec::impl&%1::leak` is not supported
//!   (verus 0.2026.07.07.109c8e0, via validate.sh). DATE: 2026-0707.
//!
//! `v.leak()` gives up ownership of the `Vec<u32>` buffer as a
//! `&'static mut [u32]`; the destructor never runs, so the buffer leaks. verus
//! has no specification for `Vec::leak`, so it rejects the call (V713).
//! Expected-failure corpse (rule 16.3).

use vstd::prelude::*;

verus! {

fn leak(v: Vec<u32>) -> &'static mut [u32] {
    v.leak()
}

} // verus!

fn main() {}
