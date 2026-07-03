// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! UnsafeLazyHoleResortBinaryHeap — `unsafe_lazy_hole_binary_heap` plus order recovery after a
//! comparison panic. The winner is memory-safe on a `T: Ord` panic (the `Hole` refills its slot,
//! no data loss) but silently leaves the heap order broken — nothing records it, so the next
//! operation never repairs it. This variant tracks the heap's **well-formedness** (the max-heap
//! order invariant) and repairs it lazily, at a cost matched to the damage.
//!
//! Two conservative "possibly" defects, BIT-PACKED into one byte, `possibly_mal_formed: u8`
//! (`0` == well-formed), so the gate run at every `&mut` entry point is a single test against zero
//! — only the cold path discriminates which repair:
//!   * `POSSIBLY_DIRTY_ROOT` — `peek_mut().deref_mut()` lent `&mut` to the root, so the root **key**
//!     may be out of place (we don't know it is). `repair_possibly_dirty_root`: `sift_down(0)`, *O*(log n).
//!   * `POSSIBLY_UNSORTED` — a comparison **panicked mid-sift**, abandoning the reorg, so a key
//!     anywhere (or many, after a `rebuild` panic) may be out of place. `repair_possibly_unsorted`:
//!     a full `rebuild()`, *O*(n) — the total resort. Recorded by the [`PanicProtection`] sift guard.
//!
//! So this is `unsafe_lazy_hole` with one more guarantee: after a comparison panic it is not just
//! memory-safe but **self-heals its order** on next use — strictly more correct than std, which
//! leaves the heap invariant unspecified after such a panic.
//!
//! Unchecked indexing throughout (`get_unchecked`, plus `swap_unchecked` in `into_sorted_vec`,
//! `feature(slice_swap_unchecked)` via RUSTC_BOOTSTRAP=1); real `unsafe` blocks.

use core::iter::{FusedIterator, TrustedLen};
use core::mem::{swap, ManuallyDrop};
use core::ops::{Deref, DerefMut};
use core::{fmt, ptr};
use std::alloc::{Allocator, Global};
use std::collections::TryReserveError;

// Well-formedness defects of the max-heap order invariant, bit-packed into the one-byte
// `possibly_mal_formed` field so the common-path gate is a single test against zero. Both are
// conservative ("possibly") — we don't know the order is broken, only that it might be; `0` == well-formed.
const POSSIBLY_DIRTY_ROOT: u8 = 0b01; // `peek_mut` lent `&mut` to the root: the root key may be out of place.
const POSSIBLY_UNSORTED: u8 = 0b10; //   a comparison panicked mid-sift: a key anywhere may be out of place.

/// A priority queue implemented with a binary (max-)heap — unchecked, lazy repair, with order
/// recovery after a comparison panic.
pub struct UnsafeLazyHoleResortBinaryHeap<T, A: Allocator = Global> {
    data: Vec<T, A>,
    // Bit-packed well-formedness defects (see `POSSIBLY_DIRTY_ROOT` / `POSSIBLY_UNSORTED` above).
    // `0` == well-formed (the common case). Repaired back to `0` by `repair_possibly_mal_formed`
    // (in `PeekMut::Drop` and at every `&mut` entry point).
    possibly_mal_formed: u8,
}

/// Mutable-greatest-element guard for `UnsafeLazyHoleResortBinaryHeap`. Created by
/// [`UnsafeLazyHoleResortBinaryHeap::peek_mut`].
///
/// The greatest element stays in place at `data[0]`; the guard derefs to it. `deref_mut`
/// only ORs `POSSIBLY_DIRTY_ROOT` into `possibly_mal_formed` (*O*(1), nothing moved). The re-sift
/// is deferred: `Drop` runs `repair_possibly_mal_formed` (the fast path), and if the guard is
/// FORGOTTEN the bit survives and the heap's next `&mut` operation repairs it — so a forgotten
/// mutated guard loses nothing and leaks nothing.
pub struct UnsafeLazyHoleResortPeekMut<'a, T: 'a + Ord, A: Allocator = Global> {
    heap: &'a mut UnsafeLazyHoleResortBinaryHeap<T, A>,
}

