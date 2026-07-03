// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! UnsafeBinaryHeap — the binary-heap / priority-queue of `alloc::collections::binary_heap`
//! (rust-libs 1.96.0), extracted in FULL: the complete public method surface, the
//! `UnsafePeekMut` mutable-peek guard, all five iterator types (`UnsafeIter`, `UnsafeIntoIter`,
//! `UnsafeIntoIterSorted`, `UnsafeDrain`, `UnsafeDrainSorted`), and every trait impl
//! (`Clone`/`Default`/`Debug`, `From`/`Into`, `FromIterator`, `IntoIterator`,
//! `Extend`, and the iterator-trait family).
//!
//! Faithful to rust-libs including the allocator generic `A: Allocator = Global`
//! (vstd specs `Vec<T, A>` generically, so this stays verus-ready).
//!
//! Renamed with the `Unsafe` prefix so nothing collides with the std types it was copied
//! from: `BinaryHeap` -> `UnsafeBinaryHeap`, `PeekMut` -> `UnsafePeekMut`, `Hole` -> `UnsafeHole`,
//! `RebuildOnDrop` -> `UnsafeRebuildOnDrop`, `Iter` -> `UnsafeIter`, `IntoIter` -> `UnsafeIntoIter`,
//! `IntoIterSorted` -> `UnsafeIntoIterSorted`, `Drain` -> `UnsafeDrain`,
//! `DrainSorted` -> `UnsafeDrainSorted`.
//!
//! Not brought across (ProcessCommentingStandard reason — std-internal markers, see
//! the commented blocks on `UnsafeIntoIter`): the in-place-collect specialization hooks
//! `SourceIter`, `InPlaceIterable`, `TrustedFused`, and `AsVecIntoIter`. They are
//! `#[doc(hidden)]` perma-unstable optimization markers, exercised by no public API,
//! test, or bench; `AsVecIntoIter` is not even nameable outside `alloc` (not exported).

use core::iter::{FusedIterator, TrustedLen};
use core::mem::{self, swap, ManuallyDrop};
use core::num::NonZero;
use core::ops::{Deref, DerefMut};
use core::{fmt, ptr};
use std::alloc::{Allocator, Global};
use std::collections::TryReserveError;

/// A priority queue implemented with a binary (max-)heap. (Renamed from `BinaryHeap`.)
///
/// It is a logic error for an item to be modified in such a way that the item's
/// ordering relative to any other item, as determined by the `Ord` trait, changes
/// while it is in the heap.
pub struct UnsafeBinaryHeap<T, A: Allocator = Global> {
    data: Vec<T, A>,
}

/// Structure wrapping a mutable reference to the greatest item on a `UnsafeBinaryHeap`.
///
/// Created by the [`UnsafeBinaryHeap::peek_mut`] method. (Renamed from `PeekMut`.)
pub struct UnsafePeekMut<'a, T: 'a + Ord, A: Allocator = Global> {
    heap: &'a mut UnsafeBinaryHeap<T, A>,
    // If a set_len + sift_down are required, this is Some. If a &mut T has not
    // yet been exposed to peek_mut()'s caller, it's None.
    original_len: Option<NonZero<usize>>,
}

impl<T: Ord + fmt::Debug, A: Allocator> fmt::Debug for UnsafePeekMut<'_, T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("UnsafePeekMut").field(&self.heap.data[0]).finish()
    }
}

impl<T: Ord, A: Allocator> Drop for UnsafePeekMut<'_, T, A> {
    fn drop(&mut self) {
        if let Some(original_len) = self.original_len {
            // SAFETY: That's how many elements were in the Vec at the time of the
            // UnsafePeekMut::deref_mut call, and therefore also at the time of the
            // UnsafeBinaryHeap::peek_mut call. Since the UnsafePeekMut did not end up getting
            // leaked, we are now undoing the leak amplification that the DerefMut
            // prepared for.
            unsafe { self.heap.data.set_len(original_len.get()) };

            // SAFETY: UnsafePeekMut is only instantiated for non-empty heaps.
            unsafe { self.heap.sift_down(0) };
        }
    }
}

impl<T: Ord, A: Allocator> Deref for UnsafePeekMut<'_, T, A> {
    type Target = T;
    fn deref(&self) -> &T {
        debug_assert!(!self.heap.is_empty());
        // SAFE: UnsafePeekMut is only instantiated for non-empty heaps.
        unsafe { self.heap.data.get_unchecked(0) }
    }
}

