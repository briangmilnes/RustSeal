// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! UnsafeLazyHoleBinaryHeap — the best-of-both variant: `unsafe_lazy_binary_heap`'s lazy-
//! reconcile `peek_mut` (forget-safe, no leak, no data loss) on top of the faithful std
//! **`Hole` sift** (move once per step, PANIC-SAFE) instead of `unsafe_lazy`'s swap sift.
//!
//! It combines the two best properties measured across the other variants:
//!   * **sift body = the `Hole`** (from `unsafe_binary_heap`): one move per displaced element
//!     (vs swap's two), and panic-safe — `Hole::Drop` fills the hole even on a comparison
//!     panic mid-sift, so there is no duplicated slot / double-drop. So `panic_safe` PASSES.
//!   * **`peek_mut` = lazy reconcile** (from `unsafe_lazy`): `deref_mut` only sets a
//!     `possibly_dirty_root` flag and returns `get_unchecked_mut(0)` (*O*(1), nothing moved);
//!     the `sift_down(0)` repair is DEFERRED to `Drop` and to `clear_possibly_dirty_root()`
//!     (`#[inline]`) at every `&mut` entry point. A forgotten mutated guard loses **nothing**
//!     (no `set_len` leak, no `split_off`, no `A: Clone`); the next op re-sifts.
//!     `test_peek_mut_leek` PASSES.
//!
//! So this should be the fastest variant that is BOTH panic-safe AND forget-safe-with-no-data-
//! loss — the `Hole` recovers the swap-vs-hole cost that left `unsafe_lazy` at ~2.3× on the
//! `peek_mut`-heavy `find_smallest` bench.
//!
//! The flag is "possibly" (set on any `&mut`-to-root lend, not a proven change); the reconcile
//! short-circuits in *O*(1) when the root is actually fine. `peek(&self)` handles the dirty
//! case in *O*(1) via max(root, ≤2 children) — hence `peek` requires `Ord` here.
//!
//! Unchecked indexing throughout (`get_unchecked`, plus `swap_unchecked` in `into_sorted_vec`,
//! `feature(slice_swap_unchecked)` via RUSTC_BOOTSTRAP=1); real `unsafe` blocks.

use core::iter::{FusedIterator, TrustedLen};
use core::mem::{swap, ManuallyDrop};
use core::ops::{Deref, DerefMut};
use core::{fmt, ptr};
use std::alloc::{Allocator, Global};
use std::collections::TryReserveError;

/// A priority queue implemented with a binary (max-)heap — unchecked + lazy-reconcile.
pub struct UnsafeLazyHoleBinaryHeap<T, A: Allocator = Global> {
    data: Vec<T, A>,
    // `true` ⇒ a `&mut` to the root was lent out via `peek_mut` and may have desynced it; the
    // subtrees `data[1..]` are still a valid heap. Cleared by `clear_possibly_dirty_root` (in
    // `PeekMut::Drop` and at every `&mut` entry point). Never set unless a heap has > 1 element.
    possibly_dirty_root: bool,
}

/// Mutable-greatest-element guard for `UnsafeLazyHoleBinaryHeap`. Created by
/// [`UnsafeLazyHoleBinaryHeap::peek_mut`].
///
/// The greatest element stays in place at `data[0]`; the guard derefs to it. `deref_mut`
/// only sets the heap's `possibly_dirty_root` flag (*O*(1), nothing moved). The re-sift is
/// deferred: `Drop` runs `clear_possibly_dirty_root` (the fast path), and if the guard is
/// FORGOTTEN the flag survives and the heap's next `&mut` operation reconciles it — so a
/// forgotten mutated guard loses nothing and leaks nothing.
pub struct UnsafeLazyHolePeekMut<'a, T: 'a + Ord, A: Allocator = Global> {
    heap: &'a mut UnsafeLazyHoleBinaryHeap<T, A>,
}

impl<T: Ord + fmt::Debug, A: Allocator> fmt::Debug for UnsafeLazyHolePeekMut<'_, T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // SAFETY: a UnsafeLazyHolePeekMut is only created for a non-empty heap.
        f.debug_tuple("UnsafeLazyHolePeekMut").field(unsafe { self.heap.data.get_unchecked(0) }).finish()
    }
}

