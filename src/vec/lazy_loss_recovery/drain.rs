// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.
use core::iter::{FusedIterator, TrustedLen};
use core::mem::{self, ManuallyDrop, SizedTypeProperties};
use core::ptr::{self, NonNull};
use core::{fmt, slice};

use super::VVec;
use std::alloc::{Allocator, Global};

/// A draining iterator for `VVec<T>`.
///
/// This `struct` is created by [`VVec::drain`].
/// See its documentation for more.
///
/// # Example
///

pub struct VVecDrain<
    'a,
    T: 'a,
    A: Allocator + 'a = Global,
> {
    /// Index of tail to preserve
    pub(super) tail_start: usize,
    /// Length of tail
    pub(super) tail_len: usize,
    /// Current remaining range to remove
    pub(super) iter: slice::Iter<'a, T>,
    pub(super) vec: NonNull<VVec<T, A>>,
}

impl<T: fmt::Debug, A: Allocator> fmt::Debug for VVecDrain<'_, T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("VVecDrain").field(&self.iter.as_slice()).finish()
    }
}

impl<'a, T, A: Allocator> VVecDrain<'a, T, A> {
    /// Returns the remaining items of this iterator as a slice.
    ///
    /// # Examples
    ///
    #[must_use]

    pub fn as_slice(&self) -> &[T] {
        self.iter.as_slice()
    }

    /// Returns a reference to the underlying allocator.

    #[must_use]
    #[inline]
    pub fn allocator(&self) -> &A {
        unsafe { self.vec.as_ref().allocator() }
    }

    /// Keep unyielded elements in the source `VVec`.
    ///
    /// # Examples
    ///

    pub fn keep_rest(self) {
        // At this moment layout looks like this:
        //
        // [head] [yielded by next] [unyielded] [yielded by next_back] [tail]
        //        ^-- start         \_________/-- unyielded_len        \____/-- self.tail_len
        //                          ^-- unyielded_ptr                  ^-- tail
        //
        // Normally `Drop` impl would drop [unyielded] and then move [tail] to the `start`.
        // Here we want to
        // 1. Move [unyielded] to `start`
        // 2. Move [tail] to a new start at `start + len(unyielded)`
        // 3. Update length of the original vec to `len(head) + len(unyielded) + len(tail)`
        //    a. In case of ZST, this is the only thing we want to do
        // 4. Do *not* drop self, as everything is put in a consistent state already, there is nothing to do
        let mut this = ManuallyDrop::new(self);

        unsafe {
            let source_vec = this.vec.as_mut();

            // lazy_loss_recovery: `keep_rest` bypasses `Drop` (ManuallyDrop) and puts the vec into a
            // consistent state by hand, so cancel the deferred restore note here.
            source_vec.pending = None;

            let start = source_vec.len();
            let tail = this.tail_start;

            let unyielded_len = this.iter.len();
            let unyielded_ptr = this.iter.as_slice().as_ptr();

            // ZSTs have no identity, so we don't need to move them around.
            if !T::IS_ZST {
                let start_ptr = source_vec.as_mut_ptr().add(start);

                // memmove back unyielded elements
                if unyielded_ptr != start_ptr {
                    let src = unyielded_ptr;
                    let dst = start_ptr;

                    ptr::copy(src, dst, unyielded_len);
                }

                // memmove back untouched tail
                if tail != (start + unyielded_len) {
                    let src = source_vec.as_ptr().add(tail);
                    let dst = start_ptr.add(unyielded_len);
                    ptr::copy(src, dst, this.tail_len);
                }
            }

            source_vec.set_len(start + unyielded_len + this.tail_len);
        }
    }
}

impl<'a, T, A: Allocator> AsRef<[T]> for VVecDrain<'a, T, A> {
    fn as_ref(&self) -> &[T] {
        self.as_slice()
    }
}

unsafe impl<T: Sync, A: Sync + Allocator> Sync for VVecDrain<'_, T, A> {}

unsafe impl<T: Send, A: Send + Allocator> Send for VVecDrain<'_, T, A> {}

impl<'a, T, A: Allocator> VVecDrain<'a, T, A> {
    /// Finish a *forgotten* drain: the iterator from `drain()` was `mem::forget`-ten, so its `drop`
    /// never ran. The vec kept the work to do in `pending`; this reconstructs an equivalent iterator
    /// over the un-yielded range and drops it, which runs the exact same finish (`Drop` below): drop
    /// the un-yielded elements, move the tail back into place, restore `len` — panic-safe identically.
    pub(super) unsafe fn finish_forgotten_drain(
        vec: &'a mut VVec<T, A>,
        tail_start: usize,
        tail_len: usize,
        drop_offset: usize,
        drop_len: usize,
    ) {
        let range_slice = unsafe { slice::from_raw_parts(vec.as_ptr().add(drop_offset), drop_len) };
        let di = VVecDrain { tail_start, tail_len, iter: range_slice.iter(), vec: NonNull::from(vec) };
        drop(di);
    }

