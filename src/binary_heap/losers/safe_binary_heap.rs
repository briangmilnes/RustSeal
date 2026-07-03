// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! SafeBinaryHeap — a SAFE re-implementation of the same binary-heap / priority-queue,
//! offering the exact public surface of `unsafe_binary_heap::UnsafeBinaryHeap` (which is
//! the faithful rust-libs extraction). This version contains **zero `unsafe` blocks**:
//! every place the unsafe heap reaches for raw memory is replaced by a checked, safe
//! operation, so it is the natural target for verus verification (no raw pointers,
//! `ManuallyDrop`, or `set_len` to model).
//!
//! How the unsafe operations are replaced (cf. the unsafe inventory):
//!   * the `UnsafeHole` move-without-drop shuffle  ->  `Vec::swap` (two moves per step
//!     instead of one — the measurable cost the benches expose);
//!   * `get_unchecked` / unsafe `sift_*` bounds elision  ->  checked indexing `data[i]`;
//!   * `UnsafePeekMut`'s `set_len` "leak amplification"  ->  a tail-split `SafePeekMut`:
//!     `deref_mut` splits the tail `data[1..]` off into the guard, leaving `[root]`; drop
//!     re-attaches and re-sifts, and forgetting the guard leaks the tail but leaves the
//!     valid single-element heap `[root]` — forget-robust with no `set_len`;
//!   * `unwrap_unchecked`  ->  `expect`.
//!
//! The ONLY `unsafe` tokens remaining are the two `unsafe impl TrustedLen` marker
//! assertions on the sorted iterators (an exact-`size_hint` claim, identical to std and
//! to the unsafe heap, carrying no memory-safety reasoning) — kept so the shared test
//! suite's `TrustedLen` bound applies to both heaps. There are no `unsafe` *blocks*.
//!
//! Behavioral note: `peek_mut` is *O*(1); the first `deref_mut` of a len > 1 heap splits
//! the tail (*O*(n) move + alloc) where the unsafe/std heap only adjusts a length (*O*(1)).
//! Externally observable results match; the cost difference is intentional and shows up in
//! `bench_*peek_mut*`.

use core::iter::{FusedIterator, TrustedLen};
use core::mem::swap;
use core::ops::{Deref, DerefMut};
use core::fmt;
use std::alloc::{Allocator, Global};
use std::collections::TryReserveError;

/// A priority queue implemented with a binary (max-)heap — safe re-implementation.
pub struct SafeBinaryHeap<T, A: Allocator = Global> {
    data: Vec<T, A>,
}

/// Mutable-greatest-element guard for `SafeBinaryHeap`. Created by [`SafeBinaryHeap::peek_mut`].
///
/// The greatest element stays in place at `data[0]`; the guard derefs to it. This is the
/// SAFE analog of `UnsafePeekMut`'s `set_len` "leak amplification": on the first
/// `deref_mut` of a heap with more than one element, the tail `data[1..]` is split off
/// and held in the guard, leaving the heap as just `[root]`. On drop the tail is appended
/// back and the (possibly mutated) root sifted down. If the guard is forgotten, the tail
/// is dropped with it and the heap is left as the valid single-element heap `[root]` — so
/// forgetting after a mutation can never leave a broken heap, with no `set_len`.
pub struct SafePeekMut<'a, T: 'a + Ord, A: Allocator = Global> {
    heap: &'a mut SafeBinaryHeap<T, A>,
    // `Some` once `deref_mut` has split the tail off (only for heaps of len > 1).
    tail: Option<Vec<T, A>>,
}

impl<T: Ord + fmt::Debug, A: Allocator> fmt::Debug for SafePeekMut<'_, T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SafePeekMut").field(&self.heap.data[0]).finish()
    }
}

impl<T: Ord, A: Allocator> Drop for SafePeekMut<'_, T, A> {
    fn drop(&mut self) {
        // If the tail was split off (a mutation happened on a len > 1 heap), put it back
        // and sift the root down to restore the heap. Otherwise the heap is already valid
        // (read-only peek, or a single-element heap whose lone element is trivially a
        // valid heap whatever its value).
        if let Some(mut tail) = self.tail.take() {
            self.heap.data.append(&mut tail);
            self.heap.sift_down(0);
        }
    }
}