impl<T: Ord, A: Allocator> Drop for UnsafeLazyHolePeekMut<'_, T, A> {
    fn drop(&mut self) {
        // Fast path: reconcile the (possibly) dirtied root now. If the guard is forgotten
        // instead, this never runs — but the flag survives and the heap self-heals on next use.
        self.heap.clear_possibly_dirty_root();
    }
}

impl<T: Ord, A: Allocator> Deref for UnsafeLazyHolePeekMut<'_, T, A> {
    type Target = T;
    fn deref(&self) -> &T {
        debug_assert!(!self.heap.is_empty());
        // SAFETY: only created for a non-empty heap, so index 0 is in bounds.
        unsafe { self.heap.data.get_unchecked(0) }
    }
}

impl<T: Ord, A: Allocator> DerefMut for UnsafeLazyHolePeekMut<'_, T, A> {
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

impl<'a, T: Ord, A: Allocator> UnsafeLazyHolePeekMut<'a, T, A> {
    /// Sifts the current element to its new position. Afterwards refers to the new
    /// element. Returns whether the maximum changed.
    #[must_use = "is equivalent to dropping and getting a new UnsafeLazyHolePeekMut except for return information"]
    pub fn refresh(&mut self) -> bool {
        // Sift the (possibly mutated) root down now; it changed iff it moved off index 0.
        let moved = self.heap.sift_down(0) != 0;
        self.heap.possibly_dirty_root = false;
        moved
    }

    /// Removes the peeked value from the heap and returns it.
    pub fn pop(this: UnsafeLazyHolePeekMut<'a, T, A>) -> T {
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

impl<T: Clone, A: Allocator + Clone> Clone for UnsafeLazyHoleBinaryHeap<T, A> {
    fn clone(&self) -> Self {
        // Preserve the flag: cloning a dirty heap must yield a dirty clone, not a "clean" heap
        // with an unsorted root.
        UnsafeLazyHoleBinaryHeap { data: self.data.clone(), possibly_dirty_root: self.possibly_dirty_root }
    }

    fn clone_from(&mut self, source: &Self) {
        self.data.clone_from(&source.data);
        self.possibly_dirty_root = source.possibly_dirty_root;
    }
}

impl<T> Default for UnsafeLazyHoleBinaryHeap<T> {
    /// Creates an empty `UnsafeLazyHoleBinaryHeap<T>`.
    #[inline]
    fn default() -> UnsafeLazyHoleBinaryHeap<T> {
        UnsafeLazyHoleBinaryHeap::new()
    }
}

impl<T: fmt::Debug, A: Allocator> fmt::Debug for UnsafeLazyHoleBinaryHeap<T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

/// Drop guard that repairs the heap tail when an in-place mutation (extend / retain)
/// finishes or unwinds.
struct UnsafeLazyHoleRebuildOnDrop<'a, T: Ord, A: Allocator = Global> {
    heap: &'a mut UnsafeLazyHoleBinaryHeap<T, A>,
    rebuild_from: usize,
}

impl<T: Ord, A: Allocator> Drop for UnsafeLazyHoleRebuildOnDrop<'_, T, A> {
    fn drop(&mut self) {
        self.heap.rebuild_tail(self.rebuild_from);
    }
}

impl<T> UnsafeLazyHoleBinaryHeap<T> {
    /// Creates an empty `UnsafeLazyHoleBinaryHeap` as a max-heap.
    #[must_use]
    pub const fn new() -> UnsafeLazyHoleBinaryHeap<T> {
        UnsafeLazyHoleBinaryHeap { data: Vec::new(), possibly_dirty_root: false }
    }

    /// Creates an empty `UnsafeLazyHoleBinaryHeap` with at least the specified capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> UnsafeLazyHoleBinaryHeap<T> {
        UnsafeLazyHoleBinaryHeap { data: Vec::with_capacity(capacity), possibly_dirty_root: false }
    }
}