    // lazy_loss_recovery: mirror front progress (a `next`) into the vec's forget-safety record so a
    // forgotten iterator is finished correctly. No-op for an untracked drain (`pending` is `None`).
    #[inline]
    fn mirror_front_advance(&mut self) {
        unsafe {
            if let Some(super::Pending::Drain { drop_offset, drop_len, .. }) =
                self.vec.as_mut().pending.as_deref_mut()
            {
                *drop_offset += 1;
                *drop_len -= 1;
            }
        }
    }

    // lazy_loss_recovery: mirror back progress (a `next_back`) — the un-yielded range shrinks from
    // the back, so only `drop_len` decreases.
    #[inline]
    fn mirror_back_retreat(&mut self) {
        unsafe {
            if let Some(super::Pending::Drain { drop_len, .. }) =
                self.vec.as_mut().pending.as_deref_mut()
            {
                *drop_len -= 1;
            }
        }
    }
}

impl<T, A: Allocator> Iterator for VVecDrain<'_, T, A> {
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<T> {
        let v = self.iter.next().map(|elt| unsafe { ptr::read(elt as *const _) });
        if v.is_some() {
            self.mirror_front_advance();
        }
        v
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<T, A: Allocator> DoubleEndedIterator for VVecDrain<'_, T, A> {
    #[inline]
    fn next_back(&mut self) -> Option<T> {
        let v = self.iter.next_back().map(|elt| unsafe { ptr::read(elt as *const _) });
        if v.is_some() {
            self.mirror_back_retreat();
        }
        v
    }
}

impl<T, A: Allocator> Drop for VVecDrain<'_, T, A> {
    fn drop(&mut self) {
        // lazy_loss_recovery: this drop is finishing the drain itself, so cancel the deferred
        // restore (no-op for an untracked drain, whose `pending` is already `None`).
        unsafe { self.vec.as_mut().pending = None };

        /// Moves back the un-`VVecDrain`ed elements to restore the original `VVec`.
        struct DropGuard<'r, 'a, T, A: Allocator>(&'r mut VVecDrain<'a, T, A>);

        impl<'r, 'a, T, A: Allocator> Drop for DropGuard<'r, 'a, T, A> {
            fn drop(&mut self) {
                if self.0.tail_len > 0 {
                    unsafe {
                        let source_vec = self.0.vec.as_mut();
                        // memmove back untouched tail, update to new length
                        let start = source_vec.len();
                        let tail = self.0.tail_start;
                        if tail != start {
                            let src = source_vec.as_ptr().add(tail);
                            let dst = source_vec.as_mut_ptr().add(start);
                            ptr::copy(src, dst, self.0.tail_len);
                        }
                        source_vec.set_len(start + self.0.tail_len);
                    }
                }
            }
        }

        let iter = mem::take(&mut self.iter);
        let drop_len = iter.len();

        let mut vec = self.vec;

        if T::IS_ZST {
            // ZSTs have no identity, so we don't need to move them around, we only need to drop the correct amount.
            // this can be achieved by manipulating the VVec length instead of moving values out from `iter`.
            unsafe {
                let vec = vec.as_mut();
                let old_len = vec.len();
                vec.set_len(old_len + drop_len + self.tail_len);
                vec.truncate(old_len + self.tail_len);
            }

            return;
        }

        // ensure elements are moved back into their appropriate places, even when drop_in_place panics
        let _guard = DropGuard(self);

        if drop_len == 0 {
            return;
        }

        // as_slice() must only be called when iter.len() is > 0 because
        // it also gets touched by vec::VVecSplice which may turn it into a dangling pointer
        // which would make it and the vec pointer point to different allocations which would
        // lead to invalid pointer arithmetic below.
        let drop_ptr = iter.as_slice().as_ptr();

        unsafe {
            // drop_ptr comes from a slice::Iter which only gives us a &[T] but for drop_in_place
            // a pointer with mutable provenance is necessary. Therefore we must reconstruct
            // it from the original vec but also avoid creating a &mut to the front since that could
            // invalidate raw pointers to it which some unsafe code might rely on.
            let vec_ptr = vec.as_mut().as_mut_ptr();
            let drop_offset = drop_ptr.offset_from_unsigned(vec_ptr);
            let to_drop = ptr::slice_from_raw_parts_mut(vec_ptr.add(drop_offset), drop_len);
            ptr::drop_in_place(to_drop);
        }
    }
}

impl<T, A: Allocator> ExactSizeIterator for VVecDrain<'_, T, A> {
    fn is_empty(&self) -> bool {
        self.iter.is_empty()
    }
}

unsafe impl<T, A: Allocator> TrustedLen for VVecDrain<'_, T, A> {}

impl<T, A: Allocator> FusedIterator for VVecDrain<'_, T, A> {}
