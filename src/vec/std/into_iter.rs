// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.
use core::iter::{FusedIterator, TrustedFused, TrustedLen, TrustedRandomAccessNoCoerce};
use core::marker::PhantomData;
use core::mem::{ManuallyDrop, MaybeUninit, SizedTypeProperties};
use core::num::NonZero;
use core::ops::Deref;
use core::panic::UnwindSafe;
use core::ptr::{self, NonNull};
use core::{array, fmt, slice};

use super::{VRawVec, VVec};
use std::alloc::{Allocator, Global};

macro non_null {
    (mut $place:expr, $t:ident) => {{
        #![allow(unused_unsafe)] // we're sometimes used within an unsafe block
        unsafe { &mut *((&raw mut $place) as *mut NonNull<$t>) }
    }},
    ($place:expr, $t:ident) => {{
        #![allow(unused_unsafe)] // we're sometimes used within an unsafe block
        unsafe { *((&raw const $place) as *const NonNull<$t>) }
    }},
}

/// An iterator that moves out of a vector.
///
/// This `struct` is created by the `into_iter` method on [`VVec`](super::VVec)
/// (provided by the [`IntoIterator`] trait).
///
/// # Example
///

pub struct VVecIntoIter<
    T,
    A: Allocator = Global,
> {
    pub(super) buf: NonNull<T>,
    pub(super) phantom: PhantomData<T>,
    pub(super) cap: usize,
    // the drop impl reconstructs a VRawVec from buf, cap and alloc
    // to avoid dropping the allocator twice we need to wrap it into ManuallyDrop
    pub(super) alloc: ManuallyDrop<A>,
    pub(super) ptr: NonNull<T>,
    /// If T is a ZST, this is actually ptr+len. This encoding is picked so that
    /// ptr == end is a quick test for the Iterator being empty, that works
    /// for both ZST and non-ZST.
    /// For non-ZSTs the pointer is treated as `NonNull<T>`
    pub(super) end: *const T,
}

// Manually mirroring what `VVec` has,
// because otherwise we get `T: RefUnwindSafe` from `NonNull`.

impl<T: UnwindSafe, A: Allocator + UnwindSafe> UnwindSafe for VVecIntoIter<T, A> {}

impl<T: fmt::Debug, A: Allocator> fmt::Debug for VVecIntoIter<T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("VVecIntoIter").field(&self.as_slice()).finish()
    }
}

impl<T, A: Allocator> VVecIntoIter<T, A> {
    /// Returns the remaining items of this iterator as a slice.
    ///
    /// # Examples
    ///

    pub fn as_slice(&self) -> &[T] {
        unsafe { slice::from_raw_parts(self.ptr.as_ptr(), self.len()) }
    }