impl<T, A: Allocator> UnsafeLazyHoleBinaryHeap<T, A> {
    /// Creates an empty `UnsafeLazyHoleBinaryHeap` as a max-heap, using `A` as allocator.
    #[must_use]
    pub const fn new_in(alloc: A) -> UnsafeLazyHoleBinaryHeap<T, A> {
        UnsafeLazyHoleBinaryHeap { data: Vec::new_in(alloc), possibly_dirty_root: false }
    }

    /// Creates an empty `UnsafeLazyHoleBinaryHeap` with at least the specified capacity, using `A`.
    #[must_use]
    pub fn with_capacity_in(capacity: usize, alloc: A) -> UnsafeLazyHoleBinaryHeap<T, A> {
        UnsafeLazyHoleBinaryHeap { data: Vec::with_capacity_in(capacity, alloc), possibly_dirty_root: false }
    }

    /// Creates a `UnsafeLazyHoleBinaryHeap` from the supplied `vec` without rebuilding it.
    ///
    /// Logically `vec` must already be a max-heap; unlike the unsafe heap this is a SAFE
    /// fn (a non-heap input only produces wrong results, never undefined behavior).
    #[must_use]
    pub fn from_raw_vec(vec: Vec<T, A>) -> UnsafeLazyHoleBinaryHeap<T, A> {
        UnsafeLazyHoleBinaryHeap { data: vec, possibly_dirty_root: false }
    }
}

impl<T: Ord, A: Allocator> UnsafeLazyHoleBinaryHeap<T, A> {
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
    pub fn peek_mut(&mut self) -> Option<UnsafeLazyHolePeekMut<'_, T, A>> {
        self.clear_possibly_dirty_root(); // heal a prior forgotten guard before lending again
        if self.is_empty() {
            None
        } else {
            Some(UnsafeLazyHolePeekMut { heap: self })
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
    /// final index. Uses the `Hole` (move once per step), and is PANIC-SAFE: if `T: Ord`
    /// panics mid-loop, `Hole::Drop` fills the hole back, so the array is never left with a
    /// duplicated slot (no double-drop on unwind). This is the faithful std sift.
    fn sift_up(&mut self, start: usize, pos: usize) -> usize {
        // SAFETY: pos < self.len() (caller invariant); every index touched (parent < hole.pos)
        // is < pos < len, and != hole.pos.
        unsafe {
            let mut hole = Hole::new(&mut self.data, pos);
            while hole.pos() > start {
                let parent = (hole.pos() - 1) / 2;
                if hole.element() <= hole.get(parent) {
                    break;
                }
                hole.move_to(parent);
            }
            hole.pos()
        }
    }

    /// Move the element at `pos` down within `[pos, end)` while smaller than its greater
    /// child. Returns the final index. Hole-based and panic-safe (see `sift_up`).
    fn sift_down_range(&mut self, pos: usize, end: usize) -> usize {
        // SAFETY: pos < end <= self.len() (caller invariant); every index touched is < end.
        unsafe {
            let mut hole = Hole::new(&mut self.data, pos);
            let mut child = 2 * hole.pos() + 1;
            while child <= end.saturating_sub(2) {
                child += (hole.get(child) <= hole.get(child + 1)) as usize;
                if hole.element() >= hole.get(child) {
                    return hole.pos();
                }
                hole.move_to(child);
                child = 2 * hole.pos() + 1;
            }
            if child == end - 1 && hole.element() < hole.get(child) {
                hole.move_to(child);
            }
            hole.pos()
        }
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
    pub fn drain_sorted(&mut self) -> UnsafeLazyHoleDrainSorted<'_, T, A> {
        UnsafeLazyHoleDrainSorted { inner: self }
    }

    /// Retains only the elements specified by the predicate, in unspecified order.
    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&T) -> bool,
    {
        self.clear_possibly_dirty_root();
        let mut guard = UnsafeLazyHoleRebuildOnDrop { rebuild_from: self.len(), heap: self };
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

impl<T, A: Allocator> UnsafeLazyHoleBinaryHeap<T, A> {
    /// Returns an iterator visiting all values in the underlying vector, in arbitrary
    /// order.
    pub fn iter(&self) -> UnsafeLazyHoleIter<'_, T> {
        UnsafeLazyHoleIter { iter: self.data.iter() }
    }

    /// Returns an iterator which retrieves elements in heap order. Consumes the heap.
    pub fn into_iter_sorted(self) -> UnsafeLazyHoleIntoIterSorted<T, A> {
        UnsafeLazyHoleIntoIterSorted { inner: self }
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
    pub fn drain(&mut self) -> UnsafeLazyHoleDrain<'_, T, A> {
        // The (possibly dirty) root is about to be removed along with everything else; the
        // emptied heap is trivially clean. (No `Ord` here, so just clear the flag — order is
        // irrelevant to a full drain anyway.)
        self.possibly_dirty_root = false;
        UnsafeLazyHoleDrain { iter: self.data.drain(..) }
    }

    /// Drops all items from the heap.
    pub fn clear(&mut self) {
        self.drain();
    }
}

/// `Hole` represents a hole in a slice — an index whose value was moved out (and is carried
/// in `elt`). On drop the value is written back at the current `pos`, so a comparison panic
/// mid-sift leaves the slice with no duplicated slot (PANIC-SAFE). The faithful std `Hole`.
struct Hole<'a, T: 'a> {
    data: &'a mut [T],
    elt: ManuallyDrop<T>,
    pos: usize,
}

impl<'a, T> Hole<'a, T> {
    /// # Safety
    /// `pos < data.len()`.
    #[inline]
    unsafe fn new(data: &'a mut [T], pos: usize) -> Self {
        debug_assert!(pos < data.len());
        let elt = unsafe { ptr::read(data.get_unchecked(pos)) };
        Hole { data, elt: ManuallyDrop::new(elt), pos }
    }