impl<T: Ord, A: Allocator> Deref for SafePeekMut<'_, T, A> {
    type Target = T;
    fn deref(&self) -> &T {
        debug_assert!(!self.heap.is_empty());
        &self.heap.data[0]
    }
}

// `A: Clone` is required only here: the safe leak-amplification splits the tail into a new
// `Vec<T, A>`, and `Vec::split_off` clones the allocator. (`Drop`/`pop`/`refresh` use
// `append`, which does not.) The default `Global` is `Clone`, so mutating through the guard
// works for the ordinary heap; only a custom non-`Clone` allocator loses `deref_mut`.
impl<T: Ord, A: Allocator + Clone> DerefMut for SafePeekMut<'_, T, A> {
    fn deref_mut(&mut self) -> &mut T {
        debug_assert!(!self.heap.is_empty());
        // Leak-amplify (safely): split the tail out so that forgetting the guard after
        // mutating the root leaves only the valid single-element heap `[root]`.
        if self.tail.is_none() && self.heap.len() > 1 {
            self.tail = Some(self.heap.data.split_off(1));
        }
        &mut self.heap.data[0]
    }
}

impl<'a, T: Ord, A: Allocator> SafePeekMut<'a, T, A> {
    /// Sifts the current element to its new position. Afterwards refers to the new
    /// element. Returns whether the maximum changed.
    #[must_use = "is equivalent to dropping and getting a new SafePeekMut except for return information"]
    pub fn refresh(&mut self) -> bool {
        // Re-attach the tail (if split), then sift the root down; it changed iff the root
        // moved off index 0.
        if let Some(mut tail) = self.tail.take() {
            self.heap.data.append(&mut tail);
        }
        self.heap.sift_down(0) != 0
    }

    /// Removes the peeked value from the heap and returns it.
    pub fn pop(mut this: SafePeekMut<'a, T, A>) -> T {
        // Re-attach the tail, then remove the root and re-heapify the rest. `this`'s Drop
        // then sees `tail == None` and does nothing.
        if let Some(mut tail) = this.tail.take() {
            this.heap.data.append(&mut tail);
        }
        let val = this.heap.data.swap_remove(0);
        if !this.heap.is_empty() {
            this.heap.sift_down(0);
        }
        val
    }
}

impl<T: Clone, A: Allocator + Clone> Clone for SafeBinaryHeap<T, A> {
    fn clone(&self) -> Self {
        SafeBinaryHeap { data: self.data.clone() }
    }

    fn clone_from(&mut self, source: &Self) {
        self.data.clone_from(&source.data);
    }
}

impl<T> Default for SafeBinaryHeap<T> {
    /// Creates an empty `SafeBinaryHeap<T>`.
    #[inline]
    fn default() -> SafeBinaryHeap<T> {
        SafeBinaryHeap::new()
    }
}

impl<T: fmt::Debug, A: Allocator> fmt::Debug for SafeBinaryHeap<T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

/// Drop guard that repairs the heap tail when an in-place mutation (extend / retain)
/// finishes or unwinds.
struct SafeRebuildOnDrop<'a, T: Ord, A: Allocator = Global> {
    heap: &'a mut SafeBinaryHeap<T, A>,
    rebuild_from: usize,
}

impl<T: Ord, A: Allocator> Drop for SafeRebuildOnDrop<'_, T, A> {
    fn drop(&mut self) {
        self.heap.rebuild_tail(self.rebuild_from);
    }
}

impl<T> SafeBinaryHeap<T> {
    /// Creates an empty `SafeBinaryHeap` as a max-heap.
    #[must_use]
    pub const fn new() -> SafeBinaryHeap<T> {
        SafeBinaryHeap { data: Vec::new() }
    }

