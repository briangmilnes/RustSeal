// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! exp010 — README "Explicit leak/forget APIs in std" item 6:
//! `CString::into_raw`, which gives up ownership as a raw pointer and leaks
//! unless reclaimed with `from_raw`. Does verus reject a call to it?
//!
//! RESULT: FAILS — verus REJECTS the program (does NOT verify).
//!   error [V713] `alloc::ffi::c_str::impl&%1::into_raw` is not supported
//!   error [V712] `alloc::ffi::c_str::CString` is not supported
//!   (verus 0.2026.07.07.109c8e0, via validate.sh). DATE: 2026-0707.
//!
//! `c.into_raw()` converts the `CString` into a `*mut c_char`, transferring
//! ownership to the caller with no destructor — leaks unless reclaimed. verus
//! rejects on two counts: it models neither the `CString` type (V712) nor the
//! `into_raw` method (V713). Expected-failure corpse (rule 16.3).

use vstd::prelude::*;
use std::ffi::CString;

verus! {

fn leak(c: CString) -> *mut core::ffi::c_char {
    c.into_raw()
}

} // verus!

fn main() {}
