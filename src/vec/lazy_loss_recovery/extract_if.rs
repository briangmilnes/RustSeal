// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.
use core::ops::{Range, RangeBounds};
use core::{fmt, ptr, slice};

use super::VVec;
use std::alloc::{Allocator, Global};

/// An iterator which uses a closure to determine if an element should be removed.
///
/// This struct is created by [`VVec::extract_if`].
/// See its documentation for more.
///
/// # Example
///

#[must_use = "iterators are lazy and do nothing unless consumed; \
    use `retain_mut` or `extract_if().for_each(drop)` to remove and discard elements"]
pub struct VVecExtractIf<
    'a,
    T,
    F,
    A: Allocator = Global,
> {
    vec: &'a mut VVec<T, A>,
    /// The index of the item that will be inspected by the next call to `next`.
    idx: usize,
    /// Elements at and beyond this point will be retained. Must be equal or smaller than `old_len`.
    end: usize,
    /// The number of items that have been drained (removed) thus far.
    del: usize,
    /// The original length of `vec` prior to draining.
    old_len: usize,
    /// The filter test predicate.
    pred: F,
}

impl<'a, T, F, A: Allocator> VVecExtractIf<'a, T, F, A> {
    pub(super) fn new<R: RangeBounds<usize>>(vec: &'a mut VVec<T, A>, pred: F, range: R) -> Self {
        let old_len = vec.len();
        let Range { start, end } = slice::range(range, ..old_len);

        // Guard against the vec getting leaked (leak amplification)
        unsafe {
            vec.set_len(0);
        }
        // lazy_loss_recovery forget-safety: record the finish work so a forgotten iterator is
        // completed by the next op (`restore_wf_wo_data_loss`) instead of leaking.
        vec.set_pending_extract_if(old_len, start, 0);
        VVecExtractIf { vec, idx: start, del: 0, end, old_len, pred }
    }

    // lazy_loss_recovery: mirror iteration progress (`idx`/`del`) into the vec's forget-safety record
    // so a forgotten iterator is finished correctly by `restore_wf_wo_data_loss`.
    #[inline]
    fn mirror_progress(&mut self) {
        if let Some(super::Pending::ExtractIf { idx, del, .. }) = self.vec.pending.as_deref_mut() {
            *idx = self.idx;
            *del = self.del;
        }
    }

    /// Returns a reference to the underlying allocator.

    #[inline]
    pub fn allocator(&self) -> &A {
        self.vec.allocator()
    }
}

impl<T, F, A: Allocator> Iterator for VVecExtractIf<'_, T, F, A>
where
    F: FnMut(&mut T) -> bool,
{
    type Item = T;

    fn next(&mut self) -> Option<T> {
        while self.idx < self.end {
            let i = self.idx;
            // SAFETY:
            //  We know that `i < self.end` from the if guard and that `self.end <= self.old_len` from
            //  the validity of `Self`. Therefore `i` points to an element within `vec`.
            //
            //  Additionally, the i-th element is valid because each element is visited at most once
            //  and it is the first time we access vec[i].
            //
            //  Note: we can't use `vec.get_unchecked_mut(i)` here since the precondition for that
            //  function is that i < vec.len(), but we've set vec's length to zero.
            let cur = unsafe { &mut *self.vec.as_mut_ptr().add(i) };
            let drained = (self.pred)(cur);
            // Update the index *after* the predicate is called. If the index
            // is updated prior and the predicate panics, the element at this
            // index would be leaked.
            self.idx += 1;
            if drained {
                self.del += 1;
                // SAFETY: We never touch this element again after returning it.
                let val = unsafe { ptr::read(cur) };
                self.mirror_progress(); // lazy_loss_recovery: record progress before handing the value out
                return Some(val);
            } else if self.del > 0 {
                // SAFETY: `self.del` > 0, so the hole slot must not overlap with current element.
                // We use copy for move, and never touch this element again.
                unsafe {
                    let hole_slot = self.vec.as_mut_ptr().add(i - self.del);
                    ptr::copy_nonoverlapping(cur, hole_slot, 1);
                }
            }
        }
        self.mirror_progress(); // lazy_loss_recovery: record final progress (iteration exhausted)
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.end - self.idx))
    }
}

impl<T, F, A: Allocator> Drop for VVecExtractIf<'_, T, F, A> {
    fn drop(&mut self) {
        // lazy_loss_recovery: this drop is finishing the extraction itself, so cancel the deferred
        // restore.
        self.vec.pending = None;
        let (old_len, idx, del) = (self.old_len, self.idx, self.del);
        // SAFETY: `idx`/`del`/`old_len` describe exactly the in-progress extraction; the trailing
        // un-inspected items are valid (never touched).
        unsafe { finish_forgotten_extract_if(self.vec, old_len, idx, del) };
    }
}

/// Finish a *forgotten* `extract_if`: its iterator was `mem::forget`-ten, so its `drop` never ran.
/// The vec kept the progress in `pending`; complete the same finish the `drop` would have — compact
/// the un-inspected tail back over the `del` removed holes and set `len = old_len - del`. No element
/// is dropped here (removed ones were already yielded; the rest are retained), so this cannot panic.
pub(super) unsafe fn finish_forgotten_extract_if<T, A: Allocator>(
    vec: &mut VVec<T, A>,
    old_len: usize,
    idx: usize,
    del: usize,
) {
    if del > 0 {
        // SAFETY: Trailing unchecked items must be valid since we never touch them.
        unsafe {
            ptr::copy(vec.as_ptr().add(idx), vec.as_mut_ptr().add(idx - del), old_len - idx);
        }
    }
    // SAFETY: After filling holes, all items are in contiguous memory.
    unsafe {
        vec.set_len(old_len - del);
    }
}

impl<T, F, A> fmt::Debug for VVecExtractIf<'_, T, F, A>
where
    T: fmt::Debug,
    A: Allocator,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // We have to use pointer arithmetics here,
        // because the length of `self.vec` is temporarily set to 0.
        let start = self.vec.as_ptr();

        // SAFETY: we always keep first `self.idx - self.del` elements valid.
        let retained = unsafe { slice::from_raw_parts(start, self.idx - self.del) };

        // SAFETY: we have not yet touched elements starting at `self.idx`.
        let valid_tail =
            unsafe { slice::from_raw_parts(start.add(self.idx), self.old_len - self.idx) };

        // SAFETY: `end - idx <= old_len - idx`, because `end <= old_len`. Also `idx <= end` by invariant.
        let (remainder, skipped_tail) =
            unsafe { valid_tail.split_at_unchecked(self.end - self.idx) };

        f.debug_struct("VVecExtractIf")
            .field("retained", &retained)
            .field("remainder", &remainder)
            .field("skipped_tail", &skipped_tail)
            .finish_non_exhaustive()
    }
}