    /// Returns the remaining items of this iterator as a mutable slice.
    ///
    /// # Examples
    ///

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        unsafe { &mut *self.as_raw_mut_slice() }
    }

    /// Returns a reference to the underlying allocator.

    #[inline]
    pub fn allocator(&self) -> &A {
        &self.alloc
    }

    fn as_raw_mut_slice(&mut self) -> *mut [T] {
        ptr::slice_from_raw_parts_mut(self.ptr.as_ptr(), self.len())
    }

    /// Drops remaining elements and relinquishes the backing allocation.
    ///
    /// This method guarantees it won't panic before relinquishing the backing
    /// allocation.
    ///
    /// This is roughly equivalent to the following, but more efficient
    ///
    ///
    /// This method is used by in-place iteration, refer to the vec::in_place_collect
    /// documentation for an overview.
    // Faithful extraction surface: the only caller is the corpsed `in_place_collect`
    // module, so this is dead here; kept to preserve `VVecIntoIter`'s upstream API.
    #[allow(dead_code)]
    pub(super) fn forget_allocation_drop_remaining(&mut self) {
        let remaining = self.as_raw_mut_slice();

        // overwrite the individual fields instead of creating a new
        // struct and then overwriting &mut self.
        // this creates less assembly
        self.cap = 0;
        self.buf = VRawVec::new().non_null();
        self.ptr = self.buf;
        self.end = self.buf.as_ptr();

        // Dropping the remaining elements can panic, so this needs to be
        // done only after updating the other fields.
        unsafe {
            ptr::drop_in_place(remaining);
        }
    }

    /// Forgets to Drop the remaining elements while still allowing the backing allocation to be freed.
    ///
    /// This method does not consume `self`, and leaves deallocation to `impl Drop for VVecIntoIter`.
    /// If consuming `self` is possible, consider calling
    /// [`Self::forget_remaining_elements_and_dealloc()`] instead.
    pub(crate) fn forget_remaining_elements(&mut self) {
        // For the ZST case, it is crucial that we mutate `end` here, not `ptr`.
        // `ptr` must stay aligned, while `end` may be unaligned.
        self.end = self.ptr.as_ptr();
    }

    /// Forgets to Drop the remaining elements and frees the backing allocation.
    /// Consuming version of [`Self::forget_remaining_elements()`].
    ///
    /// This can be used in place of `drop(self)` when `self` is known to be exhausted,
    /// to avoid producing a needless `drop_in_place::<[T]>()`.
    #[inline]
    pub(crate) fn forget_remaining_elements_and_dealloc(self) {
        let mut this = ManuallyDrop::new(self);
        // SAFETY: `this` is in ManuallyDrop, so it will not be double-freed.
        unsafe {
            this.dealloc_only();
        }
    }

    /// Frees the allocation, without checking or dropping anything else.
    ///
    /// The safe version of this method is [`Self::forget_remaining_elements_and_dealloc()`].
    /// This function exists only to share code between that method and the `impl Drop`.
    ///
    /// # Safety
    ///
    /// This function must only be called with an [`VVecIntoIter`] that is not going to be dropped
    /// or otherwise used in any way, either because it is being forgotten or because its `Drop`
    /// is already executing; otherwise a double-free will occur, and possibly a read from freed
    /// memory if there are any remaining elements.
    #[inline]
    unsafe fn dealloc_only(&mut self) {
        unsafe {
            // SAFETY: our caller promises not to touch `*self` again
            let alloc = ManuallyDrop::take(&mut self.alloc);
            // VRawVec handles deallocation
            let _ = VRawVec::from_nonnull_in(self.buf, self.cap, alloc);
        }
    }

    // CORPSE (ProcessCommentingStandard): `into_vecdeque` is the zero-copy bridge that
    // hands a `VVecIntoIter`'s allocation to a `VecDeque` via
    // `VecDeque::from_contiguous_raw_parts_in` — an alloc-private constructor not exposed
    // on the public `std::collections::VecDeque`. The only caller was the corpse
    // `spec_from_iter` (the in-place specialization). Dropped.
    // #[inline]
    // pub(crate) fn into_vecdeque(self) -> VecDeque<T, A> {
    //     // Keep our `Drop` impl from dropping the elements and the allocator
    //     let mut this = ManuallyDrop::new(self);
    //
    //     // SAFETY: This allocation originally came from a `VVec`, so it passes
    //     // all those checks. We have `this.buf` ≤ `this.ptr` ≤ `this.end`,
    //     // so the `offset_from_unsigned`s below cannot wrap, and will produce a well-formed
    //     // range. `end` ≤ `buf + cap`, so the range will be in-bounds.
    //     // Taking `alloc` is ok because nothing else is going to look at it,
    //     // since our `Drop` impl isn't going to run so there's no more code.
    //     unsafe {
    //         let buf = this.buf.as_ptr();
    //         let initialized = if T::IS_ZST {
    //             // All the pointers are the same for ZSTs, so it's fine to
    //             // say that they're all at the beginning of the "allocation".
    //             0..this.len()
    //         } else {
    //             this.ptr.offset_from_unsigned(this.buf)..this.end.offset_from_unsigned(buf)
    //         };
    //         let cap = this.cap;
    //         let alloc = ManuallyDrop::take(&mut this.alloc);
    //         VecDeque::from_contiguous_raw_parts_in(buf, initialized, cap, alloc)
    //     }
    // }
}

impl<T, A: Allocator> AsRef<[T]> for VVecIntoIter<T, A> {
    fn as_ref(&self) -> &[T] {
        self.as_slice()
    }
}

unsafe impl<T: Send, A: Allocator + Send> Send for VVecIntoIter<T, A> {}

unsafe impl<T: Sync, A: Allocator + Sync> Sync for VVecIntoIter<T, A> {}