    /// Creates an empty `SafeBinaryHeap` with at least the specified capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> SafeBinaryHeap<T> {
        SafeBinaryHeap { data: Vec::with_capacity(capacity) }
    }
}

impl<T, A: Allocator> SafeBinaryHeap<T, A> {
    /// Creates an empty `SafeBinaryHeap` as a max-heap, using `A` as allocator.
    #[must_use]
    pub const fn new_in(alloc: A) -> SafeBinaryHeap<T, A> {
        SafeBinaryHeap { data: Vec::new_in(alloc) }
    }

    /// Creates an empty `SafeBinaryHeap` with at least the specified capacity, using `A`.
    #[must_use]
    pub fn with_capacity_in(capacity: usize, alloc: A) -> SafeBinaryHeap<T, A> {
        SafeBinaryHeap { data: Vec::with_capacity_in(capacity, alloc) }
    }

    /// Creates a `SafeBinaryHeap` from the supplied `vec` without rebuilding it.
    ///
    /// Logically `vec` must already be a max-heap; unlike the unsafe heap this is a SAFE
    /// fn (a non-heap input only produces wrong results, never undefined behavior).
    #[must_use]
    pub fn from_raw_vec(vec: Vec<T, A>) -> SafeBinaryHeap<T, A> {
        SafeBinaryHeap { data: vec }
    }
}

impl<T: Ord, A: Allocator> SafeBinaryHeap<T, A> {
    /// Returns a mutable reference to the greatest item, or `None` if empty.
    pub fn peek_mut(&mut self) -> Option<SafePeekMut<'_, T, A>> {
        if self.is_empty() {
            None
        } else {
            Some(SafePeekMut { heap: self, tail: None })
        }
    }

    /// Removes the greatest item and returns it, or `None` if empty.
    pub fn pop(&mut self) -> Option<T> {
        let mut item = self.data.pop()?;
        if !self.is_empty() {
            swap(&mut item, &mut self.data[0]);
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
            self.data.swap(0, end);
            self.sift_down_range(0, end);
        }
        self.into_vec()
    }

    // --- sift family: swap-based (no hole), all indices checked ---

    /// Move the element at `pos` toward the root while it exceeds its parent. Returns the
    /// final index.
    fn sift_up(&mut self, start: usize, mut pos: usize) -> usize {
        while pos > start {
            let parent = (pos - 1) / 2;
            if self.data[pos] <= self.data[parent] {
                break;
            }
            self.data.swap(pos, parent);
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
            if right < end && self.data[child] <= self.data[right] {
                child = right;
            }
            if self.data[pos] >= self.data[child] {
                return pos;
            }
            self.data.swap(pos, child);
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
    pub fn drain_sorted(&mut self) -> SafeDrainSorted<'_, T, A> {
        SafeDrainSorted { inner: self }
    }

    /// Retains only the elements specified by the predicate, in unspecified order.
    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&T) -> bool,
    {
        let mut guard = SafeRebuildOnDrop { rebuild_from: self.len(), heap: self };
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

impl<T, A: Allocator> SafeBinaryHeap<T, A> {
    /// Returns an iterator visiting all values in the underlying vector, in arbitrary
    /// order.
    pub fn iter(&self) -> SafeIter<'_, T> {
        SafeIter { iter: self.data.iter() }
    }

    /// Returns an iterator which retrieves elements in heap order. Consumes the heap.
    pub fn into_iter_sorted(self) -> SafeIntoIterSorted<T, A> {
        SafeIntoIterSorted { inner: self }
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
    pub fn drain(&mut self) -> SafeDrain<'_, T, A> {
        SafeDrain { iter: self.data.drain(..) }
    }

    /// Drops all items from the heap.
    pub fn clear(&mut self) {
        self.drain();
    }
}

/// An iterator over the elements of a `SafeBinaryHeap`.
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct SafeIter<'a, T: 'a> {
    iter: std::slice::Iter<'a, T>,
}

impl<T> Default for SafeIter<'_, T> {
    /// Creates an empty `SafeIter`.
    fn default() -> Self {
        SafeIter { iter: Default::default() }
    }
}

