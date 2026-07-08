// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! exp009 — README "Explicit leak/forget APIs in std" item 5:
//! `std::mem::ManuallyDrop`, the type-level counterpart to `forget` — it
//! suppresses the wrapped value's destructor. Does verus reject it?
//!
//! RESULT: SUCCEEDS — 1 verified, 0 errors (verus 0.2026.07.07.109c8e0, via
//!   validate.sh). DATE: 2026-0707.
//!
//! This is the ONE leak API in the README list that verus does NOT reject.
//! `ManuallyDrop::new(x)` wraps the `Vec<u32>`; when `_m` goes out of scope its
//! `Drop` is a no-op, so the inner vector's destructor never runs and its heap
//! buffer leaks — yet verus accepts the function as verified. Unlike the free
//! functions `mem::forget` / `Box::leak` / `*::into_raw` (each rejected V713 for
//! lack of a specification), `ManuallyDrop::new` is an ordinary constructor of a
//! transparent wrapper that verus models directly, so the leak is expressible in
//! verified exec code.
//!
//! This is NOT the intended outcome — the goal was rejection — but it is the
//! honest result, kept as a corpse (rule 16.3) and a NOT-A-LIMITATION finding
//! (rule 16.4): verified-safe Rust CAN leak via `ManuallyDrop`. See the README
//! "Two verus findings" note.

use vstd::prelude::*;
use core::mem::ManuallyDrop;

verus! {

fn leak(x: Vec<u32>) {
    let _m = ManuallyDrop::new(x);   // inner Vec destructor suppressed -> leaks
}

} // verus!

fn main() {}
