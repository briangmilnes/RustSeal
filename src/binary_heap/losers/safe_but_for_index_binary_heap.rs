// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! SafeButForIndexBinaryHeap — `safe_opt_binary_heap::SafeOptBinaryHeap` with ONE change:
//! every element access is **unchecked**. This is an experiment variant to isolate the cost
//! of the bounds checks from the cost of the swap-vs-hole algorithm.
//!
//! `safe_opt` is the *O*(log n)-`peek_mut`, forget-guarantee-dropped safe heap. It keeps two
//! "safe-tax" costs relative to the faithful `unsafe_binary_heap`: (a) it sifts with
//! `Vec::swap` (two moves per step) instead of the `UnsafeHole` (one move per step), and
//! (b) every `data[i]` is bounds-checked. This variant removes ONLY (b): each `data[i]` ->
//! `get_unchecked(i)` / `get_unchecked_mut(i)`, and each `Vec::swap` -> `swap_unchecked`.
//! The swap-vs-hole structure (a) is unchanged. So:
//!
//!   * `unsafe`              vs `safe_but_for_index`  = the swap-vs-hole (2 moves vs 1) cost.
//!   * `safe_but_for_index`  vs `safe_opt`            = the bounds-check cost.
//!
//! It is therefore NOT a safe heap — it has real `unsafe` blocks at every index — and its
//! `peek_mut` keeps `safe_opt`'s dropped forget guarantee (`test_peek_mut_leek` `#[ignore]`d).
//! `swap_unchecked` is unstable (`feature(slice_swap_unchecked)`, unlocked via
//! RUSTC_BOOTSTRAP=1, declared in lib.rs).

use core::iter::{FusedIterator, TrustedLen};
use core::mem::swap;
use core::ops::{Deref, DerefMut};
use core::fmt;
use std::alloc::{Allocator, Global};
use std::collections::TryReserveError;

/// A priority queue implemented with a binary (max-)heap — safe re-implementation.
pub struct SafeButForIndexBinaryHeap<T, A: Allocator = Global> {
    data: Vec<T, A>,
}

/// Mutable-greatest-element guard for `SafeButForIndexBinaryHeap`. Created by
/// [`SafeButForIndexBinaryHeap::peek_mut`].
///
/// The greatest element stays in place at `data[0]`; the guard derefs to it. `deref_mut`
/// just returns `&mut data[0]` (*O*(1)) and records that the root may have changed; on
/// drop, if it was mutated, one `sift_down(0)` (*O*(log n)) restores the heap. UNLIKE
/// `SafeBinaryHeap`, the rest of the heap is NOT held aside — so forgetting the guard after
/// a mutation skips the sift and leaves the heap in broken order (the dropped guarantee).
pub struct SafeButForIndexPeekMut<'a, T: 'a + Ord, A: Allocator = Global> {
    heap: &'a mut SafeButForIndexBinaryHeap<T, A>,
    // Set by `deref_mut`; tells `Drop` the root may have been mutated and needs sifting.
    sift_on_drop: bool,
}

impl<T: Ord + fmt::Debug, A: Allocator> fmt::Debug for SafeButForIndexPeekMut<'_, T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // SAFETY: a SafeButForIndexPeekMut is only created for a non-empty heap.
        f.debug_tuple("SafeButForIndexPeekMut").field(unsafe { self.heap.data.get_unchecked(0) }).finish()
    }
}

impl<T: Ord, A: Allocator> Drop for SafeButForIndexPeekMut<'_, T, A> {
    fn drop(&mut self) {
        // If the root was handed out mutably, re-sift it into place — O(log n). Skipped on a
        // read-only peek. (If the guard is forgotten instead of dropped, this never runs and
        // a mutated root is left unsorted: the deliberately-dropped forget guarantee.)
        if self.sift_on_drop && !self.heap.is_empty() {
            self.heap.sift_down(0);
        }
    }
}