impl<T: fmt::Debug> fmt::Debug for SafeIter<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SafeIter").field(&self.iter.as_slice()).finish()
    }
}

impl<T> Clone for SafeIter<'_, T> {
    fn clone(&self) -> Self {
        SafeIter { iter: self.iter.clone() }
    }
}

impl<'a, T> Iterator for SafeIter<'a, T> {
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

impl<'a, T> DoubleEndedIterator for SafeIter<'a, T> {
    #[inline]
    fn next_back(&mut self) -> Option<&'a T> {
        self.iter.next_back()
    }
}

impl<T> ExactSizeIterator for SafeIter<'_, T> {
    fn is_empty(&self) -> bool {
        self.iter.is_empty()
    }
}

impl<T> FusedIterator for SafeIter<'_, T> {}

/// An owning iterator over the elements of a `SafeBinaryHeap`.
#[derive(Clone)]
pub struct SafeIntoIter<T, A: Allocator = Global> {
    iter: std::vec::IntoIter<T, A>,
}

impl<T, A: Allocator> SafeIntoIter<T, A> {
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.iter.allocator()
    }
}

impl<T: fmt::Debug, A: Allocator> fmt::Debug for SafeIntoIter<T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SafeIntoIter").field(&self.iter.as_slice()).finish()
    }
}

impl<T, A: Allocator> Iterator for SafeIntoIter<T, A> {
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

impl<T, A: Allocator> DoubleEndedIterator for SafeIntoIter<T, A> {
    #[inline]
    fn next_back(&mut self) -> Option<T> {
        self.iter.next_back()
    }
}

impl<T, A: Allocator> ExactSizeIterator for SafeIntoIter<T, A> {
    fn is_empty(&self) -> bool {
        self.iter.is_empty()
    }
}

impl<T, A: Allocator> FusedIterator for SafeIntoIter<T, A> {}

impl<T> Default for SafeIntoIter<T> {
    /// Creates an empty `SafeIntoIter`.
    fn default() -> Self {
        SafeIntoIter { iter: Default::default() }
    }
}

/// An iterator that retrieves elements in heap (sorted) order, consuming the heap.
#[must_use = "iterators are lazy and do nothing unless consumed"]
#[derive(Clone, Debug)]
pub struct SafeIntoIterSorted<T, A: Allocator = Global> {
    inner: SafeBinaryHeap<T, A>,
}

impl<T, A: Allocator> SafeIntoIterSorted<T, A> {
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.inner.allocator()
    }
}

impl<T: Ord, A: Allocator> Iterator for SafeIntoIterSorted<T, A> {
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

impl<T: Ord, A: Allocator> ExactSizeIterator for SafeIntoIterSorted<T, A> {}

impl<T: Ord, A: Allocator> FusedIterator for SafeIntoIterSorted<T, A> {}

// `TrustedLen` is an unsafe PROMISE that `size_hint` is exact: lower == upper == the
// remaining count, and `next` yields exactly that many items. Length-driven consumers
// (`extend`/`collect`/`zip`) rely on it to reserve the exact capacity once and write with
// UNCHECKED stores + `set_len`, skipping per-item capacity/bounds checks — so a lying
// impl would write out of bounds (UB), which is why it is `unsafe impl`. Sound here:
// `next` is `pop` (one item per heap element) and `size_hint` is `(len, Some(len))`, so
// the count is exact. This length promise is the ONLY `unsafe` token in the safe heap; it
// does no memory-unsafe work itself.
unsafe impl<T: Ord, A: Allocator> TrustedLen for SafeIntoIterSorted<T, A> {}

/// A draining iterator over the elements of a `SafeBinaryHeap`.
#[derive(Debug)]
pub struct SafeDrain<'a, T: 'a, A: Allocator = Global> {
    iter: std::vec::Drain<'a, T, A>,
}

impl<T, A: Allocator> SafeDrain<'_, T, A> {
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.iter.allocator()
    }
}

impl<T, A: Allocator> Iterator for SafeDrain<'_, T, A> {
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

