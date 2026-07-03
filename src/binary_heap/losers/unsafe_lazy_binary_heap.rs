// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! UnsafeLazyBinaryHeap — `safe_but_for_index_binary_heap` (unchecked indexing, swap-based
//! sift) with a `peek_mut` that is *O*(1) AND keeps the forget guarantee AND never leaks or
//! loses data — by **lazy reconciliation** instead of leak amplification.
//!
//! The idea (vs the four prior variants):
//!   * unsafe `set_len` leak amplification: forget a mutated guard => the tail is **leaked**.
//!   * safe `split_off` tail-split: forget-safe but *O*(n) per mutated peek, needs `A: Clone`.
//!   * safe_opt / safe_but_for_index `&mut data[0]` + sift-on-drop: *O*(1) but **drops** the
//!     forget guarantee (a forgotten mutated guard leaves a BROKEN heap).
//!   * THIS: `deref_mut` only sets a `possibly_dirty_root` flag and returns
//!     `get_unchecked_mut(0)` (*O*(1), no data touched). The fix-up (`sift_down(0)`) is
//!     DEFERRED: `Drop` does it (fast path), and every `&mut` entry point first calls
//!     `clear_possibly_dirty_root()`. So a forgotten mutated guard loses **nothing** — all
//!     elements stay in `data` at full length (no leak, no orphaning), and the *next*
//!     operation re-sifts the root. `test_peek_mut_leek` therefore PASSES here.
//!
//! Why the flag is "possibly": it is set whenever a `&mut` to the root is lent out, not when
//! the value provably changed; `sift_down`'s first comparison resolves it in *O*(1) when the
//! root is actually still fine (read-only deref_mut, or mutated up/equal). The reconcile is
//! idempotent and `#[inline]`, so the clean-case cost is one predictable branch.
//!
//! Only the root can change (the guard exposes solely `&mut data[0]`) and the root has no
//! parent, so `sift_down(0)` is the exact and complete repair — never `sift_up`.
//!
//! Still unchecked indexing (`get_unchecked`/`swap_unchecked`, `feature(slice_swap_unchecked)`
//! via RUSTC_BOOTSTRAP=1), so it carries real `unsafe` blocks.

use core::iter::{FusedIterator, TrustedLen};
use core::mem::swap;
use core::ops::{Deref, DerefMut};
use core::fmt;
use std::alloc::{Allocator, Global};
use std::collections::TryReserveError;

/// A priority queue implemented with a binary (max-)heap — unchecked + lazy-reconcile.
pub struct UnsafeLazyBinaryHeap<T, A: Allocator = Global> {
    data: Vec<T, A>,
    // `true` ⇒ a `&mut` to the root was lent out via `peek_mut` and may have desynced it; the
    // subtrees `data[1..]` are still a valid heap. Cleared by `clear_possibly_dirty_root` (in
    // `PeekMut::Drop` and at every `&mut` entry point). Never set unless a heap has > 1 element.
    possibly_dirty_root: bool,
}

/// Mutable-greatest-element guard for `UnsafeLazyBinaryHeap`. Created by
/// [`UnsafeLazyBinaryHeap::peek_mut`].
///
/// The greatest element stays in place at `data[0]`; the guard derefs to it. `deref_mut`
/// only sets the heap's `possibly_dirty_root` flag (*O*(1), nothing moved). The re-sift is
/// deferred: `Drop` runs `clear_possibly_dirty_root` (the fast path), and if the guard is
/// FORGOTTEN the flag survives and the heap's next `&mut` operation reconciles it — so a
/// forgotten mutated guard loses nothing and leaks nothing.
pub struct UnsafeLazyPeekMut<'a, T: 'a + Ord, A: Allocator = Global> {
    heap: &'a mut UnsafeLazyBinaryHeap<T, A>,
}

impl<T: Ord + fmt::Debug, A: Allocator> fmt::Debug for UnsafeLazyPeekMut<'_, T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // SAFETY: a UnsafeLazyPeekMut is only created for a non-empty heap.
        f.debug_tuple("UnsafeLazyPeekMut").field(unsafe { self.heap.data.get_unchecked(0) }).finish()
    }
}