impl<T: Ord + fmt::Debug, A: Allocator> fmt::Debug for UnsafeLazyHoleResortPeekMut<'_, T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // SAFETY: a UnsafeLazyHoleResortPeekMut is only created for a non-empty heap.
        f.debug_tuple("UnsafeLazyHoleResortPeekMut").field(unsafe { self.heap.data.get_unchecked(0) }).finish()
    }
}

impl<T: Ord, A: Allocator> Drop for UnsafeLazyHoleResortPeekMut<'_, T, A> {
    fn drop(&mut self) {
        // Fast path: repair the (possibly) dirtied root now. If the guard is forgotten
        // instead, this never runs — but the flag survives and the heap self-heals on next use.
        self.heap.repair_possibly_mal_formed();
    }
}

impl<T: Ord, A: Allocator> Deref for UnsafeLazyHoleResortPeekMut<'_, T, A> {
    type Target = T;
    fn deref(&self) -> &T {
        debug_assert!(!self.heap.is_empty());
        // SAFETY: only created for a non-empty heap, so index 0 is in bounds.
        unsafe { self.heap.data.get_unchecked(0) }
    }
}

impl<T: Ord, A: Allocator> DerefMut for UnsafeLazyHoleResortPeekMut<'_, T, A> {
    fn deref_mut(&mut self) -> &mut T {
        debug_assert!(!self.heap.is_empty());
        // O(1): mark the root as possibly-dirty and hand out `&mut data[0]`. The repair is
        // deferred (Drop / next op), so this never touches or moves any other element, and a
        // forget cannot lose or leak data. Only set the flag for len > 1 (a singleton is
        // always a valid heap whatever its value, so it never needs a repair).
        if self.heap.len() > 1 {
            // post-repair the heap is well-formed; lending `&mut` to the root may break it at the
            // root only (peek_mut repaired at entry, so the OR-in is effectively a set).
            self.heap.possibly_mal_formed |= POSSIBLY_DIRTY_ROOT;
        }
        // SAFETY: only created for a non-empty heap, so index 0 is in bounds.
        unsafe { self.heap.data.get_unchecked_mut(0) }
    }
}

impl<'a, T: Ord, A: Allocator> UnsafeLazyHoleResortPeekMut<'a, T, A> {
    /// Sifts the current element to its new position. Afterwards refers to the new
    /// element. Returns whether the maximum changed.
    #[must_use = "is equivalent to dropping and getting a new UnsafeLazyHoleResortPeekMut except for return information"]
    pub fn refresh(&mut self) -> bool {
        // Sift the (possibly mutated) root down now; it changed iff it moved off index 0.
        let moved = self.heap.sift_down(0) != 0;
        // sift_down repaired the root (a comparison panic would have set POSSIBLY_UNSORTED via the
        // sift's panic protection instead of reaching here); clear the dirty-root bit.
        self.heap.possibly_mal_formed &= !POSSIBLY_DIRTY_ROOT;
        moved
    }

    /// Removes the peeked value from the heap and returns it.
    pub fn pop(this: UnsafeLazyHoleResortPeekMut<'a, T, A>) -> T {
        // Remove the (possibly mutated) root and re-heapify the rest; the root is gone, so
        // clear the flag (Drop then no-ops).
        this.heap.possibly_mal_formed &= !POSSIBLY_DIRTY_ROOT;
        let val = this.heap.data.swap_remove(0);
        if !this.heap.is_empty() {
            this.heap.sift_down(0);
        }
        val
    }
}

