// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.
use core::iter::FusedIterator;
use core::marker::PhantomData;
use core::mem::{self, SizedTypeProperties};
use core::ptr::NonNull;
use core::{fmt, ptr};

use super::VVecDeque;
use std::alloc::{Allocator, Global};

/// A draining iterator over the elements of a `VVecDeque`.
///
/// This `struct` is created by the [`drain`] method on [`VVecDeque`]. See its
/// documentation for more.
///
/// [`drain`]: VVecDeque::drain
pub struct VVecDequeDrain<
    'a,
    T: 'a,
     A: Allocator = Global,
> {
    // We can't just use a &mut VVecDeque<T, A>, as that would make VVecDequeDrain invariant over T
    // and we want it to be covariant instead
    pub(super) deque: NonNull<VVecDeque<T, A>>,
    // drain_start is stored in deque.len
    pub(super) drain_len: usize,
    // index into the logical array, not the physical one (always lies in [0..deque.len))
    pub(super) idx: usize,
    // number of elements after the drained range
    pub(super) tail_len: usize,
    pub(super) remaining: usize,
    // Needed to make VVecDequeDrain covariant over T
    _marker: PhantomData<&'a T>,
}

impl<'a, T, A: Allocator> VVecDequeDrain<'a, T, A> {
    // `track_for_forget_safety` is the lazy_loss_recovery forget-safety hook: when true (the public `drain()` path) the
    // deque records a `pending` so a forgotten iterator is finished by the next deque op
    // (`restore_wf_wo_data_loss`). When false (the internal `splice` path, which always lets its held drain drop)
    // no record is kept, matching the std behavior. See `VVecDeque::pending`.
    pub(super) unsafe fn new(
        deque: &'a mut VVecDeque<T, A>,
        drain_start: usize,
        drain_len: usize,
        track_for_forget_safety: bool,
    ) -> Self {
        let orig_len = mem::replace(&mut deque.len, drain_start);
        let tail_len = orig_len - drain_start - drain_len;
        if track_for_forget_safety {
            deque.set_pending_drain(drain_len, tail_len, drain_start, drain_len);
        }
        VVecDequeDrain {
            deque: NonNull::from(deque),
            drain_len,
            idx: drain_start,
            tail_len,
            remaining: drain_len,
            _marker: PhantomData,
        }
    }

    /// Finish a *forgotten* drain: the iterator from `drain()` was `mem::forget`-ten, so its `drop`
    /// never ran. The deque kept the work to do in `pending`; this reconstructs an equivalent
    /// iterator and drops it, which runs the exact same finish (`Drop` below): drop the un-yielded
    /// drained elements, slide the rest into place, restore `len` — and is panic-safe identically.
    pub(super) unsafe fn finish_forgotten_drain(
        deque: &'a mut VVecDeque<T, A>,
        drain_len: usize,
        idx: usize,
        tail_len: usize,
        remaining: usize,
    ) {
        let di = VVecDequeDrain {
            deque: NonNull::from(deque),
            drain_len,
            idx,
            tail_len,
            remaining,
            _marker: PhantomData,
        };
        drop(di);
    }

    // lazy_loss_recovery: mirror iteration progress into the deque's forget-safety record, so that a
    // forgotten iterator is finished correctly by `restore_wf_wo_data_loss`. No-op for the untracked `splice`
    // path (its `pending` is `None`). `idx`/`remaining` delimit the un-yielded drained range.
    #[inline]
    fn mirror_progress(&mut self) {
        unsafe {
            if let Some(super::Pending::Drain { idx, remaining, .. }) = self.deque.as_mut().pending.as_deref_mut() {
                *idx = self.idx;
                *remaining = self.remaining;
            }
        }
    }

    // Only returns pointers to the slices, as that's all we need
    // to drop them. May only be called if `self.remaining != 0`.
    pub(super) unsafe fn as_slices(&self) -> (*mut [T], *mut [T]) {
        unsafe {
            let deque = self.deque.as_ref();

            // We know that `self.idx + self.remaining <= deque.len <= usize::MAX`, so this won't overflow.
            let logical_remaining_range = self.idx..self.idx + self.remaining;

            // SAFETY: `logical_remaining_range` represents the
            // range into the logical buffer of elements that
            // haven't been drained yet, so they're all initialized,
            // and `slice::range(start..end, end) == start..end`,
            // so the preconditions for `slice_ranges` are met.
            let (a_range, b_range) =
                deque.slice_ranges(logical_remaining_range.clone(), logical_remaining_range.end);
            (deque.buffer_range(a_range), deque.buffer_range(b_range))
        }
    }
}

impl<T: fmt::Debug, A: Allocator> fmt::Debug for VVecDequeDrain<'_, T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("VVecDequeDrain")
            .field(&self.drain_len)
            .field(&self.idx)
            .field(&self.tail_len)
            .field(&self.remaining)
            .finish()
    }
}

unsafe impl<T: Sync, A: Allocator + Sync> Sync for VVecDequeDrain<'_, T, A> {}
unsafe impl<T: Send, A: Allocator + Send> Send for VVecDequeDrain<'_, T, A> {}