impl<T, A: Allocator> Iterator for VVecIntoIter<T, A> {
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<T> {
        let ptr = if T::IS_ZST {
            if self.ptr.as_ptr() == self.end as *mut T {
                return None;
            }
            // `ptr` has to stay where it is to remain aligned, so we reduce the length by 1 by
            // reducing the `end`.
            self.end = self.end.wrapping_byte_sub(1);
            self.ptr
        } else {
            if self.ptr == non_null!(self.end, T) {
                return None;
            }
            let old = self.ptr;
            self.ptr = unsafe { old.add(1) };
            old
        };
        Some(unsafe { ptr.read() })
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let exact = if T::IS_ZST {
            self.end.addr().wrapping_sub(self.ptr.as_ptr().addr())
        } else {
            unsafe { non_null!(self.end, T).offset_from_unsigned(self.ptr) }
        };
        (exact, Some(exact))
    }

    #[inline]
    fn advance_by(&mut self, n: usize) -> Result<(), NonZero<usize>> {
        let step_size = self.len().min(n);
        let to_drop = ptr::slice_from_raw_parts_mut(self.ptr.as_ptr(), step_size);
        if T::IS_ZST {
            // See `next` for why we sub `end` here.
            self.end = self.end.wrapping_byte_sub(step_size);
        } else {
            // SAFETY: the min() above ensures that step_size is in bounds
            self.ptr = unsafe { self.ptr.add(step_size) };
        }
        // SAFETY: the min() above ensures that step_size is in bounds
        unsafe {
            ptr::drop_in_place(to_drop);
        }
        NonZero::new(n - step_size).map_or(Ok(()), Err)
    }

    #[inline]
    fn count(self) -> usize {
        self.len()
    }

    #[inline]
    fn last(mut self) -> Option<T> {
        self.next_back()
    }

    #[inline]
    fn next_chunk<const N: usize>(&mut self) -> Result<[T; N], core::array::IntoIter<T, N>> {
        let mut raw_ary = [const { MaybeUninit::uninit() }; N];

        let len = self.len();

        if T::IS_ZST {
            if len < N {
                self.forget_remaining_elements();
                // Safety: ZSTs can be conjured ex nihilo, only the amount has to be correct
                return Err(unsafe { array::IntoIter::new_unchecked(raw_ary, 0..len) });
            }

            self.end = self.end.wrapping_byte_sub(N);
            // Safety: ditto
            return Ok(unsafe { raw_ary.transpose().assume_init() });
        }

        if len < N {
            // Safety: `len` indicates that this many elements are available and we just checked that
            // it fits into the array.
            unsafe {
                ptr::copy_nonoverlapping(self.ptr.as_ptr(), raw_ary.as_mut_ptr() as *mut T, len);
                self.forget_remaining_elements();
                return Err(array::IntoIter::new_unchecked(raw_ary, 0..len));
            }
        }

        // Safety: `len` is larger than the array size. Copy a fixed amount here to fully initialize
        // the array.
        unsafe {
            ptr::copy_nonoverlapping(self.ptr.as_ptr(), raw_ary.as_mut_ptr() as *mut T, N);
            self.ptr = self.ptr.add(N);
            Ok(raw_ary.transpose().assume_init())
        }
    }

    fn fold<B, F>(mut self, mut accum: B, mut f: F) -> B
    where
        F: FnMut(B, Self::Item) -> B,
    {
        if T::IS_ZST {
            while self.ptr.as_ptr() != self.end.cast_mut() {
                // SAFETY: we just checked that `self.ptr` is in bounds.
                let tmp = unsafe { self.ptr.read() };
                // See `next` for why we subtract from `end` here.
                self.end = self.end.wrapping_byte_sub(1);
                accum = f(accum, tmp);
            }
        } else {
            // SAFETY: `self.end` can only be null if `T` is a ZST.
            while self.ptr != non_null!(self.end, T) {
                // SAFETY: we just checked that `self.ptr` is in bounds.
                let tmp = unsafe { self.ptr.read() };
                // SAFETY: the maximum this can be is `self.end`.
                // Increment `self.ptr` first to avoid double dropping in the event of a panic.
                self.ptr = unsafe { self.ptr.add(1) };
                accum = f(accum, tmp);
            }
        }

        // There are in fact no remaining elements to forget, but by doing this we can avoid
        // potentially generating a needless loop to drop the elements that cannot exist at
        // this point.
        self.forget_remaining_elements_and_dealloc();

        accum
    }