impl<T: Clone, A: Allocator + Clone> Clone for UnsafeLazyHoleResortBinaryHeap<T, A> {
    fn clone(&self) -> Self {
        // Preserve the flag: cloning a dirty heap must yield a dirty clone, not a "clean" heap
        // with an unsorted root.
        UnsafeLazyHoleResortBinaryHeap { data: self.data.clone(), possibly_mal_formed: self.possibly_mal_formed }
    }

    fn clone_from(&mut self, source: &Self) {
        self.data.clone_from(&source.data);
        self.possibly_mal_formed = source.possibly_mal_formed;
    }
}

impl<T> Default for UnsafeLazyHoleResortBinaryHeap<T> {
    /// Creates an empty `UnsafeLazyHoleResortBinaryHeap<T>`.
    #[inline]
    fn default() -> UnsafeLazyHoleResortBinaryHeap<T> {
        UnsafeLazyHoleResortBinaryHeap::new()
    }
}

impl<T: fmt::Debug, A: Allocator> fmt::Debug for UnsafeLazyHoleResortBinaryHeap<T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

/// Drop guard that repairs the heap tail when an in-place mutation (extend / retain)
/// finishes or unwinds.
struct UnsafeLazyHoleResortRebuildOnDrop<'a, T: Ord, A: Allocator = Global> {
    heap: &'a mut UnsafeLazyHoleResortBinaryHeap<T, A>,
    rebuild_from: usize,
}

impl<T: Ord, A: Allocator> Drop for UnsafeLazyHoleResortRebuildOnDrop<'_, T, A> {
    fn drop(&mut self) {
        self.heap.rebuild_tail(self.rebuild_from);
    }
}

impl<T> UnsafeLazyHoleResortBinaryHeap<T> {
    /// Creates an empty `UnsafeLazyHoleResortBinaryHeap` as a max-heap.
    #[must_use]
    pub const fn new() -> UnsafeLazyHoleResortBinaryHeap<T> {
        UnsafeLazyHoleResortBinaryHeap { data: Vec::new(), possibly_mal_formed: 0 }
    }

    /// Creates an empty `UnsafeLazyHoleResortBinaryHeap` with at least the specified capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> UnsafeLazyHoleResortBinaryHeap<T> {
        UnsafeLazyHoleResortBinaryHeap { data: Vec::with_capacity(capacity), possibly_mal_formed: 0 }
    }
}

impl<T, A: Allocator> UnsafeLazyHoleResortBinaryHeap<T, A> {
    /// Creates an empty `UnsafeLazyHoleResortBinaryHeap` as a max-heap, using `A` as allocator.
    #[must_use]
    pub const fn new_in(alloc: A) -> UnsafeLazyHoleResortBinaryHeap<T, A> {
        UnsafeLazyHoleResortBinaryHeap { data: Vec::new_in(alloc), possibly_mal_formed: 0 }
    }

    /// Creates an empty `UnsafeLazyHoleResortBinaryHeap` with at least the specified capacity, using `A`.
    #[must_use]
    pub fn with_capacity_in(capacity: usize, alloc: A) -> UnsafeLazyHoleResortBinaryHeap<T, A> {
        UnsafeLazyHoleResortBinaryHeap { data: Vec::with_capacity_in(capacity, alloc), possibly_mal_formed: 0 }
    }

    /// Creates a `UnsafeLazyHoleResortBinaryHeap` from the supplied `vec` without rebuilding it.
    ///
    /// Logically `vec` must already be a max-heap; unlike the unsafe heap this is a SAFE
    /// fn (a non-heap input only produces wrong results, never undefined behavior).
    #[must_use]
    pub fn from_raw_vec(vec: Vec<T, A>) -> UnsafeLazyHoleResortBinaryHeap<T, A> {
        UnsafeLazyHoleResortBinaryHeap { data: vec, possibly_mal_formed: 0 }
    }
}

