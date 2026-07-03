// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! `VRawVec` — the storage backing for `VVec`, standing in for `alloc`'s internal
//! `crate::raw_vec::RawVec`.
//!
//! WHY A SHIM (and not a faithful byte-copy of `RawVec`): `Vec` is built on
//! `alloc::raw_vec::RawVec<T, A>`, a module-private type that cannot be named outside
//! `alloc`. The faithful move would be to extract `raw_vec/mod.rs` too — but the
//! rust-libs 1.96.0 `RawVec` source uses the new const-trait surface syntax
//! (`const impl<T, A: [const] Allocator + [const] Destruct>`, `[const]` bounds) plus
//! deeply-internal niche types (`core::num::niche_types::UsizeNoHighBit`, `Unique`,
//! `Alignment`, `SizedTypeProperties`). The installed toolchain is rustc 1.95.0, one
//! release behind the source; it cannot even parse `const impl` / `[const]` bounds, so a
//! faithful `RawVec` copy does not compile here. That is a genuine toolchain blocker, not
//! a logic gap.
//!
//! So this module re-provides EXACTLY the `RawVec` API surface that `VVec` calls,
//! implemented over the stable, public `alloc::vec::Vec<T, A>`. This keeps the `VVec`
//! body BYTE-FAITHFUL (every `RawVec` becomes `VRawVec`, nothing else changes) and keeps
//! the amortized-growth policy identical, because `Vec<T, A>` is itself built on the same
//! `RawVec` — so `Vec::reserve` / `reserve_exact` / `try_reserve*` apply the very same
//! growth code that `RawVec::reserve` does.
//!
//! The single mechanism: `RawVec::reserve(len, additional)` (and `grow_one`) preserves the
//! first `len` elements (a raw `memcpy`, never reading them as `T`) when it reallocates.
//! We reproduce that by setting the backing `Vec`'s length to `len` across the
//! `Vec::reserve(additional)` call and resetting it to `0` immediately after: `Vec::reserve`
//! on a length-`len` vector ensures capacity for `len + additional` and `memcpy`s exactly
//! `len` elements on realloc — identical to `RawVec`. The backing `Vec` is held at length
//! `0` at all other times so it NEVER drops or reads any element; `VVec` owns every
//! element's lifetime, exactly as it does over `RawVec`. On drop, the length-`0` `Vec`
//! frees the buffer only — matching `RawVec`'s deallocation-only drop.

use core::mem::MaybeUninit;
use core::ptr::NonNull;
use std::alloc::{Allocator, Global};
use std::collections::TryReserveError;

/// `RawVec::MIN_NON_ZERO_CAP` policy: the smallest non-zero capacity `RawVec` grows to,
/// chosen by element size (copied verbatim from `alloc::raw_vec::min_non_zero_cap`).
const fn min_non_zero_cap(size: usize) -> usize {
    if size == 1 {
        8
    } else if size <= 1024 {
        4
    } else {
        1
    }
}

/// Storage backing for `VVec`. Holds an `alloc::vec::Vec<T, A>` whose length is kept
/// at `0` except transiently inside a reserve call (see the module docs). Mirrors the
/// `RawVec<T, A>` API surface that `VVec` uses.
pub(super) struct VRawVec<T, A: Allocator = Global> {
    inner: Vec<T, A>,
}

impl<T> VRawVec<T, Global> {
    #[inline]
    pub(super) const fn new() -> Self {
        VRawVec { inner: Vec::new() }
    }
}

impl<T, A: Allocator> VRawVec<T, A> {
    /// `RawVec::MIN_NON_ZERO_CAP`: the smallest non-zero capacity the growth policy uses
    /// for `T`. Read by `spec_from_iter_nested` when sizing the initial allocation.
    pub(super) const MIN_NON_ZERO_CAP: usize = min_non_zero_cap(size_of::<T>());

    #[inline]
    pub(super) const fn new_in(alloc: A) -> Self {
        VRawVec { inner: Vec::new_in(alloc) }
    }

    #[inline]
    pub(super) fn with_capacity_in(capacity: usize, alloc: A) -> Self {
        VRawVec { inner: Vec::with_capacity_in(capacity, alloc) }
    }

    /// `RawVec::try_with_capacity_in(capacity, alloc)`. Allocates room for `capacity`
    /// elements (or fails with `TryReserveError`), length kept at `0`.
    #[inline]
    pub(super) fn try_with_capacity_in(
        capacity: usize,
        alloc: A,
    ) -> Result<Self, TryReserveError> {
        let mut inner = Vec::new_in(alloc);
        inner.try_reserve_exact(capacity)?;
        Ok(VRawVec { inner })
    }

    /// `RawVec::into_box(len)`: relinquish the allocation as a boxed slice of `len`
    /// `MaybeUninit<T>` (the caller `assume_init`s it). The first `len` elements are the
    /// ones the owning `VVec` initialized; `VVec::into_boxed_slice` shrinks so `len ==
    /// capacity` before calling this, so no reallocation happens.
    #[inline]
    pub(super) fn into_box(self, len: usize) -> Box<[MaybeUninit<T>], A> {
        let mut inner = self.inner;
        // SAFETY: `len <= capacity` and the first `len` elements are initialized by the
        // owning VVec; we hand the allocation off as MaybeUninit so the caller owns drop.
        unsafe { inner.set_len(len) };
        let boxed: Box<[T], A> = inner.into_boxed_slice();
        let (ptr, alloc) = Box::into_raw_with_allocator(boxed);
        // SAFETY: `[T]` and `[MaybeUninit<T>]` share layout and slice metadata; this only
        // reinterprets the element type, transferring ownership of the same allocation.
        unsafe { Box::from_raw_in(ptr as *mut [MaybeUninit<T>], alloc) }
    }