impl<T: Ord, A: Allocator> Deref for SafeButForIndexPeekMut<'_, T, A> {
    type Target = T;
    fn deref(&self) -> &T {
        debug_assert!(!self.heap.is_empty());
        // SAFETY: only created for a non-empty heap, so index 0 is in bounds.
        unsafe { self.heap.data.get_unchecked(0) }
    }
}

impl<T: Ord, A: Allocator> DerefMut for SafeButForIndexPeekMut<'_, T, A> {
    fn deref_mut(&mut self) -> &mut T {
        debug_assert!(!self.heap.is_empty());
        // O(1): no leak amplification, no tail-split, no `A: Clone`. The trade is that
        // forgetting the guard after this skips the Drop sift and leaves the heap unsorted.
        self.sift_on_drop = true;
        // SAFETY: only created for a non-empty heap, so index 0 is in bounds.
        unsafe { self.heap.data.get_unchecked_mut(0) }
    }
}

impl<'a, T: Ord, A: Allocator> SafeButForIndexPeekMut<'a, T, A> {
    /// Sifts the current element to its new position. Afterwards refers to the new
    /// element. Returns whether the maximum changed.
    #[must_use = "is equivalent to dropping and getting a new SafeButForIndexPeekMut except for return information"]
    pub fn refresh(&mut self) -> bool {
        // Sift the (possibly mutated) root down; it changed iff it moved off index 0. The
        // sift is done now, so `Drop` need not repeat it.
        let moved = self.heap.sift_down(0) != 0;
        self.sift_on_drop = false;
        moved
    }

    /// Removes the peeked value from the heap and returns it.
    pub fn pop(mut this: SafeButForIndexPeekMut<'a, T, A>) -> T {
        // Remove the (possibly mutated) root and re-heapify the rest; suppress the Drop sift.
        this.sift_on_drop = false;
        let val = this.heap.data.swap_remove(0);
        if !this.heap.is_empty() {
            this.heap.sift_down(0);
        }
        val
    }
}

impl<T: Clone, A: Allocator + Clone> Clone for SafeButForIndexBinaryHeap<T, A> {
    fn clone(&self) -> Self {
        SafeButForIndexBinaryHeap { data: self.data.clone() }
    }

    fn clone_from(&mut self, source: &Self) {
        self.data.clone_from(&source.data);
    }
}

impl<T> Default for SafeButForIndexBinaryHeap<T> {
    /// Creates an empty `SafeButForIndexBinaryHeap<T>`.
    #[inline]
    fn default() -> SafeButForIndexBinaryHeap<T> {
        SafeButForIndexBinaryHeap::new()
    }
}

impl<T: fmt::Debug, A: Allocator> fmt::Debug for SafeButForIndexBinaryHeap<T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

/// Drop guard that repairs the heap tail when an in-place mutation (extend / retain)
/// finishes or unwinds.
struct SafeButForIndexRebuildOnDrop<'a, T: Ord, A: Allocator = Global> {
    heap: &'a mut SafeButForIndexBinaryHeap<T, A>,
    rebuild_from: usize,
}

impl<T: Ord, A: Allocator> Drop for SafeButForIndexRebuildOnDrop<'_, T, A> {
    fn drop(&mut self) {
        self.heap.rebuild_tail(self.rebuild_from);
    }
}

impl<T> SafeButForIndexBinaryHeap<T> {
    /// Creates an empty `SafeButForIndexBinaryHeap` as a max-heap.
    #[must_use]
    pub const fn new() -> SafeButForIndexBinaryHeap<T> {
        SafeButForIndexBinaryHeap { data: Vec::new() }
    }

    /// Creates an empty `SafeButForIndexBinaryHeap` with at least the specified capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> SafeButForIndexBinaryHeap<T> {
        SafeButForIndexBinaryHeap { data: Vec::with_capacity(capacity) }
    }
}

impl<T, A: Allocator> SafeButForIndexBinaryHeap<T, A> {
    /// Creates an empty `SafeButForIndexBinaryHeap` as a max-heap, using `A` as allocator.
    #[must_use]
    pub const fn new_in(alloc: A) -> SafeButForIndexBinaryHeap<T, A> {
        SafeButForIndexBinaryHeap { data: Vec::new_in(alloc) }
    }