impl<T: Ord, A: Allocator> UnsafeLazyHoleResortBinaryHeap<T, A> {
    /// Returns the greatest item, or `None` if empty.
    ///
    /// Note: unlike std, `peek` here requires `Ord`, because the max is not always at `data[0]`:
    ///   * `POSSIBLY_DIRTY_ROOT` (a forgotten mutated `peek_mut`): the root may have sunk, but the
    ///     subtrees stay valid heaps, so the max is the root or one of its ≤ 2 children — *O*(1).
    ///   * `POSSIBLY_UNSORTED` (a comparison panicked mid-sift): order may be broken anywhere and
    ///     `&self` cannot resort, so the max is found by an *O*(n) scan. Rare (post-panic window).
    #[must_use]
    pub fn peek(&self) -> Option<&T> {
        let len = self.data.len();
        if len == 0 {
            return None;
        }
        if self.possibly_mal_formed == 0 {
            // SAFETY: len > 0.
            return Some(unsafe { self.data.get_unchecked(0) });
        }
        if self.possibly_mal_formed & POSSIBLY_UNSORTED != 0 {
            return self.data.iter().max();
        }
        // POSSIBLY_DIRTY_ROOT only: the max is the root or one of its ≤ 2 immediate children.
        // SAFETY: every index touched is < len.
        unsafe {
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
    pub fn peek_mut(&mut self) -> Option<UnsafeLazyHoleResortPeekMut<'_, T, A>> {
        self.repair_possibly_mal_formed(); // heal a prior forgotten guard before lending again
        if self.is_empty() {
            None
        } else {
            Some(UnsafeLazyHoleResortPeekMut { heap: self })
        }
    }

    /// Removes the greatest item and returns it, or `None` if empty.
    pub fn pop(&mut self) -> Option<T> {
        self.repair_possibly_mal_formed();
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
        self.repair_possibly_mal_formed();
        let old_len = self.len();
        self.data.push(item);
        self.sift_up(0, old_len);
    }

    /// Consumes the heap and returns a vector in sorted (ascending) order.
    #[must_use = "`self` will be dropped if the result is not used"]
    pub fn into_sorted_vec(mut self) -> Vec<T, A> {
        self.repair_possibly_mal_formed();
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
        let Self { data, possibly_mal_formed, .. } = self;
        // Panic protection: if a comparison panics mid-sift the `Hole` refills its slot (no data
        // loss), but the order is left broken — record it so the next op resorts. Cleared on success.
        let mut protection = PanicProtection { possibly_mal_formed, sift_completed: false };
        // SAFETY: pos < self.len() (caller invariant); every index touched (parent < hole.pos)
        // is < pos < len, and != hole.pos.
        let final_pos = unsafe {
            let mut hole = Hole::new(data, pos);
            while hole.pos() > start {
                let parent = (hole.pos() - 1) / 2;
                if hole.element() <= hole.get(parent) {
                    break;
                }
                hole.move_to(parent);
            }
            hole.pos()
        };
        protection.sift_completed = true;
        final_pos
    }

    /// Move the element at `pos` down within `[pos, end)` while smaller than its greater
    /// child. Returns the final index. Hole-based and panic-safe (see `sift_up`).
    fn sift_down_range(&mut self, pos: usize, end: usize) -> usize {
        let Self { data, possibly_mal_formed, .. } = self;
        // See `sift_up`: panic protection — a mid-sift comparison panic refills the hole (no data
        // loss) but leaves the order broken, so the next op must resort. Cleared on success.
        let mut protection = PanicProtection { possibly_mal_formed, sift_completed: false };
        // SAFETY: pos < end <= self.len() (caller invariant); every index touched is < end.
        let final_pos = 'sift: {
            unsafe {
                let mut hole = Hole::new(data, pos);
                let mut child = 2 * hole.pos() + 1;
                while child <= end.saturating_sub(2) {
                    child += (hole.get(child) <= hole.get(child + 1)) as usize;
                    if hole.element() >= hole.get(child) {
                        break 'sift hole.pos();
                    }
                    hole.move_to(child);
                    child = 2 * hole.pos() + 1;
                }
                if child == end - 1 && hole.element() < hole.get(child) {
                    hole.move_to(child);
                }
                hole.pos()
            }
        };
        protection.sift_completed = true;
        final_pos
    }