impl<T: Ord, A: Allocator> DerefMut for UnsafePeekMut<'_, T, A> {
    fn deref_mut(&mut self) -> &mut T {
        debug_assert!(!self.heap.is_empty());

        let len = self.heap.len();
        if len > 1 {
            // Here we preemptively leak all the rest of the underlying vector after the
            // currently max element. If the caller mutates the &mut T we're about to
            // give them, and then leaks the UnsafePeekMut, all these elements will remain
            // leaked. If they don't leak the UnsafePeekMut, then either Drop or
            // UnsafePeekMut::pop will un-leak the vector elements. This technique is
            // described throughout the standard library as "leak amplification".
            unsafe {
                // SAFETY: len > 1 so len != 0.
                self.original_len = Some(NonZero::new_unchecked(len));
                // SAFETY: len > 1 so all this does for now is leak elements, which is
                // safe.
                self.heap.data.set_len(1);
            }
        }

        // SAFE: UnsafePeekMut is only instantiated for non-empty heaps.
        unsafe { self.heap.data.get_unchecked_mut(0) }
    }
}

impl<'a, T: Ord, A: Allocator> UnsafePeekMut<'a, T, A> {
    /// Sifts the current element to its new position.
    ///
    /// Afterwards refers to the new element. Returns if the element changed.
    #[must_use = "is equivalent to dropping and getting a new UnsafePeekMut except for return information"]
    pub fn refresh(&mut self) -> bool {
        // The length of the underlying heap is unchanged by sifting down. The value
        // stored for leak amplification thus remains accurate. We erase the leak
        // amplification firstly because the operation is then equivalent to
        // constructing a new UnsafePeekMut and secondly this avoids any future complication
        // where original_len being non-empty would be interpreted as the heap having
        // been leak amplified instead of checking the heap itself.
        if let Some(original_len) = self.original_len.take() {
            // SAFETY: This is how many elements were in the Vec at the time of the
            // UnsafeBinaryHeap::peek_mut call.
            unsafe { self.heap.data.set_len(original_len.get()) };

            // The length of the heap did not change by sifting, upholding our own
            // invariants.

            // SAFETY: UnsafePeekMut is only instantiated for non-empty heaps.
            (unsafe { self.heap.sift_down(0) }) != 0
        } else {
            // The element was not modified.
            false
        }
    }

    /// Removes the peeked value from the heap and returns it.
    pub fn pop(mut this: UnsafePeekMut<'a, T, A>) -> T {
        if let Some(original_len) = this.original_len.take() {
            // SAFETY: This is how many elements were in the Vec at the time of the
            // UnsafeBinaryHeap::peek_mut call.
            unsafe { this.heap.data.set_len(original_len.get()) };

            // Unlike in Drop, here we don't also need to do a sift_down even if the
            // caller could've mutated the element. It is removed from the heap on the
            // next line and pop() is not sensitive to its value.
        }

        // SAFETY: Having a `UnsafePeekMut` element proves that the associated binary heap is
        // non-empty, so the `pop` operation will not fail.
        unsafe { this.heap.pop().unwrap_unchecked() }
    }
}

impl<T: Clone, A: Allocator + Clone> Clone for UnsafeBinaryHeap<T, A> {
    fn clone(&self) -> Self {
        UnsafeBinaryHeap { data: self.data.clone() }
    }

    /// Overwrites the contents of `self` with a clone of the contents of `source`,
    /// avoiding reallocation if possible.
    fn clone_from(&mut self, source: &Self) {
        self.data.clone_from(&source.data);
    }
}

impl<T> Default for UnsafeBinaryHeap<T> {
    /// Creates an empty `UnsafeBinaryHeap<T>`.
    #[inline]
    fn default() -> UnsafeBinaryHeap<T> {
        UnsafeBinaryHeap::new()
    }
}

impl<T: fmt::Debug, A: Allocator> fmt::Debug for UnsafeBinaryHeap<T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

/// Drop guard that repairs the heap tail when an in-place mutation (extend / retain)
/// finishes or unwinds. (Renamed from `RebuildOnDrop`.)
struct UnsafeRebuildOnDrop<'a, T: Ord, A: Allocator = Global> {
    heap: &'a mut UnsafeBinaryHeap<T, A>,
    rebuild_from: usize,
}

impl<T: Ord, A: Allocator> Drop for UnsafeRebuildOnDrop<'_, T, A> {
    fn drop(&mut self) {
        self.heap.rebuild_tail(self.rebuild_from);
    }
}

impl<T> UnsafeBinaryHeap<T> {
    /// Creates an empty `UnsafeBinaryHeap` as a max-heap.
    #[must_use]
    pub const fn new() -> UnsafeBinaryHeap<T> {
        UnsafeBinaryHeap { data: Vec::new() }
    }

    /// Creates an empty `UnsafeBinaryHeap` with at least the specified capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> UnsafeBinaryHeap<T> {
        UnsafeBinaryHeap { data: Vec::with_capacity(capacity) }
    }
}