    #[inline]
    fn pos(&self) -> usize {
        self.pos
    }

    #[inline]
    fn element(&self) -> &T {
        &self.elt
    }

    /// # Safety
    /// `index != self.pos` and `index < self.data.len()`.
    #[inline]
    unsafe fn get(&self, index: usize) -> &T {
        debug_assert!(index != self.pos);
        debug_assert!(index < self.data.len());
        unsafe { self.data.get_unchecked(index) }
    }

    /// Move the hole to `index`, shifting that element into the old hole.
    /// # Safety
    /// `index != self.pos` and `index < self.data.len()`.
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

impl<T> Drop for Hole<'_, T> {
    #[inline]
    fn drop(&mut self) {
        // Fill the hole again (also runs on a panic-unwind — the panic safety).
        unsafe {
            let pos = self.pos;
            ptr::copy_nonoverlapping(&*self.elt, self.data.get_unchecked_mut(pos), 1);
        }
    }
}

/// An iterator over the elements of a `UnsafeLazyHoleBinaryHeap`.
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct UnsafeLazyHoleIter<'a, T: 'a> {
    iter: std::slice::Iter<'a, T>,
}

impl<T> Default for UnsafeLazyHoleIter<'_, T> {
    /// Creates an empty `UnsafeLazyHoleIter`.
    fn default() -> Self {
        UnsafeLazyHoleIter { iter: Default::default() }
    }
}

impl<T: fmt::Debug> fmt::Debug for UnsafeLazyHoleIter<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("UnsafeLazyHoleIter").field(&self.iter.as_slice()).finish()
    }
}

impl<T> Clone for UnsafeLazyHoleIter<'_, T> {
    fn clone(&self) -> Self {
        UnsafeLazyHoleIter { iter: self.iter.clone() }
    }
}

impl<'a, T> Iterator for UnsafeLazyHoleIter<'a, T> {
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

impl<'a, T> DoubleEndedIterator for UnsafeLazyHoleIter<'a, T> {
    #[inline]
    fn next_back(&mut self) -> Option<&'a T> {
        self.iter.next_back()
    }
}

impl<T> ExactSizeIterator for UnsafeLazyHoleIter<'_, T> {
    fn is_empty(&self) -> bool {
        self.iter.is_empty()
    }
}

impl<T> FusedIterator for UnsafeLazyHoleIter<'_, T> {}

/// An owning iterator over the elements of a `UnsafeLazyHoleBinaryHeap`.
#[derive(Clone)]
pub struct UnsafeLazyHoleIntoIter<T, A: Allocator = Global> {
    iter: std::vec::IntoIter<T, A>,
}