    fn sift_down(&mut self, pos: usize) -> usize {
        let len = self.len();
        self.sift_down_range(pos, len)
    }

    /// Repairs the heap's well-formedness if either "possibly" defect is set, then marks it
    /// well-formed. *O*(1) in the common case (one test against zero — the whole reason `deref_mut`
    /// could be *O*(1)); the actual repair is out-of-line so the ~20 inlined gates at `&mut` entry
    /// points stay a single compare + branch. Idempotent — safe to call at the start of every op.
    #[inline]
    fn repair_possibly_mal_formed(&mut self) {
        if self.possibly_mal_formed != 0 {
            self.repair_possibly_mal_formed_cold();
        }
    }

    #[inline(never)]
    fn repair_possibly_mal_formed_cold(&mut self) {
        // POSSIBLY_UNSORTED dominates: its full resort also repairs a dirty root.
        if self.possibly_mal_formed & POSSIBLY_UNSORTED != 0 {
            self.repair_possibly_unsorted();
        } else if self.possibly_mal_formed & POSSIBLY_DIRTY_ROOT != 0 {
            self.repair_possibly_dirty_root();
        }
    }

    /// Repair a possibly-dirty root: only the root key may be out of place, the subtrees are valid
    /// — one `sift_down(0)`, *O*(log n).
    fn repair_possibly_dirty_root(&mut self) {
        if self.len() > 1 {
            self.sift_down(0);
        }
        // Reached only on success; a comparison panic sets POSSIBLY_UNSORTED via the sift's panic protection.
        self.possibly_mal_formed &= !POSSIBLY_DIRTY_ROOT;
    }

    /// Repair a possibly-unsorted heap: a key anywhere may be out of place — a full `rebuild()`,
    /// *O*(n) total resort, which also subsumes a dirty root.
    fn repair_possibly_unsorted(&mut self) {
        self.rebuild();
        self.possibly_mal_formed = 0;
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
        self.repair_possibly_mal_formed();
        other.repair_possibly_mal_formed();
        if self.len() < other.len() {
            swap(self, other);
        }

        let start = self.data.len();

        self.data.append(&mut other.data);

        self.rebuild_tail(start);
    }

    /// Clears the heap, returning an iterator over the removed elements in heap order.
    #[inline]
    pub fn drain_sorted(&mut self) -> UnsafeLazyHoleResortDrainSorted<'_, T, A> {
        UnsafeLazyHoleResortDrainSorted { inner: self }
    }

    /// Retains only the elements specified by the predicate, in unspecified order.
    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&T) -> bool,
    {
        self.repair_possibly_mal_formed();
        let mut guard = UnsafeLazyHoleResortRebuildOnDrop { rebuild_from: self.len(), heap: self };
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

impl<T, A: Allocator> UnsafeLazyHoleResortBinaryHeap<T, A> {
    /// Returns an iterator visiting all values in the underlying vector, in arbitrary
    /// order.
    pub fn iter(&self) -> UnsafeLazyHoleResortIter<'_, T> {
        UnsafeLazyHoleResortIter { iter: self.data.iter() }
    }

    /// Returns an iterator which retrieves elements in heap order. Consumes the heap.
    pub fn into_iter_sorted(self) -> UnsafeLazyHoleResortIntoIterSorted<T, A> {
        UnsafeLazyHoleResortIntoIterSorted { inner: self }
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
    pub fn drain(&mut self) -> UnsafeLazyHoleResortDrain<'_, T, A> {
        // The (possibly dirty) root is about to be removed along with everything else; the
        // emptied heap is trivially clean. (No `Ord` here, so just clear the flag — order is
        // irrelevant to a full drain anyway.)
        self.possibly_mal_formed = 0;
        UnsafeLazyHoleResortDrain { iter: self.data.drain(..) }
    }

    /// Drops all items from the heap.
    pub fn clear(&mut self) {
        self.drain();
    }
}