    /// Creates an empty `SafeButForIndexBinaryHeap` with at least the specified capacity, using `A`.
    #[must_use]
    pub fn with_capacity_in(capacity: usize, alloc: A) -> SafeButForIndexBinaryHeap<T, A> {
        SafeButForIndexBinaryHeap { data: Vec::with_capacity_in(capacity, alloc) }
    }

    /// Creates a `SafeButForIndexBinaryHeap` from the supplied `vec` without rebuilding it.
    ///
    /// Logically `vec` must already be a max-heap; unlike the unsafe heap this is a SAFE
    /// fn (a non-heap input only produces wrong results, never undefined behavior).
    #[must_use]
    pub fn from_raw_vec(vec: Vec<T, A>) -> SafeButForIndexBinaryHeap<T, A> {
        SafeButForIndexBinaryHeap { data: vec }
    }
}

impl<T: Ord, A: Allocator> SafeButForIndexBinaryHeap<T, A> {
    /// Returns a mutable reference to the greatest item, or `None` if empty.
    pub fn peek_mut(&mut self) -> Option<SafeButForIndexPeekMut<'_, T, A>> {
        if self.is_empty() {
            None
        } else {
            Some(SafeButForIndexPeekMut { heap: self, sift_on_drop: false })
        }
    }

    /// Removes the greatest item and returns it, or `None` if empty.
    pub fn pop(&mut self) -> Option<T> {
        let mut item = self.data.pop()?;
        if !self.is_empty() {
            // SAFETY: !is_empty() means index 0 is in bounds.
            unsafe { swap(&mut item, self.data.get_unchecked_mut(0)) };
            self.sift_down_to_bottom(0);
        }
        Some(item)
    }

    /// Removes and returns the greatest item if `predicate` returns `true`, else `None`.
    pub fn pop_if(&mut self, predicate: impl FnOnce(&T) -> bool) -> Option<T> {
        let first = self.peek()?;
        if predicate(first) { self.pop() } else { None }
    }

    /// Pushes an item onto the heap.
    pub fn push(&mut self, item: T) {
        let old_len = self.len();
        self.data.push(item);
        self.sift_up(0, old_len);
    }

    /// Consumes the heap and returns a vector in sorted (ascending) order.
    #[must_use = "`self` will be dropped if the result is not used"]
    pub fn into_sorted_vec(mut self) -> Vec<T, A> {
        let mut end = self.len();
        while end > 1 {
            end -= 1;
            // SAFETY: 0 < end < self.len(), both in bounds.
            unsafe { self.data.swap_unchecked(0, end) };
            self.sift_down_range(0, end);
        }
        self.into_vec()
    }

    // --- sift family: swap-based (no hole), indices UNCHECKED (the one thing this
    // variant un-safes: every `data[i]` and `Vec::swap` is replaced by `get_unchecked`
    // and `swap_unchecked`. The swap-vs-hole structure is unchanged from safe_opt). ---

    /// Move the element at `pos` toward the root while it exceeds its parent. Returns the
    /// final index.
    fn sift_up(&mut self, start: usize, mut pos: usize) -> usize {
        while pos > start {
            let parent = (pos - 1) / 2;
            // SAFETY: pos < self.len() (caller invariant) and parent < pos, both in bounds.
            if unsafe { self.data.get_unchecked(pos) <= self.data.get_unchecked(parent) } {
                break;
            }
            // SAFETY: same as above.
            unsafe { self.data.swap_unchecked(pos, parent) };
            pos = parent;
        }
        pos
    }

    /// Move the element at `pos` down within `[pos, end)` while smaller than its greater
    /// child. Returns the final index.
    fn sift_down_range(&mut self, mut pos: usize, end: usize) -> usize {
        let mut child = 2 * pos + 1;
        while child < end {
            // pick the greater of the two children
            let right = child + 1;
            // SAFETY: child < end <= self.len(), and right < end when the && short-circuits.
            if right < end && unsafe { self.data.get_unchecked(child) <= self.data.get_unchecked(right) } {
                child = right;
            }
            // SAFETY: pos < child < end <= self.len(), both in bounds.
            if unsafe { self.data.get_unchecked(pos) >= self.data.get_unchecked(child) } {
                return pos;
            }
            // SAFETY: same as above.
            unsafe { self.data.swap_unchecked(pos, child) };
            pos = child;
            child = 2 * pos + 1;
        }
        pos
    }

    fn sift_down(&mut self, pos: usize) -> usize {
        let len = self.len();
        self.sift_down_range(pos, len)
    }

    /// The unsafe heap has a bottom-then-up variant exploiting the hole; for the safe heap
    /// a plain `sift_down` is correct and simpler.
    fn sift_down_to_bottom(&mut self, pos: usize) -> usize {
        self.sift_down(pos)
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
                self.sift_up(0, i);
            }
        }
    }

    fn rebuild(&mut self) {
        let mut n = self.len() / 2;
        while n > 0 {
            n -= 1;
            self.sift_down(n);
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
    #[inline]
    pub fn drain_sorted(&mut self) -> SafeButForIndexDrainSorted<'_, T, A> {
        SafeButForIndexDrainSorted { inner: self }
    }

    /// Retains only the elements specified by the predicate, in unspecified order.
    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&T) -> bool,
    {
        let mut guard = SafeButForIndexRebuildOnDrop { rebuild_from: self.len(), heap: self };
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

impl<T, A: Allocator> SafeButForIndexBinaryHeap<T, A> {
    /// Returns an iterator visiting all values in the underlying vector, in arbitrary
    /// order.
    pub fn iter(&self) -> SafeButForIndexIter<'_, T> {
        SafeButForIndexIter { iter: self.data.iter() }
    }

    /// Returns an iterator which retrieves elements in heap order. Consumes the heap.
    pub fn into_iter_sorted(self) -> SafeButForIndexIntoIterSorted<T, A> {
        SafeButForIndexIntoIterSorted { inner: self }
    }

    /// Returns the greatest item, or `None` if empty.
    #[must_use]
    pub fn peek(&self) -> Option<&T> {
        self.data.first()
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
    /// Logically the caller must keep it a max-heap; unlike the unsafe heap this is SAFE
    /// (misuse only produces wrong results, never undefined behavior).
    #[must_use]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
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
    pub fn drain(&mut self) -> SafeButForIndexDrain<'_, T, A> {
        SafeButForIndexDrain { iter: self.data.drain(..) }
    }

    /// Drops all items from the heap.
    pub fn clear(&mut self) {
        self.drain();
    }
}