    fn try_fold<B, F, R>(&mut self, mut accum: B, mut f: F) -> R
    where
        Self: Sized,
        F: FnMut(B, Self::Item) -> R,
        R: core::ops::Try<Output = B>,
    {
        if T::IS_ZST {
            while self.ptr.as_ptr() != self.end.cast_mut() {
                // SAFETY: we just checked that `self.ptr` is in bounds.
                let tmp = unsafe { self.ptr.read() };
                // See `next` for why we subtract from `end` here.
                self.end = self.end.wrapping_byte_sub(1);
                accum = f(accum, tmp)?;
            }
        } else {
            // SAFETY: `self.end` can only be null if `T` is a ZST.
            while self.ptr != non_null!(self.end, T) {
                // SAFETY: we just checked that `self.ptr` is in bounds.
                let tmp = unsafe { self.ptr.read() };
                // SAFETY: the maximum this can be is `self.end`.
                // Increment `self.ptr` first to avoid double dropping in the event of a panic.
                self.ptr = unsafe { self.ptr.add(1) };
                accum = f(accum, tmp)?;
            }
        }
        R::from_output(accum)
    }

    unsafe fn __iterator_get_unchecked(&mut self, i: usize) -> Self::Item
    where
        Self: TrustedRandomAccessNoCoerce,
    {
        // SAFETY: the caller must guarantee that `i` is in bounds of the
        // `VVec<T>`, so `i` cannot overflow an `isize`, and the `self.ptr.add(i)`
        // is guaranteed to pointer to an element of the `VVec<T>` and
        // thus guaranteed to be valid to dereference.
        //
        // Also note the implementation of `Self: TrustedRandomAccess` requires
        // that `T: Copy` so reading elements from the buffer doesn't invalidate
        // them for `Drop`.
        unsafe { self.ptr.add(i).read() }
    }
}

impl<T, A: Allocator> DoubleEndedIterator for VVecIntoIter<T, A> {
    #[inline]
    fn next_back(&mut self) -> Option<T> {
        if T::IS_ZST {
            if self.ptr.as_ptr() == self.end as *mut _ {
                return None;
            }
            // See above for why 'ptr.offset' isn't used
            self.end = self.end.wrapping_byte_sub(1);
            // Note that even though this is next_back() we're reading from `self.ptr`, not
            // `self.end`. We track our length using the byte offset from `self.ptr` to `self.end`,
            // so the end pointer may not be suitably aligned for T.
            Some(unsafe { ptr::read(self.ptr.as_ptr()) })
        } else {
            if self.ptr == non_null!(self.end, T) {
                return None;
            }
            unsafe {
                self.end = self.end.sub(1);
                Some(ptr::read(self.end))
            }
        }
    }

    #[inline]
    fn advance_back_by(&mut self, n: usize) -> Result<(), NonZero<usize>> {
        let step_size = self.len().min(n);
        if T::IS_ZST {
            // SAFETY: same as for advance_by()
            self.end = self.end.wrapping_byte_sub(step_size);
        } else {
            // SAFETY: same as for advance_by()
            self.end = unsafe { self.end.sub(step_size) };
        }
        let to_drop = if T::IS_ZST {
            // ZST may cause unalignment
            ptr::slice_from_raw_parts_mut(ptr::NonNull::<T>::dangling().as_ptr(), step_size)
        } else {
            ptr::slice_from_raw_parts_mut(self.end as *mut T, step_size)
        };
        // SAFETY: same as for advance_by()
        unsafe {
            ptr::drop_in_place(to_drop);
        }
        NonZero::new(n - step_size).map_or(Ok(()), Err)
    }
}

impl<T, A: Allocator> ExactSizeIterator for VVecIntoIter<T, A> {
    fn is_empty(&self) -> bool {
        if T::IS_ZST {
            self.ptr.as_ptr() == self.end as *mut _
        } else {
            self.ptr == non_null!(self.end, T)
        }
    }
}

impl<T, A: Allocator> FusedIterator for VVecIntoIter<T, A> {}

#[doc(hidden)]

unsafe impl<T, A: Allocator> TrustedFused for VVecIntoIter<T, A> {}

unsafe impl<T, A: Allocator> TrustedLen for VVecIntoIter<T, A> {}