/// Panic protection for a sift. While a sift runs, the heap order is being rebuilt and is
/// transiently ill-formed. If a `T: Ord` comparison panics mid-sift, the `Hole` refills its slot
/// (no data loss) but the order is left broken — this guard's `Drop` then ORs in `POSSIBLY_UNSORTED`
/// so the next operation does a full resort. The sift sets `sift_completed = true` once it finishes
/// normally, so on the normal path the `Drop` does nothing. Declared before the `Hole` in each sift,
/// so on a panic the `Hole` drops first (refills the slot) and then this guard marks the order suspect.
struct PanicProtection<'a> {
    possibly_mal_formed: &'a mut u8,
    sift_completed: bool,
}

impl Drop for PanicProtection<'_> {
    fn drop(&mut self) {
        // If the sift did not finish (a comparison panicked), the order may be broken anywhere.
        if !self.sift_completed {
            *self.possibly_mal_formed |= POSSIBLY_UNSORTED;
        }
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

/// An iterator over the elements of a `UnsafeLazyHoleResortBinaryHeap`.
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct UnsafeLazyHoleResortIter<'a, T: 'a> {
    iter: std::slice::Iter<'a, T>,
}

impl<T> Default for UnsafeLazyHoleResortIter<'_, T> {
    /// Creates an empty `UnsafeLazyHoleResortIter`.
    fn default() -> Self {
        UnsafeLazyHoleResortIter { iter: Default::default() }
    }
}

impl<T: fmt::Debug> fmt::Debug for UnsafeLazyHoleResortIter<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("UnsafeLazyHoleResortIter").field(&self.iter.as_slice()).finish()
    }
}

impl<T> Clone for UnsafeLazyHoleResortIter<'_, T> {
    fn clone(&self) -> Self {
        UnsafeLazyHoleResortIter { iter: self.iter.clone() }
    }
}

impl<'a, T> Iterator for UnsafeLazyHoleResortIter<'a, T> {
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

impl<'a, T> DoubleEndedIterator for UnsafeLazyHoleResortIter<'a, T> {
    #[inline]
    fn next_back(&mut self) -> Option<&'a T> {
        self.iter.next_back()
    }
}

impl<T> ExactSizeIterator for UnsafeLazyHoleResortIter<'_, T> {
    fn is_empty(&self) -> bool {
        self.iter.is_empty()
    }
}

impl<T> FusedIterator for UnsafeLazyHoleResortIter<'_, T> {}

/// An owning iterator over the elements of a `UnsafeLazyHoleResortBinaryHeap`.
#[derive(Clone)]
pub struct UnsafeLazyHoleResortIntoIter<T, A: Allocator = Global> {
    iter: std::vec::IntoIter<T, A>,
}

impl<T, A: Allocator> UnsafeLazyHoleResortIntoIter<T, A> {
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.iter.allocator()
    }
}

impl<T: fmt::Debug, A: Allocator> fmt::Debug for UnsafeLazyHoleResortIntoIter<T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("UnsafeLazyHoleResortIntoIter").field(&self.iter.as_slice()).finish()
    }
}

