// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.
use super::VVec;
use std::alloc::Allocator;

// Specialization trait used for VVec::from_elem
pub(super) trait SpecFromElem: Sized {
    fn from_elem<A: Allocator>(elem: Self, n: usize, alloc: A) -> VVec<Self, A>;
}

impl<T: Clone> SpecFromElem for T {
    default fn from_elem<A: Allocator>(elem: Self, n: usize, alloc: A) -> VVec<Self, A> {
        let mut v = VVec::with_capacity_in(n, alloc);
        v.extend_with(n, elem);
        v
    }
}

// CORPSE (ProcessCommentingStandard): the `IsZero` and `i8`/`u8` specializations of
// `from_elem` take the zeroed-allocation fast path
// (`VRawVec::with_capacity_zeroed_in`), which the `VRawVec`-over-`std::Vec` shim cannot
// provide — public `Vec` has no zeroed-allocation constructor, and `IsZero` lives in the
// corpse `is_zero` module. These element types fall through to the generic `T: Clone`
// impl above (correct, just without the memset/zeroed-alloc optimization).
// impl<T: Clone + IsZero> SpecFromElem for T {
//     #[inline]
//     default fn from_elem<A: Allocator>(elem: T, n: usize, alloc: A) -> VVec<T, A> {
//         if elem.is_zero() {
//             return VVec { buf: VRawVec::with_capacity_zeroed_in(n, alloc), len: n };
//         }
//         let mut v = VVec::with_capacity_in(n, alloc);
//         v.extend_with(n, elem);
//         v
//     }
// }
//
// impl SpecFromElem for i8 {
//     #[inline]
//     fn from_elem<A: Allocator>(elem: i8, n: usize, alloc: A) -> VVec<i8, A> {
//         if elem == 0 {
//             return VVec { buf: VRawVec::with_capacity_zeroed_in(n, alloc), len: n };
//         }
//         let mut v = VVec::with_capacity_in(n, alloc);
//         unsafe {
//             ptr::write_bytes(v.as_mut_ptr(), elem as u8, n);
//             v.set_len(n);
//         }
//         v
//     }
// }
//
// impl SpecFromElem for u8 {
//     #[inline]
//     fn from_elem<A: Allocator>(elem: u8, n: usize, alloc: A) -> VVec<u8, A> {
//         if elem == 0 {
//             return VVec { buf: VRawVec::with_capacity_zeroed_in(n, alloc), len: n };
//         }
//         let mut v = VVec::with_capacity_in(n, alloc);
//         unsafe {
//             ptr::write_bytes(v.as_mut_ptr(), elem, n);
//             v.set_len(n);
//         }
//         v
//     }
// }

// A better way would be to implement this for all ZSTs which are `Copy` and have trivial `Clone`
// but the latter cannot be detected currently
impl SpecFromElem for () {
    #[inline]
    fn from_elem<A: Allocator>(_elem: (), n: usize, alloc: A) -> VVec<(), A> {
        let mut v = VVec::with_capacity_in(n, alloc);
        // SAFETY: the capacity has just been set to `n`
        // and `()` is a ZST with trivial `Clone` implementation
        unsafe {
            v.set_len(n);
        }
        v
    }
}
