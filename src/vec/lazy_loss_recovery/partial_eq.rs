// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.
use super::VVec;
use std::alloc::Allocator;

// The stability-attribute plumbing (`, #[$stability:meta]` matcher arm and the
// `#[$stability]` it emitted) is dropped here: stability attributes may not be used
// outside the standard library (E0734), so the extraction carries none.
macro_rules! __impl_slice_eq1 {
    ([$($vars:tt)*] $lhs:ty, $rhs:ty $(where $ty:ty: $bound:ident)?) => {
        impl<T, U, $($vars)*> PartialEq<$rhs> for $lhs
        where
            T: PartialEq<U>,
            $($ty: $bound)?
        {
            #[inline]
            fn eq(&self, other: &$rhs) -> bool { self[..] == other[..] }
            #[inline]
            fn ne(&self, other: &$rhs) -> bool { self[..] != other[..] }
        }
    }
}

// Kept: `Self` is the local `VVec`, so these satisfy the orphan rule.
__impl_slice_eq1! { [A1: Allocator, A2: Allocator] VVec<T, A1>, VVec<U, A2> }
__impl_slice_eq1! { [A: Allocator] VVec<T, A>, &[U] }
__impl_slice_eq1! { [A: Allocator] VVec<T, A>, &mut [U] }
__impl_slice_eq1! { [A: Allocator] VVec<T, A>, [U] }
__impl_slice_eq1! { [A: Allocator, const N: usize] VVec<T, A>, [U; N] }
__impl_slice_eq1! { [A: Allocator, const N: usize] VVec<T, A>, &[U; N] }

// CORPSE (ProcessCommentingStandard): these reverse-direction and `Cow` impls have a
// FOREIGN `Self` (`&[T]`, `[T]`, `std::borrow::Cow`) with the local `VVec` only in the
// trait's type argument. In `alloc` `Vec` and `Cow` are local so these are legal; here
// they violate the orphan rule (E0117 — `impl ForeignTrait for ForeignType`), so they are
// dropped. The forward direction (`VVec == &[U]`, etc.) above still gives the comparison.
// __impl_slice_eq1! { [A: Allocator] &[T], VVec<U, A> }
// __impl_slice_eq1! { [A: Allocator] &mut [T], VVec<U, A> }
// __impl_slice_eq1! { [A: Allocator] [T], VVec<U, A> }
// __impl_slice_eq1! { [A: Allocator] Cow<'_, [T]>, VVec<U, A> where T: Clone }
// __impl_slice_eq1! { [] Cow<'_, [T]>, &[U] where T: Clone }
// __impl_slice_eq1! { [] Cow<'_, [T]>, &mut [U] where T: Clone }

// NOTE: some less important impls are omitted to reduce code bloat
// FIXME(Centril): Reconsider this?
//__impl_slice_eq1! { [const N: usize] VVec<A>, &mut [B; N], }
//__impl_slice_eq1! { [const N: usize] [A; N], VVec<B>, }
//__impl_slice_eq1! { [const N: usize] &[A; N], VVec<B>, }
//__impl_slice_eq1! { [const N: usize] &mut [A; N], VVec<B>, }
//__impl_slice_eq1! { [const N: usize] Cow<'a, [A]>, [B; N], }
//__impl_slice_eq1! { [const N: usize] Cow<'a, [A]>, &[B; N], }
//__impl_slice_eq1! { [const N: usize] Cow<'a, [A]>, &mut [B; N], }