impl<T, A: Allocator> Iterator for UnsafeLazyHoleResortIntoIter<T, A> {
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

impl<T, A: Allocator> DoubleEndedIterator for UnsafeLazyHoleResortIntoIter<T, A> {
    #[inline]
    fn next_back(&mut self) -> Option<T> {
        self.iter.next_back()
    }
}

impl<T, A: Allocator> ExactSizeIterator for UnsafeLazyHoleResortIntoIter<T, A> {
    fn is_empty(&self) -> bool {
        self.iter.is_empty()
    }
}

impl<T, A: Allocator> FusedIterator for UnsafeLazyHoleResortIntoIter<T, A> {}

impl<T> Default for UnsafeLazyHoleResortIntoIter<T> {
    /// Creates an empty `UnsafeLazyHoleResortIntoIter`.
    fn default() -> Self {
        UnsafeLazyHoleResortIntoIter { iter: Default::default() }
    }
}

/// An iterator that retrieves elements in heap (sorted) order, consuming the heap.
#[must_use = "iterators are lazy and do nothing unless consumed"]
#[derive(Clone, Debug)]
pub struct UnsafeLazyHoleResortIntoIterSorted<T, A: Allocator = Global> {
    inner: UnsafeLazyHoleResortBinaryHeap<T, A>,
}

impl<T, A: Allocator> UnsafeLazyHoleResortIntoIterSorted<T, A> {
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.inner.allocator()
    }
}

impl<T: Ord, A: Allocator> Iterator for UnsafeLazyHoleResortIntoIterSorted<T, A> {
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

impl<T: Ord, A: Allocator> ExactSizeIterator for UnsafeLazyHoleResortIntoIterSorted<T, A> {}

impl<T: Ord, A: Allocator> FusedIterator for UnsafeLazyHoleResortIntoIterSorted<T, A> {}

// `TrustedLen` is an unsafe PROMISE that `size_hint` is exact: lower == upper == the
// remaining count, and `next` yields exactly that many items. Length-driven consumers
// (`extend`/`collect`/`zip`) rely on it to reserve the exact capacity once and write with
// UNCHECKED stores + `set_len`, skipping per-item capacity/bounds checks — so a lying
// impl would write out of bounds (UB), which is why it is `unsafe impl`. Sound here:
// `next` is `pop` (one item per heap element) and `size_hint` is `(len, Some(len))`, so
// the count is exact. This length promise is the ONLY `unsafe` token in the safe heap; it
// does no memory-unsafe work itself.
unsafe impl<T: Ord, A: Allocator> TrustedLen for UnsafeLazyHoleResortIntoIterSorted<T, A> {}

/// A draining iterator over the elements of a `UnsafeLazyHoleResortBinaryHeap`.
#[derive(Debug)]
pub struct UnsafeLazyHoleResortDrain<'a, T: 'a, A: Allocator = Global> {
    iter: std::vec::Drain<'a, T, A>,
}

impl<T, A: Allocator> UnsafeLazyHoleResortDrain<'_, T, A> {
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.iter.allocator()
    }
}

impl<T, A: Allocator> Iterator for UnsafeLazyHoleResortDrain<'_, T, A> {
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

impl<T, A: Allocator> DoubleEndedIterator for UnsafeLazyHoleResortDrain<'_, T, A> {
    #[inline]
    fn next_back(&mut self) -> Option<T> {
        self.iter.next_back()
    }
}

impl<T, A: Allocator> ExactSizeIterator for UnsafeLazyHoleResortDrain<'_, T, A> {
    fn is_empty(&self) -> bool {
        self.iter.is_empty()
    }
}

impl<T, A: Allocator> FusedIterator for UnsafeLazyHoleResortDrain<'_, T, A> {}

/// A draining iterator over the elements of a `UnsafeLazyHoleResortBinaryHeap` in heap (sorted) order.
#[derive(Debug)]
pub struct UnsafeLazyHoleResortDrainSorted<'a, T: Ord, A: Allocator = Global> {
    inner: &'a mut UnsafeLazyHoleResortBinaryHeap<T, A>,
}

impl<'a, T: Ord, A: Allocator> UnsafeLazyHoleResortDrainSorted<'a, T, A> {
    /// Returns a reference to the underlying allocator.
    pub fn allocator(&self) -> &A {
        self.inner.allocator()
    }
}