impl<T: Ord, A: Allocator> Drop for UnsafeLazyPeekMut<'_, T, A> {
    fn drop(&mut self) {
        // Fast path: reconcile the (possibly) dirtied root now. If the guard is forgotten
        // instead, this never runs — but the flag survives and the heap self-heals on next use.
        self.heap.clear_possibly_dirty_root();
    }
}

impl<T: Ord, A: Allocator> Deref for UnsafeLazyPeekMut<'_, T, A> {
    type Target = T;
    fn deref(&self) -> &T {
        debug_assert!(!self.heap.is_empty());
        // SAFETY: only created for a non-empty heap, so index 0 is in bounds.
        unsafe { self.heap.data.get_unchecked(0) }
    }
}

impl<T: Ord, A: Allocator> DerefMut for UnsafeLazyPeekMut<'_, T, A> {
    fn deref_mut(&mut self) -> &mut T {
        debug_assert!(!self.heap.is_empty());
        // O(1): mark the root as possibly-dirty and hand out `&mut data[0]`. The repair is
        // deferred (Drop / next op), so this never touches or moves any other element, and a
        // forget cannot lose or leak data. Only set the flag for len > 1 (a singleton is
        // always a valid heap whatever its value, so it never needs a reconcile).
        if self.heap.len() > 1 {
            self.heap.possibly_dirty_root = true;
        }
        // SAFETY: only created for a non-empty heap, so index 0 is in bounds.
        unsafe { self.heap.data.get_unchecked_mut(0) }
    }
}

impl<'a, T: Ord, A: Allocator> UnsafeLazyPeekMut<'a, T, A> {
    /// Sifts the current element to its new position. Afterwards refers to the new
    /// element. Returns whether the maximum changed.
    #[must_use = "is equivalent to dropping and getting a new UnsafeLazyPeekMut except for return information"]
    pub fn refresh(&mut self) -> bool {
        // Sift the (possibly mutated) root down now; it changed iff it moved off index 0.
        let moved = self.heap.sift_down(0) != 0;
        self.heap.possibly_dirty_root = false;
        moved
    }

    /// Removes the peeked value from the heap and returns it.
    pub fn pop(this: UnsafeLazyPeekMut<'a, T, A>) -> T {
        // Remove the (possibly mutated) root and re-heapify the rest; the root is gone, so
        // clear the flag (Drop then no-ops).
        this.heap.possibly_dirty_root = false;
        let val = this.heap.data.swap_remove(0);
        if !this.heap.is_empty() {
            this.heap.sift_down(0);
        }
        val
    }
}

impl<T: Clone, A: Allocator + Clone> Clone for UnsafeLazyBinaryHeap<T, A> {
    fn clone(&self) -> Self {
        // Preserve the flag: cloning a dirty heap must yield a dirty clone, not a "clean" heap
        // with an unsorted root.
        UnsafeLazyBinaryHeap { data: self.data.clone(), possibly_dirty_root: self.possibly_dirty_root }
    }

    fn clone_from(&mut self, source: &Self) {
        self.data.clone_from(&source.data);
        self.possibly_dirty_root = source.possibly_dirty_root;
    }
}

impl<T> Default for UnsafeLazyBinaryHeap<T> {
    /// Creates an empty `UnsafeLazyBinaryHeap<T>`.
    #[inline]
    fn default() -> UnsafeLazyBinaryHeap<T> {
        UnsafeLazyBinaryHeap::new()
    }
}

impl<T: fmt::Debug, A: Allocator> fmt::Debug for UnsafeLazyBinaryHeap<T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

/// Drop guard that repairs the heap tail when an in-place mutation (extend / retain)
/// finishes or unwinds.
struct UnsafeLazyRebuildOnDrop<'a, T: Ord, A: Allocator = Global> {
    heap: &'a mut UnsafeLazyBinaryHeap<T, A>,
    rebuild_from: usize,
}