    /// `RawVec::from_raw_parts_in`: adopt an existing allocation of `capacity` elements.
    ///
    /// # Safety
    /// Same contract as `RawVec::from_raw_parts_in` / `Vec::from_raw_parts_in` with a
    /// length of `0`: `ptr` is a valid allocation from `alloc` for `capacity` elements.
    #[inline]
    pub(super) unsafe fn from_raw_parts_in(ptr: *mut T, capacity: usize, alloc: A) -> Self {
        // SAFETY: forwarded contract; length 0 because the vec tracks its own elements.
        VRawVec { inner: unsafe { Vec::from_raw_parts_in(ptr, 0, capacity, alloc) } }
    }

    /// `RawVec::from_nonnull_in`: adopt an existing allocation given a `NonNull` pointer.
    ///
    /// # Safety
    /// Same contract as `RawVec::from_nonnull_in`: `ptr` is a valid allocation from
    /// `alloc` for `capacity` elements.
    #[inline]
    pub(super) unsafe fn from_nonnull_in(ptr: NonNull<T>, capacity: usize, alloc: A) -> Self {
        // SAFETY: forwarded contract; length 0 because the vec tracks its own elements.
        VRawVec {
            inner: unsafe { Vec::from_raw_parts_in(ptr.as_ptr(), 0, capacity, alloc) },
        }
    }

    #[inline]
    pub(super) fn ptr(&self) -> *mut T {
        self.inner.as_ptr() as *mut T
    }

    /// `RawVec::non_null`: the allocation pointer as a `NonNull<T>` (dangling-but-aligned
    /// when capacity is 0, exactly like `Vec::as_ptr`).
    #[inline]
    pub(super) fn non_null(&self) -> NonNull<T> {
        // SAFETY: `Vec::as_ptr` never returns null — it returns a dangling, aligned
        // pointer for a zero-capacity buffer — matching `RawVec::non_null`.
        unsafe { NonNull::new_unchecked(self.inner.as_ptr() as *mut T) }
    }

    #[inline]
    pub(super) fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    #[inline]
    pub(super) fn allocator(&self) -> &A {
        self.inner.allocator()
    }

    /// `RawVec::reserve(len, additional)`: ensure capacity for `len + additional`,
    /// preserving the first `len` elements on realloc.
    #[inline]
    pub(super) fn reserve(&mut self, len: usize, additional: usize) {
        // SAFETY: `len <= self.capacity()` (vec invariant `len <= capacity`); the backing
        // Vec is length 0, so setting it to `len` only tells `reserve` how many elements to
        // memcpy on realloc — exactly RawVec's behavior. Reset to 0 immediately after.
        unsafe { self.inner.set_len(len) };
        self.inner.reserve(additional);
        unsafe { self.inner.set_len(0) };
    }

    /// `RawVec::reserve_exact(len, additional)`.
    #[inline]
    pub(super) fn reserve_exact(&mut self, len: usize, additional: usize) {
        // SAFETY: see `reserve`.
        unsafe { self.inner.set_len(len) };
        self.inner.reserve_exact(additional);
        unsafe { self.inner.set_len(0) };
    }

    /// `RawVec::try_reserve(len, additional)`.
    #[inline]
    pub(super) fn try_reserve(
        &mut self,
        len: usize,
        additional: usize,
    ) -> Result<(), TryReserveError> {
        // SAFETY: see `reserve`.
        unsafe { self.inner.set_len(len) };
        let r = self.inner.try_reserve(additional);
        unsafe { self.inner.set_len(0) };
        r
    }

    /// `RawVec::try_reserve_exact(len, additional)`.
    #[inline]
    pub(super) fn try_reserve_exact(
        &mut self,
        len: usize,
        additional: usize,
    ) -> Result<(), TryReserveError> {
        // SAFETY: see `reserve`.
        unsafe { self.inner.set_len(len) };
        let r = self.inner.try_reserve_exact(additional);
        unsafe { self.inner.set_len(0) };
        r
    }

    /// `RawVec::shrink_to_fit(cap)`: shrink the allocation to hold `cap` elements,
    /// preserving the first `cap` elements.
    #[inline]
    pub(super) fn shrink_to_fit(&mut self, cap: usize) {
        // SAFETY: `cap <= self.capacity()` (we are shrinking); see `reserve` for the
        // set_len dance — it only controls how many elements `shrink_to` preserves.
        unsafe { self.inner.set_len(cap) };
        self.inner.shrink_to(cap);
        unsafe { self.inner.set_len(0) };
    }

    /// `RawVec::try_shrink_to_fit(cap)`. `Vec::shrink_to` is infallible over the public
    /// API (it aborts on the rare shrink-realloc failure rather than returning), so this
    /// always returns `Ok(())` after preserving the first `cap` elements.
    #[inline]
    pub(super) fn try_shrink_to_fit(&mut self, cap: usize) -> Result<(), TryReserveError> {
        // SAFETY: see `shrink_to_fit`.
        unsafe { self.inner.set_len(cap) };
        self.inner.shrink_to(cap);
        unsafe { self.inner.set_len(0) };
        Ok(())
    }

    /// `RawVec::grow_one`: grow the allocation by at least one element, preserving the
    /// entire current buffer. `VVec::push` only calls this when full (`len == capacity`),
    /// so the whole physical buffer is initialized and preserving `capacity` is correct.
    #[inline]
    pub(super) fn grow_one(&mut self) {
        let cap = self.inner.capacity();
        // SAFETY: see `reserve`; `cap == capacity` so the whole buffer is preserved.
        unsafe { self.inner.set_len(cap) };
        self.inner.reserve(1);
        unsafe { self.inner.set_len(0) };
    }
}