impl<T, A: Allocator> UnsafeBinaryHeap<T, A> {
    /// Creates an empty `UnsafeBinaryHeap` as a max-heap, using `A` as allocator.
    #[must_use]
    pub const fn new_in(alloc: A) -> UnsafeBinaryHeap<T, A> {
        UnsafeBinaryHeap { data: Vec::new_in(alloc) }
    }

    /// Creates an empty `UnsafeBinaryHeap` with at least the specified capacity, using `A`.
    #[must_use]
    pub fn with_capacity_in(capacity: usize, alloc: A) -> UnsafeBinaryHeap<T, A> {
        UnsafeBinaryHeap { data: Vec::with_capacity_in(capacity, alloc) }
    }

    /// Creates a `UnsafeBinaryHeap` from the supplied `vec` without rebuilding it.
    ///
    /// # Safety
    ///
    /// The supplied `vec` must be a max-heap, i.e. for all indices `0 < i < vec.len()`,
    /// `vec[(i - 1) / 2] >= vec[i]`.
    #[must_use]
    pub unsafe fn from_raw_vec(vec: Vec<T, A>) -> UnsafeBinaryHeap<T, A> {
        UnsafeBinaryHeap { data: vec }
    }
}

impl<T: Ord, A: Allocator> UnsafeBinaryHeap<T, A> {
    /// Returns a mutable reference to the greatest item, or `None` if empty.
    ///
    /// Note: If the `UnsafePeekMut` value is leaked, some heap elements might get leaked
    /// along with it, but the remaining elements will remain a valid heap.
    pub fn peek_mut(&mut self) -> Option<UnsafePeekMut<'_, T, A>> {
        if self.is_empty() { None } else { Some(UnsafePeekMut { heap: self, original_len: None }) }
    }

    /// Removes the greatest item and returns it, or `None` if empty.
    pub fn pop(&mut self) -> Option<T> {
        self.data.pop().map(|mut item| {
            if !self.is_empty() {
                swap(&mut item, &mut self.data[0]);
                // SAFETY: !self.is_empty() means that self.len() > 0.
                unsafe { self.sift_down_to_bottom(0) };
            }
            item
        })
    }

    /// Removes and returns the greatest item if `predicate` returns `true`, else
    /// `None` (the predicate is not called on an empty heap).
    pub fn pop_if(&mut self, predicate: impl FnOnce(&T) -> bool) -> Option<T> {
        let first = self.peek()?;
        if predicate(first) { self.pop() } else { None }
    }

    /// Pushes an item onto the heap.
    pub fn push(&mut self, item: T) {
        let old_len = self.len();
        self.data.push(item);
        // SAFETY: Since we pushed a new item it means that
        //  old_len = self.len() - 1 < self.len().
        unsafe { self.sift_up(0, old_len) };
    }

    /// Consumes the heap and returns a vector in sorted (ascending) order.
    #[must_use = "`self` will be dropped if the result is not used"]
    pub fn into_sorted_vec(mut self) -> Vec<T, A> {
        let mut end = self.len();
        while end > 1 {
            end -= 1;
            // SAFETY: `end` goes from `self.len() - 1` to 1 (both included), so it's
            //  always a valid index to access. It is safe to access index 0 (i.e.
            //  `ptr`), because 1 <= end < self.len(), which means self.len() >= 2.
            unsafe {
                let ptr = self.data.as_mut_ptr();
                ptr::swap(ptr, ptr.add(end));
            }
            // SAFETY: `end` goes from `self.len() - 1` to 1 (both included) so:
            //  0 < 1 <= end <= self.len() - 1 < self.len().
            unsafe { self.sift_down_range(0, end) };
        }
        self.into_vec()
    }

    // The implementations of sift_up and sift_down use unsafe blocks in order to move
    // an element out of the vector (leaving behind a hole), shift along the others and
    // move the removed element back into the vector at the final location of the hole.
    // The `UnsafeHole` type is used to represent this, and makes sure the hole is filled
    // back at the end of its scope, even on panic.

    /// # Safety
    ///
    /// The caller must guarantee that `pos < self.len()`. Returns the new position.
    unsafe fn sift_up(&mut self, start: usize, pos: usize) -> usize {
        // Take out the value at `pos` and create a hole.
        // SAFETY: The caller guarantees that pos < self.len().
        let mut hole = unsafe { UnsafeHole::new(&mut self.data, pos) };

        while hole.pos() > start {
            let parent = (hole.pos() - 1) / 2;

            // SAFETY: hole.pos() > start >= 0, which means hole.pos() > 0 and so
            //  hole.pos() - 1 can't underflow. This guarantees that parent < hole.pos()
            //  so it's a valid index and also != hole.pos().
            if hole.element() <= unsafe { hole.get(parent) } {
                break;
            }

            // SAFETY: Same as above.
            unsafe { hole.move_to(parent) };
        }

        hole.pos()
    }

    /// Take an element at `pos` and move it down the heap, while its children are
    /// larger. Returns the new position.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `pos < end <= self.len()`.
    unsafe fn sift_down_range(&mut self, pos: usize, end: usize) -> usize {
        // SAFETY: The caller guarantees that pos < end <= self.len().
        let mut hole = unsafe { UnsafeHole::new(&mut self.data, pos) };
        let mut child = 2 * hole.pos() + 1;

        // Loop invariant: child == 2 * hole.pos() + 1.
        while child <= end.saturating_sub(2) {
            // compare with the greater of the two children
            // SAFETY: child < end - 1 < self.len() and child + 1 < end <= self.len(),
            //  so they're valid indexes. child == 2 * hole.pos() + 1 != hole.pos() and
            //  child + 1 == 2 * hole.pos() + 2 != hole.pos().
            child += unsafe { hole.get(child) <= hole.get(child + 1) } as usize;

            // if we are already in order, stop.
            // SAFETY: child is now either the old child or the old child+1. We already
            //  proved that both are < self.len() and != hole.pos().
            if hole.element() >= unsafe { hole.get(child) } {
                return hole.pos();
            }

            // SAFETY: same as above.
            unsafe { hole.move_to(child) };
            child = 2 * hole.pos() + 1;
        }

        // SAFETY: && short circuit, which means that in the second condition it's
        //  already true that child == end - 1 < self.len().
        if child == end - 1 && hole.element() < unsafe { hole.get(child) } {
            // SAFETY: child is already proven to be a valid index and
            //  child == 2 * hole.pos() + 1 != hole.pos().
            unsafe { hole.move_to(child) };
        }

        hole.pos()
    }

    /// # Safety
    ///
    /// The caller must guarantee that `pos < self.len()`.
    unsafe fn sift_down(&mut self, pos: usize) -> usize {
        let len = self.len();
        // SAFETY: pos < len is guaranteed by the caller and obviously
        //  len = self.len() <= self.len().
        unsafe { self.sift_down_range(pos, len) }
    }

    /// Take an element at `pos` and move it all the way down the heap, then sift it up
    /// to its position.
    ///
    /// Note: This is faster when the element is known to be large / should be closer to
    /// the bottom.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `pos < self.len()`.
    unsafe fn sift_down_to_bottom(&mut self, mut pos: usize) {
        let end = self.len();
        let start = pos;

        // SAFETY: The caller guarantees that pos < self.len().
        let mut hole = unsafe { UnsafeHole::new(&mut self.data, pos) };
        let mut child = 2 * hole.pos() + 1;

        // Loop invariant: child == 2 * hole.pos() + 1.
        while child <= end.saturating_sub(2) {
            // SAFETY: child < end - 1 < self.len() and child + 1 < end <= self.len(),
            //  so they're valid indexes. child == 2 * hole.pos() + 1 != hole.pos() and
            //  child + 1 == 2 * hole.pos() + 2 != hole.pos().
            child += unsafe { hole.get(child) <= hole.get(child + 1) } as usize;

            // SAFETY: Same as above.
            unsafe { hole.move_to(child) };
            child = 2 * hole.pos() + 1;
        }

        if child == end - 1 {
            // SAFETY: child == end - 1 < self.len(), so it's a valid index and
            //  child == 2 * hole.pos() + 1 != hole.pos().
            unsafe { hole.move_to(child) };
        }
        pos = hole.pos();
        drop(hole);

        // SAFETY: pos is the position in the hole and was already proven to be a valid
        //  index.
        unsafe { self.sift_up(start, pos) };
    }

    /// Rebuild assuming data[0..start] is still a proper heap.
    fn rebuild_tail(&mut self, start: usize) {
        if start == self.len() {
            return;
        }

        let tail_len = self.len() - start;

        #[inline(always)]
        fn log2_fast(x: usize) -> usize {
            (usize::BITS - x.leading_zeros() - 1) as usize
        }

        // `rebuild` takes O(self.len()) operations and about 2 * self.len() comparisons
        // in the worst case while repeating `sift_up` takes O(tail_len * log(start))
        // operations and about 1 * tail_len * log_2(start) comparisons in the worst
        // case, assuming start >= tail_len. For larger heaps, the crossover point no
        // longer follows this reasoning and was determined empirically.
        let better_to_rebuild = if start < tail_len {
            true
        } else if self.len() <= 2048 {
            2 * self.len() < tail_len * log2_fast(start)
        } else {
            2 * self.len() < tail_len * 11
        };

        if better_to_rebuild {
            self.rebuild();
        } else {
            for i in start..self.len() {
                // SAFETY: The index `i` is always less than self.len().
                unsafe { self.sift_up(0, i) };
            }
        }
    }

    fn rebuild(&mut self) {
        let mut n = self.len() / 2;
        while n > 0 {
            n -= 1;
            // SAFETY: n starts from self.len() / 2 and goes down to 0. The only case
            //  when !(n < self.len()) is if self.len() == 0, but it's ruled out by the
            //  loop condition.
            unsafe { self.sift_down(n) };
        }
    }

    /// Moves all the elements of `other` into `self`, leaving `other` empty.
    pub fn append(&mut self, other: &mut Self) {
        if self.len() < other.len() {
            swap(self, other);
        }

        let start = self.data.len();

        self.data.append(&mut other.data);

        self.rebuild_tail(start);
    }

    /// Clears the heap, returning an iterator over the removed elements in heap order.
    ///
    /// Note: `.drain_sorted()` is *O*(*n* \* log(*n*)); much slower than `.drain()`.
    #[inline]
    pub fn drain_sorted(&mut self) -> UnsafeDrainSorted<'_, T, A> {
        UnsafeDrainSorted { inner: self }
    }

    /// Retains only the elements specified by the predicate, in unspecified order.
    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&T) -> bool,
    {
        // rebuild_from will be updated to the first touched element below, and the
        // rebuild will only be done for the tail.
        let mut guard = UnsafeRebuildOnDrop { rebuild_from: self.len(), heap: self };
        let mut i = 0;

        guard.heap.data.retain(|e| {
            let keep = f(e);
            if !keep && i < guard.rebuild_from {
                guard.rebuild_from = i;
            }
            i += 1;
            keep
        });
    }
}