impl<T, A: Allocator> DoubleEndedIterator for SafeDrain<'_, T, A> {
    #[inline]
    fn next_back(&mut self) -> Option<T> {
        self.iter.next_back()
    }
}

impl<T, A: Allocator> ExactSizeIterator for SafeDrain<'_, T, A> {
    fn is_empty(&self) -> bool {
        self.iter.is_empty()
    }
}

impl<T, A: Allocator> FusedIterator for SafeDrain<'_, T, A> {}

/// A draining iterator over the elements of a `SafeBinaryHeap` in heap (sorted) order.
#[derive(Debug)]
pub struct SafeDrainSorted<'a, T: Ord, A: Allocator = Global> {
    inner: &'a mut SafeBinaryHeap<T, A>,
}

impl<'a, T: Ord, A: Allocator> SafeDrainSorted<'a, T, A> {
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.inner.allocator()
    }
}

impl<'a, T: Ord, A: Allocator> Drop for SafeDrainSorted<'a, T, A> {
    /// Removes heap elements in heap order.
    fn drop(&mut self) {
        struct DropGuard<'r, 'a, T: Ord, A: Allocator>(&'r mut SafeDrainSorted<'a, T, A>);

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

impl<T: Ord, A: Allocator> Iterator for SafeDrainSorted<'_, T, A> {
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

impl<T: Ord, A: Allocator> ExactSizeIterator for SafeDrainSorted<'_, T, A> {}

impl<T: Ord, A: Allocator> FusedIterator for SafeDrainSorted<'_, T, A> {}

// Sound exact-size assertion; see the SafeIntoIterSorted note above.
unsafe impl<T: Ord, A: Allocator> TrustedLen for SafeDrainSorted<'_, T, A> {}

impl<T: Ord, A: Allocator> From<Vec<T, A>> for SafeBinaryHeap<T, A> {
    /// Converts a `Vec<T>` into a `SafeBinaryHeap<T>`, in-place, *O*(*n*).
    fn from(vec: Vec<T, A>) -> SafeBinaryHeap<T, A> {
        let mut heap = SafeBinaryHeap { data: vec };
        heap.rebuild();
        heap
    }
}

impl<T: Ord, const N: usize> From<[T; N]> for SafeBinaryHeap<T> {
    fn from(arr: [T; N]) -> Self {
        Self::from_iter(arr)
    }
}

impl<T, A: Allocator> From<SafeBinaryHeap<T, A>> for Vec<T, A> {
    /// Converts a `SafeBinaryHeap<T>` into a `Vec<T>`. No data movement or allocation,
    /// constant time.
    fn from(heap: SafeBinaryHeap<T, A>) -> Vec<T, A> {
        heap.data
    }
}

impl<T: Ord> FromIterator<T> for SafeBinaryHeap<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> SafeBinaryHeap<T> {
        SafeBinaryHeap::from(iter.into_iter().collect::<Vec<_>>())
    }
}

impl<T, A: Allocator> IntoIterator for SafeBinaryHeap<T, A> {
    type Item = T;
    type IntoIter = SafeIntoIter<T, A>;

    /// Creates a consuming iterator that moves each value out of the heap in arbitrary
    /// order. The heap cannot be used after calling this.
    fn into_iter(self) -> SafeIntoIter<T, A> {
        SafeIntoIter { iter: self.data.into_iter() }
    }
}

impl<'a, T, A: Allocator> IntoIterator for &'a SafeBinaryHeap<T, A> {
    type Item = &'a T;
    type IntoIter = SafeIter<'a, T>;

    fn into_iter(self) -> SafeIter<'a, T> {
        self.iter()
    }
}

impl<T: Ord, A: Allocator> Extend<T> for SafeBinaryHeap<T, A> {
    #[inline]
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        let guard = SafeRebuildOnDrop { rebuild_from: self.len(), heap: self };
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

impl<'a, T: 'a + Ord + Copy, A: Allocator> Extend<&'a T> for SafeBinaryHeap<T, A> {
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