impl<T: Ord, A: Allocator> Drop for UnsafeLazyRebuildOnDrop<'_, T, A> {
    fn drop(&mut self) {
        self.heap.rebuild_tail(self.rebuild_from);
    }
}

impl<T> UnsafeLazyBinaryHeap<T> {
    /// Creates an empty `UnsafeLazyBinaryHeap` as a max-heap.
    #[must_use]
    pub const fn new() -> UnsafeLazyBinaryHeap<T> {
        UnsafeLazyBinaryHeap { data: Vec::new(), possibly_dirty_root: false }
    }

    /// Creates an empty `UnsafeLazyBinaryHeap` with at least the specified capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> UnsafeLazyBinaryHeap<T> {
        UnsafeLazyBinaryHeap { data: Vec::with_capacity(capacity), possibly_dirty_root: false }
    }
}

impl<T, A: Allocator> UnsafeLazyBinaryHeap<T, A> {
    /// Creates an empty `UnsafeLazyBinaryHeap` as a max-heap, using `A` as allocator.
    #[must_use]
    pub const fn new_in(alloc: A) -> UnsafeLazyBinaryHeap<T, A> {
        UnsafeLazyBinaryHeap { data: Vec::new_in(alloc), possibly_dirty_root: false }
    }

    /// Creates an empty `UnsafeLazyBinaryHeap` with at least the specified capacity, using `A`.
    #[must_use]
    pub fn with_capacity_in(capacity: usize, alloc: A) -> UnsafeLazyBinaryHeap<T, A> {
        UnsafeLazyBinaryHeap { data: Vec::with_capacity_in(capacity, alloc), possibly_dirty_root: false }
    }

    /// Creates a `UnsafeLazyBinaryHeap` from the supplied `vec` without rebuilding it.
    ///
    /// Logically `vec` must already be a max-heap; unlike the unsafe heap this is a SAFE
    /// fn (a non-heap input only produces wrong results, never undefined behavior).
    #[must_use]
    pub fn from_raw_vec(vec: Vec<T, A>) -> UnsafeLazyBinaryHeap<T, A> {
        UnsafeLazyBinaryHeap { data: vec, possibly_dirty_root: false }
    }
}

impl<T: Ord, A: Allocator> UnsafeLazyBinaryHeap<T, A> {
    /// Returns the greatest item, or `None` if empty.
    ///
    /// Note: unlike std, `peek` here requires `Ord`. If a `peek_mut` guard was forgotten after
    /// mutating the root (`possibly_dirty_root`), the max may no longer be at `data[0]` — but
    /// the subtrees are still valid heaps, so the max is the root or one of its (≤ 2) immediate
    /// children, found in *O*(1) without a reconcile (which `&self` can't do anyway).
    #[must_use]
    pub fn peek(&self) -> Option<&T> {
        let len = self.data.len();
        if len == 0 {
            return None;
        }
        // SAFETY: every index touched below is `< len`.
        unsafe {
            if !self.possibly_dirty_root {
                return Some(self.data.get_unchecked(0));
            }
            let mut best = 0usize;
            if len > 1 && self.data.get_unchecked(1) > self.data.get_unchecked(best) {
                best = 1;
            }
            if len > 2 && self.data.get_unchecked(2) > self.data.get_unchecked(best) {
                best = 2;
            }
            Some(self.data.get_unchecked(best))
        }
    }