/// An iterator over the elements of a `SafeButForIndexBinaryHeap`.
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct SafeButForIndexIter<'a, T: 'a> {
    iter: std::slice::Iter<'a, T>,
}

impl<T> Default for SafeButForIndexIter<'_, T> {
    /// Creates an empty `SafeButForIndexIter`.
    fn default() -> Self {
        SafeButForIndexIter { iter: Default::default() }
    }
}

impl<T: fmt::Debug> fmt::Debug for SafeButForIndexIter<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SafeButForIndexIter").field(&self.iter.as_slice()).finish()
    }
}

impl<T> Clone for SafeButForIndexIter<'_, T> {
    fn clone(&self) -> Self {
        SafeButForIndexIter { iter: self.iter.clone() }
    }
}

impl<'a, T> Iterator for SafeButForIndexIter<'a, T> {
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

impl<'a, T> DoubleEndedIterator for SafeButForIndexIter<'a, T> {
    #[inline]
    fn next_back(&mut self) -> Option<&'a T> {
        self.iter.next_back()
    }
}

impl<T> ExactSizeIterator for SafeButForIndexIter<'_, T> {
    fn is_empty(&self) -> bool {
        self.iter.is_empty()
    }
}

impl<T> FusedIterator for SafeButForIndexIter<'_, T> {}

/// An owning iterator over the elements of a `SafeButForIndexBinaryHeap`.
#[derive(Clone)]
pub struct SafeButForIndexIntoIter<T, A: Allocator = Global> {
    iter: std::vec::IntoIter<T, A>,
}

