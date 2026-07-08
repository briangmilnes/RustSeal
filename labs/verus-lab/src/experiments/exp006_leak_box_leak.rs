// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! exp006 — README "Explicit leak/forget APIs in std" item 2: `Box::leak`, which
//! consumes the box and returns `&'static mut T`, never freeing the allocation.
//! Does verus reject a call to it?
//!
//! RESULT: FAILS — verus REJECTS the program (does NOT verify).
//!   error [V713] `alloc::boxed::impl&%9::leak` is not supported
//!   (verus 0.2026.07.07.109c8e0, via validate.sh). DATE: 2026-0707.
//!
//! `Box::leak(b)` gives up ownership of the boxed `u32` as a `&'static mut u32`;
//! the destructor never runs, so the allocation leaks. verus has no
//! specification for `Box::leak`, so it rejects the call (V713). Expected-failure
//! corpse (rule 16.3).

use vstd::prelude::*;

verus! {

fn leak(b: Box<u32>) -> &'static mut u32 {
    Box::leak(b)
}

} // verus!

fn main() {}