impl<T, A: Allocator> UnsafeBinaryHeap<T, A> {
    /// Returns an iterator visiting all values in the underlying vector, in arbitrary
    /// order.
    pub fn iter(&self) -> UnsafeIter<'_, T> {
        UnsafeIter { iter: self.data.iter() }
    }

    /// Returns an iterator which retrieves elements in heap order. Consumes the heap.
    pub fn into_iter_sorted(self) -> UnsafeIntoIterSorted<T, A> {
        UnsafeIntoIterSorted { inner: self }
    }

    /// Returns the greatest item, or `None` if empty.
    #[must_use]
    // Faithful to rust-libs (`self.data.get(0)`); `.first()` is behaviorally identical
    // but would diverge from the source text. clippy::get_first allowed for fidelity.
    #[allow(clippy::get_first)]
    pub fn peek(&self) -> Option<&T> {
        self.data.get(0)
    }

    /// Returns the number of elements the heap can hold without reallocating.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.data.capacity()
    }

    /// Reserves the minimum capacity for at least `additional` more elements.
    pub fn reserve_exact(&mut self, additional: usize) {
        self.data.reserve_exact(additional);
    }

    /// Reserves capacity for at least `additional` more elements.
    pub fn reserve(&mut self, additional: usize) {
        self.data.reserve(additional);
    }

    /// Tries to reserve the minimum capacity for at least `additional` more elements.
    pub fn try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.data.try_reserve_exact(additional)
    }

    /// Tries to reserve capacity for at least `additional` more elements.
    pub fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.data.try_reserve(additional)
    }

    /// Discards as much additional capacity as possible.
    pub fn shrink_to_fit(&mut self) {
        self.data.shrink_to_fit();
    }

    /// Discards capacity with a lower bound.
    #[inline]
    pub fn shrink_to(&mut self, min_capacity: usize) {
        self.data.shrink_to(min_capacity)
    }

    /// Returns a slice of all values in the underlying vector, in arbitrary order.
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        self.data.as_slice()
    }

    /// Returns a mutable slice of all values in the underlying vector.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the slice remains a max-heap, i.e. for all indices
    /// `0 < i < slice.len()`, `slice[(i - 1) / 2] >= slice[i]`, before the borrow ends
    /// and the binary heap is used.
    #[must_use]
    pub unsafe fn as_mut_slice(&mut self) -> &mut [T] {
        self.data.as_mut_slice()
    }

    /// Consumes the heap and returns the underlying vector in arbitrary order.
    #[must_use = "`self` will be dropped if the result is not used"]
    pub fn into_vec(self) -> Vec<T, A> {
        self.into()
    }

    /// Returns a reference to the underlying allocator.
    #[inline]
    pub fn allocator(&self) -> &A {
        self.data.allocator()
    }

    /// Returns the number of items in the heap.
    #[must_use]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Checks if the heap is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clears the heap, returning an iterator over the removed elements in arbitrary
    /// order.
    #[inline]
    pub fn drain(&mut self) -> UnsafeDrain<'_, T, A> {
        UnsafeDrain { iter: self.data.drain(..) }
    }

    /// Drops all items from the heap.
    pub fn clear(&mut self) {
        self.drain();
    }
}