impl<'a, T: Ord, A: Allocator> Drop for UnsafeLazyHoleResortDrainSorted<'a, T, A> {
    /// Removes heap elements in heap order.
    fn drop(&mut self) {
        struct DropGuard<'r, 'a, T: Ord, A: Allocator>(&'r mut UnsafeLazyHoleResortDrainSorted<'a, T, A>);

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

impl<T: Ord, A: Allocator> Iterator for UnsafeLazyHoleResortDrainSorted<'_, T, A> {
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

impl<T: Ord, A: Allocator> ExactSizeIterator for UnsafeLazyHoleResortDrainSorted<'_, T, A> {}

impl<T: Ord, A: Allocator> FusedIterator for UnsafeLazyHoleResortDrainSorted<'_, T, A> {}

// Sound exact-size assertion; see the UnsafeLazyHoleResortIntoIterSorted note above.
unsafe impl<T: Ord, A: Allocator> TrustedLen for UnsafeLazyHoleResortDrainSorted<'_, T, A> {}

impl<T: Ord, A: Allocator> From<Vec<T, A>> for UnsafeLazyHoleResortBinaryHeap<T, A> {
    /// Converts a `Vec<T>` into a `UnsafeLazyHoleResortBinaryHeap<T>`, in-place, *O*(*n*).
    fn from(vec: Vec<T, A>) -> UnsafeLazyHoleResortBinaryHeap<T, A> {
        let mut heap = UnsafeLazyHoleResortBinaryHeap { data: vec, possibly_mal_formed: 0 };
        heap.rebuild();
        heap
    }
}

impl<T: Ord, const N: usize> From<[T; N]> for UnsafeLazyHoleResortBinaryHeap<T> {
    fn from(arr: [T; N]) -> Self {
        Self::from_iter(arr)
    }
}

impl<T, A: Allocator> From<UnsafeLazyHoleResortBinaryHeap<T, A>> for Vec<T, A> {
    /// Converts a `UnsafeLazyHoleResortBinaryHeap<T>` into a `Vec<T>`. No data movement or allocation,
    /// constant time.
    fn from(heap: UnsafeLazyHoleResortBinaryHeap<T, A>) -> Vec<T, A> {
        heap.data
    }
}

impl<T: Ord> FromIterator<T> for UnsafeLazyHoleResortBinaryHeap<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> UnsafeLazyHoleResortBinaryHeap<T> {
        UnsafeLazyHoleResortBinaryHeap::from(iter.into_iter().collect::<Vec<_>>())
    }
}

impl<T, A: Allocator> IntoIterator for UnsafeLazyHoleResortBinaryHeap<T, A> {
    type Item = T;
    type IntoIter = UnsafeLazyHoleResortIntoIter<T, A>;

    /// Creates a consuming iterator that moves each value out of the heap in arbitrary
    /// order. The heap cannot be used after calling this.
    fn into_iter(self) -> UnsafeLazyHoleResortIntoIter<T, A> {
        UnsafeLazyHoleResortIntoIter { iter: self.data.into_iter() }
    }
}

impl<'a, T, A: Allocator> IntoIterator for &'a UnsafeLazyHoleResortBinaryHeap<T, A> {
    type Item = &'a T;
    type IntoIter = UnsafeLazyHoleResortIter<'a, T>;

    fn into_iter(self) -> UnsafeLazyHoleResortIter<'a, T> {
        self.iter()
    }
}

impl<T: Ord, A: Allocator> Extend<T> for UnsafeLazyHoleResortBinaryHeap<T, A> {
    #[inline]
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        let guard = UnsafeLazyHoleResortRebuildOnDrop { rebuild_from: self.len(), heap: self };
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

impl<'a, T: 'a + Ord + Copy, A: Allocator> Extend<&'a T> for UnsafeLazyHoleResortBinaryHeap<T, A> {
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
