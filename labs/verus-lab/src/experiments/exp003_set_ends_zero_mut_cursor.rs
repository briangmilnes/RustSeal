// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! exp003 — same first/last mutation as exp002, but performed through a MUTABLE
//! ITERATOR into the vector rather than by direct indexing, and prove the same
//! equality property.
//!
//! RESULT: SUCCEEDS — 1 verified, 0 errors (verus 0.2026.07.07.109c8e0)
//! DATE: 2026-0707
//!
//! The literal mutable iterator `<[T]>::iter_mut` (`core::slice::IterMut`) is
//! NOT supported by this verus build: calling `s.iter_mut()` is rejected with
//!   error [V713] `core::slice::<impl [T]>::iter_mut` is not supported
//! and `IterMut` has no `IteratorSpecImpl`, so a `for x in v.iter_mut()` loop
//! cannot be verified (there is no ghost iterator to track the yielded `&mut`s).
//! (Probed 2026-0707; the exp004 corpse and the README record the reproducer.)
//!
//! The verus-supported way to obtain mutable references INTO the vector is the
//! mutable-cursor pair `<[T]>::first_mut` and `<[T]>::last_mut` from
//! `vstd::std_specs::slice` — the two ends of the mutable iterator, each
//! returning `Option<&mut T>`. vstd specs them with prophetic final-value
//! ensures:
//!   first_mut: final(slice)@ == old(slice)@.update(0, *final(res.unwrap()))
//!   last_mut:  final(slice)@ == old(slice)@.update(len - 1, *final(res.unwrap()))
//! Writing `*f = 0` fixes the prophesied final value of that cursor to 0, so the
//! two updates compose into exactly the exp002 postcondition. `as_mut_slice`
//! borrows the whole vector as `&mut [u32]` (its view equals the vector's view).
//!
//! Work/span: Theta(1) — two constant-time cursor writes; no iteration.

use vstd::prelude::*;
use vstd::std_specs::slice::*;

verus! {

fn zero_ends(v: &mut Vec<u32>)
    requires
        old(v).len() >= 1,
    ensures
        final(v)@ == old(v)@.update(0, 0u32).update(old(v).len() - 1, 0u32),
{
    let s = v.as_mut_slice();
    if let Some(f) = s.first_mut() {
        *f = 0;
    }
    if let Some(l) = s.last_mut() {
        *l = 0;
    }
}

} // verus!

fn main() {}