/// UnsafeHole represents a hole in a slice i.e., an index without valid value (because it
/// was moved from or duplicated). In drop, `UnsafeHole` restores the slice by filling the
/// hole position with the value that was originally removed. (Renamed from `Hole`.)
/// Faithful to rust-libs: `Hole` carries no allocator generic.
struct UnsafeHole<'a, T: 'a> {
    data: &'a mut [T],
    elt: ManuallyDrop<T>,
    pos: usize,
}

impl<'a, T> UnsafeHole<'a, T> {
    /// Creates a new `UnsafeHole` at index `pos`.
    ///
    /// Unsafe because pos must be within the data slice.
    #[inline]
    unsafe fn new(data: &'a mut [T], pos: usize) -> Self {
        debug_assert!(pos < data.len());
        // SAFE: pos should be inside the slice.
        let elt = unsafe { ptr::read(data.get_unchecked(pos)) };
        UnsafeHole { data, elt: ManuallyDrop::new(elt), pos }
    }

    #[inline]
    fn pos(&self) -> usize {
        self.pos
    }

    /// Returns a reference to the element removed.
    #[inline]
    fn element(&self) -> &T {
        &self.elt
    }

    /// Returns a reference to the element at `index`.
    ///
    /// Unsafe because index must be within the data slice and not equal to pos.
    #[inline]
    unsafe fn get(&self, index: usize) -> &T {
        debug_assert!(index != self.pos);
        debug_assert!(index < self.data.len());
        unsafe { self.data.get_unchecked(index) }
    }