impl<T, A: Allocator> SafeButForIndexIntoIter<T, A> {
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.iter.allocator()
    }
}

impl<T: fmt::Debug, A: Allocator> fmt::Debug for SafeButForIndexIntoIter<T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SafeButForIndexIntoIter").field(&self.iter.as_slice()).finish()
    }
}

impl<T, A: Allocator> Iterator for SafeButForIndexIntoIter<T, A> {
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

impl<T, A: Allocator> DoubleEndedIterator for SafeButForIndexIntoIter<T, A> {
    #[inline]
    fn next_back(&mut self) -> Option<T> {
        self.iter.next_back()
    }
}

impl<T, A: Allocator> ExactSizeIterator for SafeButForIndexIntoIter<T, A> {
    fn is_empty(&self) -> bool {
        self.iter.is_empty()
    }
}

impl<T, A: Allocator> FusedIterator for SafeButForIndexIntoIter<T, A> {}

impl<T> Default for SafeButForIndexIntoIter<T> {
    /// Creates an empty `SafeButForIndexIntoIter`.
    fn default() -> Self {
        SafeButForIndexIntoIter { iter: Default::default() }
    }
}

/// An iterator that retrieves elements in heap (sorted) order, consuming the heap.
#[must_use = "iterators are lazy and do nothing unless consumed"]
#[derive(Clone, Debug)]
pub struct SafeButForIndexIntoIterSorted<T, A: Allocator = Global> {
    inner: SafeButForIndexBinaryHeap<T, A>,
}

impl<T, A: Allocator> SafeButForIndexIntoIterSorted<T, A> {
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.inner.allocator()
    }
}

impl<T: Ord, A: Allocator> Iterator for SafeButForIndexIntoIterSorted<T, A> {
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

impl<T: Ord, A: Allocator> ExactSizeIterator for SafeButForIndexIntoIterSorted<T, A> {}

impl<T: Ord, A: Allocator> FusedIterator for SafeButForIndexIntoIterSorted<T, A> {}

// `TrustedLen` is an unsafe PROMISE that `size_hint` is exact: lower == upper == the
// remaining count, and `next` yields exactly that many items. Length-driven consumers
// (`extend`/`collect`/`zip`) rely on it to reserve the exact capacity once and write with
// UNCHECKED stores + `set_len`, skipping per-item capacity/bounds checks — so a lying
// impl would write out of bounds (UB), which is why it is `unsafe impl`. Sound here:
// `next` is `pop` (one item per heap element) and `size_hint` is `(len, Some(len))`, so
// the count is exact. This length promise is the ONLY `unsafe` token in the safe heap; it
// does no memory-unsafe work itself.
unsafe impl<T: Ord, A: Allocator> TrustedLen for SafeButForIndexIntoIterSorted<T, A> {}

/// A draining iterator over the elements of a `SafeButForIndexBinaryHeap`.
#[derive(Debug)]
pub struct SafeButForIndexDrain<'a, T: 'a, A: Allocator = Global> {
    iter: std::vec::Drain<'a, T, A>,
}

impl<T, A: Allocator> SafeButForIndexDrain<'_, T, A> {
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.iter.allocator()
    }
}

impl<T, A: Allocator> Iterator for SafeButForIndexDrain<'_, T, A> {
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

impl<T, A: Allocator> DoubleEndedIterator for SafeButForIndexDrain<'_, T, A> {
    #[inline]
    fn next_back(&mut self) -> Option<T> {
        self.iter.next_back()
    }
}

impl<T, A: Allocator> ExactSizeIterator for SafeButForIndexDrain<'_, T, A> {
    fn is_empty(&self) -> bool {
        self.iter.is_empty()
    }
}

impl<T, A: Allocator> FusedIterator for SafeButForIndexDrain<'_, T, A> {}

/// A draining iterator over the elements of a `SafeButForIndexBinaryHeap` in heap (sorted) order.
#[derive(Debug)]
pub struct SafeButForIndexDrainSorted<'a, T: Ord, A: Allocator = Global> {
    inner: &'a mut SafeButForIndexBinaryHeap<T, A>,
}

