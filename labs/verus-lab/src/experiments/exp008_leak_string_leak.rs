// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! exp008 — README "Explicit leak/forget APIs in std" item 4: `String::leak`,
//! which consumes the string and returns `&'static mut str`, never freeing the
//! buffer. Does verus reject a call to it?
//!
//! RESULT: FAILS — verus REJECTS the program (does NOT verify).
//!   error [V713] `alloc::string::impl&%0::leak` is not supported
//!   (verus 0.2026.07.07.109c8e0, via validate.sh). DATE: 2026-0707.
//!
//! `s.leak()` gives up ownership of the `String` buffer as a `&'static mut str`;
//! the destructor never runs, so the buffer leaks. verus has no specification
//! for `String::leak`, so it rejects the call (V713). Expected-failure corpse
//! (rule 16.3).

use vstd::prelude::*;

verus! {

fn leak(s: String) -> &'static mut str {
    s.leak()
}

} // verus!

fn main() {}