    /// Move hole to new location.
    ///
    /// Unsafe because index must be within the data slice and not equal to pos.
    #[inline]
    unsafe fn move_to(&mut self, index: usize) {
        debug_assert!(index != self.pos);
        debug_assert!(index < self.data.len());
        unsafe {
            let ptr = self.data.as_mut_ptr();
            let index_ptr: *const _ = ptr.add(index);
            let hole_ptr = ptr.add(self.pos);
            ptr::copy_nonoverlapping(index_ptr, hole_ptr, 1);
        }
        self.pos = index;
    }
}

impl<T> Drop for UnsafeHole<'_, T> {
    #[inline]
    fn drop(&mut self) {
        // fill the hole again
        unsafe {
            let pos = self.pos;
            ptr::copy_nonoverlapping(&*self.elt, self.data.get_unchecked_mut(pos), 1);
        }
    }
}

/// An iterator over the elements of a `UnsafeBinaryHeap`. (Renamed from `Iter`.)
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct UnsafeIter<'a, T: 'a> {
    iter: std::slice::Iter<'a, T>,
}

impl<T> Default for UnsafeIter<'_, T> {
    /// Creates an empty `UnsafeIter`.
    fn default() -> Self {
        UnsafeIter { iter: Default::default() }
    }
}

impl<T: fmt::Debug> fmt::Debug for UnsafeIter<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("UnsafeIter").field(&self.iter.as_slice()).finish()
    }
}

// FIXME(#26925) Remove in favor of `#[derive(Clone)]`.
impl<T> Clone for UnsafeIter<'_, T> {
    fn clone(&self) -> Self {
        UnsafeIter { iter: self.iter.clone() }
    }
}

impl<'a, T> Iterator for UnsafeIter<'a, T> {
    type Item = &'a T;

    #[inline]
    fn next(&mut self) -> Option<&'a T> {
        self.iter.next()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }

    #[inline]
    fn last(self) -> Option<&'a T> {
        self.iter.last()
    }
}

impl<'a, T> DoubleEndedIterator for UnsafeIter<'a, T> {
    #[inline]
    fn next_back(&mut self) -> Option<&'a T> {
        self.iter.next_back()
    }
}

impl<T> ExactSizeIterator for UnsafeIter<'_, T> {
    fn is_empty(&self) -> bool {
        self.iter.is_empty()
    }
}

impl<T> FusedIterator for UnsafeIter<'_, T> {}

/// An owning iterator over the elements of a `UnsafeBinaryHeap`. (Renamed from `IntoIter`.)
#[derive(Clone)]
pub struct UnsafeIntoIter<T, A: Allocator = Global> {
    iter: std::vec::IntoIter<T, A>,
}

