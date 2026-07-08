// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! exp013 — README "Explicit leak/forget APIs in std" item 9: `Arc::into_raw`,
//! which gives up ownership as a raw pointer WITHOUT decrementing the strong
//! count, leaking unless reclaimed. Does verus reject a call to it?
//!
//! RESULT: FAILS — verus REJECTS the program (does NOT verify).
//!   error [V713] `alloc::sync::impl&%24::into_raw` is not supported
//!   (verus 0.2026.07.07.109c8e0, via validate.sh). DATE: 2026-0707.
//!
//! `Arc::into_raw(a)` yields a `*const u32` while leaving the strong count
//! untouched, so the referent is never freed unless reclaimed via `from_raw`.
//! verus has no specification for `Arc::into_raw`, so it rejects the call (V713).
//! Expected-failure corpse (rule 16.3).

use vstd::prelude::*;
use std::sync::Arc;

verus! {

fn leak(a: Arc<u32>) -> *const u32 {
    Arc::into_raw(a)
}

} // verus!

fn main() {}