    /// Returns a mutable reference to the greatest item, or `None` if empty.
    pub fn peek_mut(&mut self) -> Option<UnsafeLazyPeekMut<'_, T, A>> {
        self.clear_possibly_dirty_root(); // heal a prior forgotten guard before lending again
        if self.is_empty() {
            None
        } else {
            Some(UnsafeLazyPeekMut { heap: self })
        }
    }

    /// Removes the greatest item and returns it, or `None` if empty.
    pub fn pop(&mut self) -> Option<T> {
        self.clear_possibly_dirty_root();
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
        self.clear_possibly_dirty_root();
        let old_len = self.len();
        self.data.push(item);
        self.sift_up(0, old_len);
    }

    /// Consumes the heap and returns a vector in sorted (ascending) order.
    #[must_use = "`self` will be dropped if the result is not used"]
    pub fn into_sorted_vec(mut self) -> Vec<T, A> {
        self.clear_possibly_dirty_root();
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

    /// Lazy reconcile: if `peek_mut` lent out a `&mut` to the root and it may have desynced,
    /// sift it back into place. *O*(1) when clean (one predictable, hoistable branch — the
    /// whole reason `deref_mut` could be *O*(1)); *O*(log n) only when the root actually sank.
    /// Idempotent, so it is safe to call unconditionally at the start of every `&mut` op.
    #[inline]
    fn clear_possibly_dirty_root(&mut self) {
        if self.possibly_dirty_root {
            if self.len() > 1 {
                self.sift_down(0);
            }
            self.possibly_dirty_root = false;
        }
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
        self.clear_possibly_dirty_root();
        other.clear_possibly_dirty_root();
        if self.len() < other.len() {
            swap(self, other);
        }

        let start = self.data.len();

        self.data.append(&mut other.data);

        self.rebuild_tail(start);
    }

    /// Clears the heap, returning an iterator over the removed elements in heap order.
    #[inline]
    pub fn drain_sorted(&mut self) -> UnsafeLazyDrainSorted<'_, T, A> {
        UnsafeLazyDrainSorted { inner: self }
    }

    /// Retains only the elements specified by the predicate, in unspecified order.
    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&T) -> bool,
    {
        self.clear_possibly_dirty_root();
        let mut guard = UnsafeLazyRebuildOnDrop { rebuild_from: self.len(), heap: self };
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

impl<T, A: Allocator> UnsafeLazyBinaryHeap<T, A> {
    /// Returns an iterator visiting all values in the underlying vector, in arbitrary
    /// order.
    pub fn iter(&self) -> UnsafeLazyIter<'_, T> {
        UnsafeLazyIter { iter: self.data.iter() }
    }

    /// Returns an iterator which retrieves elements in heap order. Consumes the heap.
    pub fn into_iter_sorted(self) -> UnsafeLazyIntoIterSorted<T, A> {
        UnsafeLazyIntoIterSorted { inner: self }
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
    pub fn drain(&mut self) -> UnsafeLazyDrain<'_, T, A> {
        // The (possibly dirty) root is about to be removed along with everything else; the
        // emptied heap is trivially clean. (No `Ord` here, so just clear the flag — order is
        // irrelevant to a full drain anyway.)
        self.possibly_dirty_root = false;
        UnsafeLazyDrain { iter: self.data.drain(..) }
    }

    /// Drops all items from the heap.
    pub fn clear(&mut self) {
        self.drain();
    }
}

/// An iterator over the elements of a `UnsafeLazyBinaryHeap`.
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct UnsafeLazyIter<'a, T: 'a> {
    iter: std::slice::Iter<'a, T>,
}

impl<T> Default for UnsafeLazyIter<'_, T> {
    /// Creates an empty `UnsafeLazyIter`.
    fn default() -> Self {
        UnsafeLazyIter { iter: Default::default() }
    }
}

impl<T: fmt::Debug> fmt::Debug for UnsafeLazyIter<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("UnsafeLazyIter").field(&self.iter.as_slice()).finish()
    }
}

impl<T> Clone for UnsafeLazyIter<'_, T> {
    fn clone(&self) -> Self {
        UnsafeLazyIter { iter: self.iter.clone() }
    }
}

impl<'a, T> Iterator for UnsafeLazyIter<'a, T> {
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

impl<'a, T> DoubleEndedIterator for UnsafeLazyIter<'a, T> {
    #[inline]
    fn next_back(&mut self) -> Option<&'a T> {
        self.iter.next_back()
    }
}

impl<T> ExactSizeIterator for UnsafeLazyIter<'_, T> {
    fn is_empty(&self) -> bool {
        self.iter.is_empty()
    }
}