impl<T, A: Allocator> UnsafeIntoIter<T, A> {
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.iter.allocator()
    }
}

impl<T: fmt::Debug, A: Allocator> fmt::Debug for UnsafeIntoIter<T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("UnsafeIntoIter").field(&self.iter.as_slice()).finish()
    }
}

impl<T, A: Allocator> Iterator for UnsafeIntoIter<T, A> {
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<T> {
        self.iter.next()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<T, A: Allocator> DoubleEndedIterator for UnsafeIntoIter<T, A> {
    #[inline]
    fn next_back(&mut self) -> Option<T> {
        self.iter.next_back()
    }
}

impl<T, A: Allocator> ExactSizeIterator for UnsafeIntoIter<T, A> {
    fn is_empty(&self) -> bool {
        self.iter.is_empty()
    }
}

impl<T, A: Allocator> FusedIterator for UnsafeIntoIter<T, A> {}

impl<T> Default for UnsafeIntoIter<T> {
    /// Creates an empty `UnsafeIntoIter`.
    fn default() -> Self {
        UnsafeIntoIter { iter: Default::default() }
    }
}

// NOT brought across (ProcessCommentingStandard reason — std-internal in-place-collect
// markers): the upstream `IntoIter` also impls `TrustedFused`, `SourceIter`,
// `InPlaceIterable`, and (cfg(not(test))) `AsVecIntoIter`. These are `#[doc(hidden)]`
// perma-unstable specialization hooks that let `Vec`-sourced iterators collect in
// place; no public method, test, or bench exercises them, and `AsVecIntoIter` is not
// exported from `alloc`, so it cannot be named — let alone implemented — outside the
// standard library. Faithful corpse, left commented rather than silently dropped:
//
//   unsafe impl<T, A: Allocator> TrustedFused for UnsafeIntoIter<T, A> {}
//   unsafe impl<T, A: Allocator> SourceIter for UnsafeIntoIter<T, A> {
//       type Source = UnsafeIntoIter<T, A>;
//       unsafe fn as_inner(&mut self) -> &mut Self::Source { self }
//   }
//   unsafe impl<I, A: Allocator> InPlaceIterable for UnsafeIntoIter<I, A> {
//       const EXPAND_BY: Option<NonZero<usize>> = NonZero::new(1);
//       const MERGE_BY: Option<NonZero<usize>> = NonZero::new(1);
//   }
//   unsafe impl<I> AsVecIntoIter for UnsafeIntoIter<I> {
//       type Item = I;
//       fn as_into_iter(&mut self) -> &mut vec::IntoIter<Self::Item> { &mut self.iter }
//   }

/// An iterator that retrieves elements in heap (sorted) order, consuming the heap.
/// (Renamed from `IntoIterSorted`.)
#[must_use = "iterators are lazy and do nothing unless consumed"]
#[derive(Clone, Debug)]
pub struct UnsafeIntoIterSorted<T, A: Allocator = Global> {
    inner: UnsafeBinaryHeap<T, A>,
}

impl<T, A: Allocator> UnsafeIntoIterSorted<T, A> {
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.inner.allocator()
    }
}

impl<T: Ord, A: Allocator> Iterator for UnsafeIntoIterSorted<T, A> {
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<T> {
        self.inner.pop()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let exact = self.inner.len();
        (exact, Some(exact))
    }
}

impl<T: Ord, A: Allocator> ExactSizeIterator for UnsafeIntoIterSorted<T, A> {}

impl<T: Ord, A: Allocator> FusedIterator for UnsafeIntoIterSorted<T, A> {}

unsafe impl<T: Ord, A: Allocator> TrustedLen for UnsafeIntoIterSorted<T, A> {}

/// A draining iterator over the elements of a `UnsafeBinaryHeap`. (Renamed from `Drain`.)
#[derive(Debug)]
pub struct UnsafeDrain<'a, T: 'a, A: Allocator = Global> {
    iter: std::vec::Drain<'a, T, A>,
}

impl<T, A: Allocator> UnsafeDrain<'_, T, A> {
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.iter.allocator()
    }
}

impl<T, A: Allocator> Iterator for UnsafeDrain<'_, T, A> {
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<T> {
        self.iter.next()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<T, A: Allocator> DoubleEndedIterator for UnsafeDrain<'_, T, A> {
    #[inline]
    fn next_back(&mut self) -> Option<T> {
        self.iter.next_back()
    }
}

impl<T, A: Allocator> ExactSizeIterator for UnsafeDrain<'_, T, A> {
    fn is_empty(&self) -> bool {
        self.iter.is_empty()
    }
}

impl<T, A: Allocator> FusedIterator for UnsafeDrain<'_, T, A> {}