impl<T, A: Allocator> UnsafeLazyHoleIntoIter<T, A> {
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.iter.allocator()
    }
}

impl<T: fmt::Debug, A: Allocator> fmt::Debug for UnsafeLazyHoleIntoIter<T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("UnsafeLazyHoleIntoIter").field(&self.iter.as_slice()).finish()
    }
}

impl<T, A: Allocator> Iterator for UnsafeLazyHoleIntoIter<T, A> {
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

impl<T, A: Allocator> DoubleEndedIterator for UnsafeLazyHoleIntoIter<T, A> {
    #[inline]
    fn next_back(&mut self) -> Option<T> {
        self.iter.next_back()
    }
}

impl<T, A: Allocator> ExactSizeIterator for UnsafeLazyHoleIntoIter<T, A> {
    fn is_empty(&self) -> bool {
        self.iter.is_empty()
    }
}

impl<T, A: Allocator> FusedIterator for UnsafeLazyHoleIntoIter<T, A> {}

impl<T> Default for UnsafeLazyHoleIntoIter<T> {
    /// Creates an empty `UnsafeLazyHoleIntoIter`.
    fn default() -> Self {
        UnsafeLazyHoleIntoIter { iter: Default::default() }
    }
}

/// An iterator that retrieves elements in heap (sorted) order, consuming the heap.
#[must_use = "iterators are lazy and do nothing unless consumed"]
#[derive(Clone, Debug)]
pub struct UnsafeLazyHoleIntoIterSorted<T, A: Allocator = Global> {
    inner: UnsafeLazyHoleBinaryHeap<T, A>,
}

impl<T, A: Allocator> UnsafeLazyHoleIntoIterSorted<T, A> {
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.inner.allocator()
    }
}

impl<T: Ord, A: Allocator> Iterator for UnsafeLazyHoleIntoIterSorted<T, A> {
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

impl<T: Ord, A: Allocator> ExactSizeIterator for UnsafeLazyHoleIntoIterSorted<T, A> {}

impl<T: Ord, A: Allocator> FusedIterator for UnsafeLazyHoleIntoIterSorted<T, A> {}

// `TrustedLen` is an unsafe PROMISE that `size_hint` is exact: lower == upper == the
// remaining count, and `next` yields exactly that many items. Length-driven consumers
// (`extend`/`collect`/`zip`) rely on it to reserve the exact capacity once and write with
// UNCHECKED stores + `set_len`, skipping per-item capacity/bounds checks — so a lying
// impl would write out of bounds (UB), which is why it is `unsafe impl`. Sound here:
// `next` is `pop` (one item per heap element) and `size_hint` is `(len, Some(len))`, so
// the count is exact. This length promise is the ONLY `unsafe` token in the safe heap; it
// does no memory-unsafe work itself.
unsafe impl<T: Ord, A: Allocator> TrustedLen for UnsafeLazyHoleIntoIterSorted<T, A> {}

/// A draining iterator over the elements of a `UnsafeLazyHoleBinaryHeap`.
#[derive(Debug)]
pub struct UnsafeLazyHoleDrain<'a, T: 'a, A: Allocator = Global> {
    iter: std::vec::Drain<'a, T, A>,
}

impl<T, A: Allocator> UnsafeLazyHoleDrain<'_, T, A> {
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.iter.allocator()
    }
}

impl<T, A: Allocator> Iterator for UnsafeLazyHoleDrain<'_, T, A> {
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

impl<T, A: Allocator> DoubleEndedIterator for UnsafeLazyHoleDrain<'_, T, A> {
    #[inline]
    fn next_back(&mut self) -> Option<T> {
        self.iter.next_back()
    }
}

impl<T, A: Allocator> ExactSizeIterator for UnsafeLazyHoleDrain<'_, T, A> {
    fn is_empty(&self) -> bool {
        self.iter.is_empty()
    }
}

impl<T, A: Allocator> FusedIterator for UnsafeLazyHoleDrain<'_, T, A> {}