impl<T, A> Default for VVecIntoIter<T, A>
where
    A: Allocator + Default,
{
    /// Creates an empty `vec::IntoIter`.
    ///
    fn default() -> Self {
        super::VVec::new_in(Default::default()).into_iter()
    }
}

// CORPSE (ProcessCommentingStandard): the `NonDrop` marker trait exists only to bound the
// `TrustedRandomAccessNoCoerce` impl below, which is itself a corpse (it cannot be
// expressed without alloc-internal specialization markers). With that impl gone, the trait
// has no users.
// #[doc(hidden)]
// pub trait NonDrop {}
// // T: Copy as approximation for !Drop since get_unchecked does not advance self.ptr
// // and thus we can't implement drop-handling
// impl<T: Copy> NonDrop for T {}

// CORPSE (ProcessCommentingStandard): this `TrustedRandomAccessNoCoerce` impl is bounded
// on `T: NonDrop`, where `NonDrop` is an alloc-internal specialization marker trait
// (`#[rustc_unsafe_specialization_marker]`) that cannot be named or specialized on outside
// `alloc` ("cannot specialize on trait `NonDrop`"). It is a TrustedRandomAccess
// optimization for in-place iteration; dropped.
// #[doc(hidden)]
// // TrustedRandomAccess (without NoCoerce) must not be implemented because
// // subtypes/supertypes of `T` might not be `NonDrop`
// unsafe impl<T, A: Allocator> TrustedRandomAccessNoCoerce for VVecIntoIter<T, A>
// where
//     T: NonDrop,
// {
//     const MAY_HAVE_SIDE_EFFECT: bool = false;
// }

impl<T: Clone, A: Allocator + Clone> Clone for VVecIntoIter<T, A> {
    fn clone(&self) -> Self {
        // Adapted: upstream `self.as_slice().to_vec_in(alloc).into_iter()` uses the
        // alloc-internal `<[T]>::to_vec_in` and would yield a std `IntoIter`. Build a
        // `VVec` by cloning the remaining elements, then take its `VVecIntoIter`.
        let mut v = VVec::with_capacity_in(self.as_slice().len(), self.alloc.deref().clone());
        v.extend(self.as_slice().iter().cloned());
        v.into_iter()
    }
}

unsafe impl<#[may_dangle] T, A: Allocator> Drop for VVecIntoIter<T, A> {
    fn drop(&mut self) {
        struct DropGuard<'a, T, A: Allocator>(&'a mut VVecIntoIter<T, A>);

        impl<T, A: Allocator> Drop for DropGuard<'_, T, A> {
            fn drop(&mut self) {
                unsafe {
                    self.0.dealloc_only();
                }
            }
        }

        let guard = DropGuard(self);
        // destroy the remaining elements
        unsafe {
            ptr::drop_in_place(guard.0.as_raw_mut_slice());
        }
        // now `guard` will be dropped and do the rest
    }
}

// CORPSE (ProcessCommentingStandard): these three impls are the hooks that let
// `in_place_collect` reuse an `VVecIntoIter`'s backing allocation as the output buffer
// while collecting. `in_place_collect` is itself a corpse (it manipulates the raw
// allocation and cannot be expressed over the `VRawVec`-over-`std::Vec` shim), and
// `AsVecIntoIter` is defined only in that corpse module. With the in-place path removed
// these markers serve no purpose, so they are dropped.
// In addition to the SAFETY invariants of the following three unsafe traits
// also refer to the vec::in_place_collect module documentation to get an overview
// #[doc(hidden)]
// unsafe impl<T, A: Allocator> InPlaceIterable for VVecIntoIter<T, A> {
//     const EXPAND_BY: Option<NonZero<usize>> = NonZero::new(1);
//     const MERGE_BY: Option<NonZero<usize>> = NonZero::new(1);
// }
//
// #[doc(hidden)]
// unsafe impl<T, A: Allocator> SourceIter for VVecIntoIter<T, A> {
//     type Source = Self;
//
//     #[inline]
//     unsafe fn as_inner(&mut self) -> &mut Self::Source {
//         self
//     }
// }
//
// unsafe impl<T> AsVecIntoIter for VVecIntoIter<T> {
//     type Item = T;
//
//     fn as_into_iter(&mut self) -> &mut VVecIntoIter<Self::Item> {
//         self
//     }
// }