/// A draining iterator over the elements of a `UnsafeBinaryHeap` in heap (sorted) order.
/// (Renamed from `DrainSorted`.)
#[derive(Debug)]
pub struct UnsafeDrainSorted<'a, T: Ord, A: Allocator = Global> {
    inner: &'a mut UnsafeBinaryHeap<T, A>,
}

impl<'a, T: Ord, A: Allocator> UnsafeDrainSorted<'a, T, A> {
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.inner.allocator()
    }
}

impl<'a, T: Ord, A: Allocator> Drop for UnsafeDrainSorted<'a, T, A> {
    /// Removes heap elements in heap order.
    fn drop(&mut self) {
        struct DropGuard<'r, 'a, T: Ord, A: Allocator>(&'r mut UnsafeDrainSorted<'a, T, A>);

        impl<'r, 'a, T: Ord, A: Allocator> Drop for DropGuard<'r, 'a, T, A> {
            fn drop(&mut self) {
                while self.0.inner.pop().is_some() {}
            }
        }

        while let Some(item) = self.inner.pop() {
            let guard = DropGuard(self);
            drop(item);
            mem::forget(guard);
        }
    }
}

impl<T: Ord, A: Allocator> Iterator for UnsafeDrainSorted<'_, T, A> {
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<T> {
        self.inner.pop()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let exact = self.inner.len();
        (exact, Some(exact))
    }
}

impl<T: Ord, A: Allocator> ExactSizeIterator for UnsafeDrainSorted<'_, T, A> {}

impl<T: Ord, A: Allocator> FusedIterator for UnsafeDrainSorted<'_, T, A> {}

unsafe impl<T: Ord, A: Allocator> TrustedLen for UnsafeDrainSorted<'_, T, A> {}

impl<T: Ord, A: Allocator> From<Vec<T, A>> for UnsafeBinaryHeap<T, A> {
    /// Converts a `Vec<T>` into a `UnsafeBinaryHeap<T>`, in-place, *O*(*n*).
    fn from(vec: Vec<T, A>) -> UnsafeBinaryHeap<T, A> {
        let mut heap = UnsafeBinaryHeap { data: vec };
        heap.rebuild();
        heap
    }
}

impl<T: Ord, const N: usize> From<[T; N]> for UnsafeBinaryHeap<T> {
    fn from(arr: [T; N]) -> Self {
        Self::from_iter(arr)
    }
}

impl<T, A: Allocator> From<UnsafeBinaryHeap<T, A>> for Vec<T, A> {
    /// Converts a `UnsafeBinaryHeap<T>` into a `Vec<T>`. No data movement or allocation,
    /// constant time.
    fn from(heap: UnsafeBinaryHeap<T, A>) -> Vec<T, A> {
        heap.data
    }
}

impl<T: Ord> FromIterator<T> for UnsafeBinaryHeap<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> UnsafeBinaryHeap<T> {
        UnsafeBinaryHeap::from(iter.into_iter().collect::<Vec<_>>())
    }
}

impl<T, A: Allocator> IntoIterator for UnsafeBinaryHeap<T, A> {
    type Item = T;
    type IntoIter = UnsafeIntoIter<T, A>;

    /// Creates a consuming iterator that moves each value out of the heap in arbitrary
    /// order. The heap cannot be used after calling this.
    fn into_iter(self) -> UnsafeIntoIter<T, A> {
        UnsafeIntoIter { iter: self.data.into_iter() }
    }
}

impl<'a, T, A: Allocator> IntoIterator for &'a UnsafeBinaryHeap<T, A> {
    type Item = &'a T;
    type IntoIter = UnsafeIter<'a, T>;

    fn into_iter(self) -> UnsafeIter<'a, T> {
        self.iter()
    }
}

impl<T: Ord, A: Allocator> Extend<T> for UnsafeBinaryHeap<T, A> {
    #[inline]
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        let guard = UnsafeRebuildOnDrop { rebuild_from: self.len(), heap: self };
        guard.heap.data.extend(iter);
    }

    #[inline]
    fn extend_one(&mut self, item: T) {
        self.push(item);
    }

    #[inline]
    fn extend_reserve(&mut self, additional: usize) {
        self.reserve(additional);
    }
}

impl<'a, T: 'a + Ord + Copy, A: Allocator> Extend<&'a T> for UnsafeBinaryHeap<T, A> {
    fn extend<I: IntoIterator<Item = &'a T>>(&mut self, iter: I) {
        self.extend(iter.into_iter().cloned());
    }

    #[inline]
    fn extend_one(&mut self, &item: &'a T) {
        self.push(item);
    }

    #[inline]
    fn extend_reserve(&mut self, additional: usize) {
        self.reserve(additional);
    }
}