impl<T> FusedIterator for UnsafeLazyIter<'_, T> {}

/// An owning iterator over the elements of a `UnsafeLazyBinaryHeap`.
#[derive(Clone)]
pub struct UnsafeLazyIntoIter<T, A: Allocator = Global> {
    iter: std::vec::IntoIter<T, A>,
}

impl<T, A: Allocator> UnsafeLazyIntoIter<T, A> {
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.iter.allocator()
    }
}

impl<T: fmt::Debug, A: Allocator> fmt::Debug for UnsafeLazyIntoIter<T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("UnsafeLazyIntoIter").field(&self.iter.as_slice()).finish()
    }
}

impl<T, A: Allocator> Iterator for UnsafeLazyIntoIter<T, A> {
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

impl<T, A: Allocator> DoubleEndedIterator for UnsafeLazyIntoIter<T, A> {
    #[inline]
    fn next_back(&mut self) -> Option<T> {
        self.iter.next_back()
    }
}

impl<T, A: Allocator> ExactSizeIterator for UnsafeLazyIntoIter<T, A> {
    fn is_empty(&self) -> bool {
        self.iter.is_empty()
    }
}

impl<T, A: Allocator> FusedIterator for UnsafeLazyIntoIter<T, A> {}

impl<T> Default for UnsafeLazyIntoIter<T> {
    /// Creates an empty `UnsafeLazyIntoIter`.
    fn default() -> Self {
        UnsafeLazyIntoIter { iter: Default::default() }
    }
}

/// An iterator that retrieves elements in heap (sorted) order, consuming the heap.
#[must_use = "iterators are lazy and do nothing unless consumed"]
#[derive(Clone, Debug)]
pub struct UnsafeLazyIntoIterSorted<T, A: Allocator = Global> {
    inner: UnsafeLazyBinaryHeap<T, A>,
}

impl<T, A: Allocator> UnsafeLazyIntoIterSorted<T, A> {
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.inner.allocator()
    }
}

impl<T: Ord, A: Allocator> Iterator for UnsafeLazyIntoIterSorted<T, A> {
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

impl<T: Ord, A: Allocator> ExactSizeIterator for UnsafeLazyIntoIterSorted<T, A> {}

impl<T: Ord, A: Allocator> FusedIterator for UnsafeLazyIntoIterSorted<T, A> {}

// `TrustedLen` is an unsafe PROMISE that `size_hint` is exact: lower == upper == the
// remaining count, and `next` yields exactly that many items. Length-driven consumers
// (`extend`/`collect`/`zip`) rely on it to reserve the exact capacity once and write with
// UNCHECKED stores + `set_len`, skipping per-item capacity/bounds checks — so a lying
// impl would write out of bounds (UB), which is why it is `unsafe impl`. Sound here:
// `next` is `pop` (one item per heap element) and `size_hint` is `(len, Some(len))`, so
// the count is exact. This length promise is the ONLY `unsafe` token in the safe heap; it
// does no memory-unsafe work itself.
unsafe impl<T: Ord, A: Allocator> TrustedLen for UnsafeLazyIntoIterSorted<T, A> {}

/// A draining iterator over the elements of a `UnsafeLazyBinaryHeap`.
#[derive(Debug)]
pub struct UnsafeLazyDrain<'a, T: 'a, A: Allocator = Global> {
    iter: std::vec::Drain<'a, T, A>,
}

impl<T, A: Allocator> UnsafeLazyDrain<'_, T, A> {
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.iter.allocator()
    }
}

impl<T, A: Allocator> Iterator for UnsafeLazyDrain<'_, T, A> {
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

impl<T, A: Allocator> DoubleEndedIterator for UnsafeLazyDrain<'_, T, A> {
    #[inline]
    fn next_back(&mut self) -> Option<T> {
        self.iter.next_back()
    }
}

impl<T, A: Allocator> ExactSizeIterator for UnsafeLazyDrain<'_, T, A> {
    fn is_empty(&self) -> bool {
        self.iter.is_empty()
    }
}