impl<T, A: Allocator> Drop for VVecDequeDrain<'_, T, A> {
    fn drop(&mut self) {
        // lazy_loss_recovery: this `drop` is finishing the drain itself, so cancel the deferred restore_wf_wo_data_loss
        // (no-op for the untracked `splice` path, whose `pending` is already `None`).
        unsafe { self.deque.as_mut().pending = None };

        struct DropGuard<'r, 'a, T, A: Allocator>(&'r mut VVecDequeDrain<'a, T, A>);

        let guard = DropGuard(self);

        if mem::needs_drop::<T>() && guard.0.remaining != 0 {
            unsafe {
                // SAFETY: We just checked that `self.remaining != 0`.
                let (front, back) = guard.0.as_slices();
                // since idx is a logical index, we don't need to worry about wrapping.
                guard.0.idx += front.len();
                guard.0.remaining -= front.len();
                ptr::drop_in_place(front);
                guard.0.remaining = 0;
                ptr::drop_in_place(back);
            }
        }

        // Dropping `guard` handles moving the remaining elements into place.
        impl<'r, 'a, T, A: Allocator> Drop for DropGuard<'r, 'a, T, A> {
            #[inline]
            fn drop(&mut self) {
                if mem::needs_drop::<T>() && self.0.remaining != 0 {
                    unsafe {
                        // SAFETY: We just checked that `self.remaining != 0`.
                        let (front, back) = self.0.as_slices();
                        ptr::drop_in_place(front);
                        ptr::drop_in_place(back);
                    }
                }

                let source_deque = unsafe { self.0.deque.as_mut() };

                let drain_len = self.0.drain_len;
                let head_len = source_deque.len; // #elements in front of the drain
                let tail_len = self.0.tail_len; // #elements behind the drain
                let new_len = head_len + tail_len;

                if T::IS_ZST {
                    // no need to copy around any memory if T is a ZST
                    source_deque.len = new_len;
                    return;
                }

                // Next, we will fill the hole left by the drain with as few writes as possible.
                // The code below handles the following control flow and reduces the amount of
                // branches under the assumption that `head_len == 0 || tail_len == 0`, i.e.
                // draining at the front or at the back of the dequeue is especially common.
                //
                // H = "head index" = `deque.head`
                // h = elements in front of the drain
                // d = elements in the drain
                // t = elements behind the drain
                //
                // Note that the buffer may wrap at any point and the wrapping is handled by
                // `wrap_copy` and `to_physical_idx`.
                //
                // Case 1: if `head_len == 0 && tail_len == 0`
                // Everything was drained, reset the head index back to 0.
                //             H
                // [ . . . . . d d d d . . . . . ]
                //   H
                // [ . . . . . . . . . . . . . . ]
                //
                // Case 2: else if `tail_len == 0`
                // Don't move data or the head index.
                //         H
                // [ . . . h h h h d d d d . . . ]
                //         H
                // [ . . . h h h h . . . . . . . ]
                //
                // Case 3: else if `head_len == 0`
                // Don't move data, but move the head index.
                //         H
                // [ . . . d d d d t t t t . . . ]
                //                 H
                // [ . . . . . . . t t t t . . . ]
                //
                // Case 4: else if `tail_len <= head_len`
                // Move data, but not the head index.
                //       H
                // [ . . h h h h d d d d t t . . ]
                //       H
                // [ . . h h h h t t . . . . . . ]
                //
                // Case 5: else
                // Move data and the head index.
                //       H
                // [ . . h h d d d d t t t t . . ]
                //               H
                // [ . . . . . . h h t t t t . . ]

                // When draining at the front (`.drain(..n)`) or at the back (`.drain(n..)`),
                // we don't need to copy any data. The number of elements copied would be 0.
                if head_len != 0 && tail_len != 0 {
                    join_head_and_tail_wrapping(source_deque, drain_len, head_len, tail_len);
                    // Marking this function as cold helps LLVM to eliminate it entirely if
                    // this branch is never taken.
                    // We use `#[cold]` instead of `#[inline(never)]`, because inlining this
                    // function into the general case (`.drain(n..m)`) is fine.
                    // See `tests/codegen-llvm/vecdeque-drain.rs` for a test.
                    #[cold]
                    fn join_head_and_tail_wrapping<T, A: Allocator>(
                        source_deque: &mut VVecDeque<T, A>,
                        drain_len: usize,
                        head_len: usize,
                        tail_len: usize,
                    ) {
                        // Pick whether to move the head or the tail here.
                        let (src, dst, len);
                        if head_len < tail_len {
                            src = source_deque.head;
                            dst = source_deque.to_physical_idx(drain_len);
                            len = head_len;
                        } else {
                            src = source_deque.to_physical_idx(head_len + drain_len);
                            dst = source_deque.to_physical_idx(head_len);
                            len = tail_len;
                        };

                        unsafe {
                            source_deque.wrap_copy(src, dst, len);
                        }
                    }
                }

                if new_len == 0 {
                    // Special case: If the entire deque was drained, reset the head back to 0,
                    // like `.clear()` does.
                    source_deque.head = 0;
                } else if head_len < tail_len {
                    // If we moved the head above, then we need to adjust the head index here.
                    source_deque.head = source_deque.to_physical_idx(drain_len);
                }
                source_deque.len = new_len;
            }
        }
    }
}

impl<T, A: Allocator> Iterator for VVecDequeDrain<'_, T, A> {
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<T> {
        if self.remaining == 0 {
            return None;
        }
        let wrapped_idx = unsafe { self.deque.as_ref().to_physical_idx(self.idx) };
        self.idx += 1;
        self.remaining -= 1;
        self.mirror_progress();
        Some(unsafe { self.deque.as_mut().buffer_read(wrapped_idx) })
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.remaining;
        (len, Some(len))
    }
}

impl<T, A: Allocator> DoubleEndedIterator for VVecDequeDrain<'_, T, A> {
    #[inline]
    fn next_back(&mut self) -> Option<T> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        self.mirror_progress();
        let wrapped_idx = unsafe { self.deque.as_ref().to_physical_idx(self.idx + self.remaining) };
        Some(unsafe { self.deque.as_mut().buffer_read(wrapped_idx) })
    }
}

impl<T, A: Allocator> ExactSizeIterator for VVecDequeDrain<'_, T, A> {}

impl<T, A: Allocator> FusedIterator for VVecDequeDrain<'_, T, A> {}