impl<'a, T: Ord, A: Allocator> SafeButForIndexDrainSorted<'a, T, A> {
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.inner.allocator()
    }
}

impl<'a, T: Ord, A: Allocator> Drop for SafeButForIndexDrainSorted<'a, T, A> {
    /// Removes heap elements in heap order.
    fn drop(&mut self) {
        struct DropGuard<'r, 'a, T: Ord, A: Allocator>(&'r mut SafeButForIndexDrainSorted<'a, T, A>);

        impl<'r, 'a, T: Ord, A: Allocator> Drop for DropGuard<'r, 'a, T, A> {
            fn drop(&mut self) {
                while self.0.inner.pop().is_some() {}
            }
        }

        while let Some(item) = self.inner.pop() {
            let guard = DropGuard(self);
            drop(item);
            core::mem::forget(guard);
        }
    }
}

impl<T: Ord, A: Allocator> Iterator for SafeButForIndexDrainSorted<'_, T, A> {
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

impl<T: Ord, A: Allocator> ExactSizeIterator for SafeButForIndexDrainSorted<'_, T, A> {}

impl<T: Ord, A: Allocator> FusedIterator for SafeButForIndexDrainSorted<'_, T, A> {}

// Sound exact-size assertion; see the SafeButForIndexIntoIterSorted note above.
unsafe impl<T: Ord, A: Allocator> TrustedLen for SafeButForIndexDrainSorted<'_, T, A> {}

impl<T: Ord, A: Allocator> From<Vec<T, A>> for SafeButForIndexBinaryHeap<T, A> {
    /// Converts a `Vec<T>` into a `SafeButForIndexBinaryHeap<T>`, in-place, *O*(*n*).
    fn from(vec: Vec<T, A>) -> SafeButForIndexBinaryHeap<T, A> {
        let mut heap = SafeButForIndexBinaryHeap { data: vec };
        heap.rebuild();
        heap
    }
}

impl<T: Ord, const N: usize> From<[T; N]> for SafeButForIndexBinaryHeap<T> {
    fn from(arr: [T; N]) -> Self {
        Self::from_iter(arr)
    }
}

impl<T, A: Allocator> From<SafeButForIndexBinaryHeap<T, A>> for Vec<T, A> {
    /// Converts a `SafeButForIndexBinaryHeap<T>` into a `Vec<T>`. No data movement or allocation,
    /// constant time.
    fn from(heap: SafeButForIndexBinaryHeap<T, A>) -> Vec<T, A> {
        heap.data
    }
}

impl<T: Ord> FromIterator<T> for SafeButForIndexBinaryHeap<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> SafeButForIndexBinaryHeap<T> {
        SafeButForIndexBinaryHeap::from(iter.into_iter().collect::<Vec<_>>())
    }
}

impl<T, A: Allocator> IntoIterator for SafeButForIndexBinaryHeap<T, A> {
    type Item = T;
    type IntoIter = SafeButForIndexIntoIter<T, A>;

    /// Creates a consuming iterator that moves each value out of the heap in arbitrary
    /// order. The heap cannot be used after calling this.
    fn into_iter(self) -> SafeButForIndexIntoIter<T, A> {
        SafeButForIndexIntoIter { iter: self.data.into_iter() }
    }
}

impl<'a, T, A: Allocator> IntoIterator for &'a SafeButForIndexBinaryHeap<T, A> {
    type Item = &'a T;
    type IntoIter = SafeButForIndexIter<'a, T>;

    fn into_iter(self) -> SafeButForIndexIter<'a, T> {
        self.iter()
    }
}

impl<T: Ord, A: Allocator> Extend<T> for SafeButForIndexBinaryHeap<T, A> {
    #[inline]
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        let guard = SafeButForIndexRebuildOnDrop { rebuild_from: self.len(), heap: self };
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

impl<'a, T: 'a + Ord + Copy, A: Allocator> Extend<&'a T> for SafeButForIndexBinaryHeap<T, A> {
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