impl<T, A: Allocator> FusedIterator for UnsafeLazyDrain<'_, T, A> {}

/// A draining iterator over the elements of a `UnsafeLazyBinaryHeap` in heap (sorted) order.
#[derive(Debug)]
pub struct UnsafeLazyDrainSorted<'a, T: Ord, A: Allocator = Global> {
    inner: &'a mut UnsafeLazyBinaryHeap<T, A>,
}

impl<'a, T: Ord, A: Allocator> UnsafeLazyDrainSorted<'a, T, A> {
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.inner.allocator()
    }
}

impl<'a, T: Ord, A: Allocator> Drop for UnsafeLazyDrainSorted<'a, T, A> {
    /// Removes heap elements in heap order.
    fn drop(&mut self) {
        struct DropGuard<'r, 'a, T: Ord, A: Allocator>(&'r mut UnsafeLazyDrainSorted<'a, T, A>);

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

impl<T: Ord, A: Allocator> Iterator for UnsafeLazyDrainSorted<'_, T, A> {
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

impl<T: Ord, A: Allocator> ExactSizeIterator for UnsafeLazyDrainSorted<'_, T, A> {}

impl<T: Ord, A: Allocator> FusedIterator for UnsafeLazyDrainSorted<'_, T, A> {}

// Sound exact-size assertion; see the UnsafeLazyIntoIterSorted note above.
unsafe impl<T: Ord, A: Allocator> TrustedLen for UnsafeLazyDrainSorted<'_, T, A> {}

impl<T: Ord, A: Allocator> From<Vec<T, A>> for UnsafeLazyBinaryHeap<T, A> {
    /// Converts a `Vec<T>` into a `UnsafeLazyBinaryHeap<T>`, in-place, *O*(*n*).
    fn from(vec: Vec<T, A>) -> UnsafeLazyBinaryHeap<T, A> {
        let mut heap = UnsafeLazyBinaryHeap { data: vec, possibly_dirty_root: false };
        heap.rebuild();
        heap
    }
}

impl<T: Ord, const N: usize> From<[T; N]> for UnsafeLazyBinaryHeap<T> {
    fn from(arr: [T; N]) -> Self {
        Self::from_iter(arr)
    }
}

impl<T, A: Allocator> From<UnsafeLazyBinaryHeap<T, A>> for Vec<T, A> {
    /// Converts a `UnsafeLazyBinaryHeap<T>` into a `Vec<T>`. No data movement or allocation,
    /// constant time.
    fn from(heap: UnsafeLazyBinaryHeap<T, A>) -> Vec<T, A> {
        heap.data
    }
}

impl<T: Ord> FromIterator<T> for UnsafeLazyBinaryHeap<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> UnsafeLazyBinaryHeap<T> {
        UnsafeLazyBinaryHeap::from(iter.into_iter().collect::<Vec<_>>())
    }
}

impl<T, A: Allocator> IntoIterator for UnsafeLazyBinaryHeap<T, A> {
    type Item = T;
    type IntoIter = UnsafeLazyIntoIter<T, A>;

    /// Creates a consuming iterator that moves each value out of the heap in arbitrary
    /// order. The heap cannot be used after calling this.
    fn into_iter(self) -> UnsafeLazyIntoIter<T, A> {
        UnsafeLazyIntoIter { iter: self.data.into_iter() }
    }
}

impl<'a, T, A: Allocator> IntoIterator for &'a UnsafeLazyBinaryHeap<T, A> {
    type Item = &'a T;
    type IntoIter = UnsafeLazyIter<'a, T>;

    fn into_iter(self) -> UnsafeLazyIter<'a, T> {
        self.iter()
    }
}

impl<T: Ord, A: Allocator> Extend<T> for UnsafeLazyBinaryHeap<T, A> {
    #[inline]
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        let guard = UnsafeLazyRebuildOnDrop { rebuild_from: self.len(), heap: self };
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

impl<'a, T: 'a + Ord + Copy, A: Allocator> Extend<&'a T> for UnsafeLazyBinaryHeap<T, A> {
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