/// A draining iterator over the elements of a `UnsafeLazyHoleBinaryHeap` in heap (sorted) order.
#[derive(Debug)]
pub struct UnsafeLazyHoleDrainSorted<'a, T: Ord, A: Allocator = Global> {
    inner: &'a mut UnsafeLazyHoleBinaryHeap<T, A>,
}

impl<'a, T: Ord, A: Allocator> UnsafeLazyHoleDrainSorted<'a, T, A> {
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.inner.allocator()
    }
}

impl<'a, T: Ord, A: Allocator> Drop for UnsafeLazyHoleDrainSorted<'a, T, A> {
    /// Removes heap elements in heap order.
    fn drop(&mut self) {
        struct DropGuard<'r, 'a, T: Ord, A: Allocator>(&'r mut UnsafeLazyHoleDrainSorted<'a, T, A>);

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

impl<T: Ord, A: Allocator> Iterator for UnsafeLazyHoleDrainSorted<'_, T, A> {
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

impl<T: Ord, A: Allocator> ExactSizeIterator for UnsafeLazyHoleDrainSorted<'_, T, A> {}

impl<T: Ord, A: Allocator> FusedIterator for UnsafeLazyHoleDrainSorted<'_, T, A> {}

// Sound exact-size assertion; see the UnsafeLazyHoleIntoIterSorted note above.
unsafe impl<T: Ord, A: Allocator> TrustedLen for UnsafeLazyHoleDrainSorted<'_, T, A> {}

impl<T: Ord, A: Allocator> From<Vec<T, A>> for UnsafeLazyHoleBinaryHeap<T, A> {
    /// Converts a `Vec<T>` into a `UnsafeLazyHoleBinaryHeap<T>`, in-place, *O*(*n*).
    fn from(vec: Vec<T, A>) -> UnsafeLazyHoleBinaryHeap<T, A> {
        let mut heap = UnsafeLazyHoleBinaryHeap { data: vec, possibly_dirty_root: false };
        heap.rebuild();
        heap
    }
}

impl<T: Ord, const N: usize> From<[T; N]> for UnsafeLazyHoleBinaryHeap<T> {
    fn from(arr: [T; N]) -> Self {
        Self::from_iter(arr)
    }
}

impl<T, A: Allocator> From<UnsafeLazyHoleBinaryHeap<T, A>> for Vec<T, A> {
    /// Converts a `UnsafeLazyHoleBinaryHeap<T>` into a `Vec<T>`. No data movement or allocation,
    /// constant time.
    fn from(heap: UnsafeLazyHoleBinaryHeap<T, A>) -> Vec<T, A> {
        heap.data
    }
}

impl<T: Ord> FromIterator<T> for UnsafeLazyHoleBinaryHeap<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> UnsafeLazyHoleBinaryHeap<T> {
        UnsafeLazyHoleBinaryHeap::from(iter.into_iter().collect::<Vec<_>>())
    }
}

impl<T, A: Allocator> IntoIterator for UnsafeLazyHoleBinaryHeap<T, A> {
    type Item = T;
    type IntoIter = UnsafeLazyHoleIntoIter<T, A>;

    /// Creates a consuming iterator that moves each value out of the heap in arbitrary
    /// order. The heap cannot be used after calling this.
    fn into_iter(self) -> UnsafeLazyHoleIntoIter<T, A> {
        UnsafeLazyHoleIntoIter { iter: self.data.into_iter() }
    }
}

impl<'a, T, A: Allocator> IntoIterator for &'a UnsafeLazyHoleBinaryHeap<T, A> {
    type Item = &'a T;
    type IntoIter = UnsafeLazyHoleIter<'a, T>;

    fn into_iter(self) -> UnsafeLazyHoleIter<'a, T> {
        self.iter()
    }
}

impl<T: Ord, A: Allocator> Extend<T> for UnsafeLazyHoleBinaryHeap<T, A> {
    #[inline]
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        let guard = UnsafeLazyHoleRebuildOnDrop { rebuild_from: self.len(), heap: self };
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

impl<'a, T: 'a + Ord + Copy, A: Allocator> Extend<&'a T> for UnsafeLazyHoleBinaryHeap<T, A> {
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
