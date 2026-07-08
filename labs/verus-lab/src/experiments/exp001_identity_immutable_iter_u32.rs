// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! exp001 — prove an identity function over a NON-MUTABLE iterator over `u32`.
//!
//! RESULT: SUCCEEDS — 2 verified, 0 errors (verus 0.2026.07.07.109c8e0)
//! DATE: 2026-0707
//!
//! `identity` reads every element of `v: &Vec<u32>` through the immutable slice
//! iterator `v.iter()` (item type `&u32`) and pushes each into a fresh `out`.
//! The postcondition `out@ == v@` states the function is the identity on the
//! abstract sequence view — the copy equals the original element-for-element.
//!
//! The `for x in it: v.iter()` header binds a ghost iterator `it` with:
//!   - `it.index()`      — number of elements already yielded (loop progress),
//!   - `it.seq()`        — the full `Seq<&u32>` the iterator ranges over.
//! `it.seq()` has element type `&u32`; `.unref()` maps it to `Seq<u32>` so it can
//! be compared to `v@`. vstd's `axiom_spec_slice_iter` (broadcast) supplies
//! `it.seq().unref() == v@`. Verus proves the loop invariant is maintained and,
//! at loop exit (`it.index() == v.len()`), discharges `out@ == v@`.
//!
//! Work/span: one pass, Theta(n) pushes for `n == v.len()`; span Theta(n)
//! (sequential loop). The proof adds no runtime cost — the ghost iterator and
//! invariant are erased.

use vstd::prelude::*;
use vstd::std_specs::slice::*;

verus! {

fn identity(v: &Vec<u32>) -> (out: Vec<u32>)
    ensures
        out@ == v@,
{
    let mut out: Vec<u32> = Vec::new();
    for x in it: v.iter()
        invariant
            out.len() == it.index(),
            it.seq().unref() == v@,
            forall|k: int| 0 <= k < out.len() ==> out@[k] == v@[k],
    {
        out.push(*x);
    }
    assert(out@ =~= v@);   // extensional equality closes out@ == v@
    out
}

} // verus!

fn main() {}
