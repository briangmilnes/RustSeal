// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.
use core::ops::{Deref, DerefMut};

use super::VVec;
use std::alloc::{Allocator, Global};
use std::fmt;

/// Structure wrapping a mutable reference to the last item in a
/// `VVec`.
///
/// This `struct` is created by the [`peek_mut`] method on [`VVec`]. See
/// its documentation for more.
///
/// [`peek_mut`]: VVec::peek_mut

pub struct VVecPeekMut<
    'a,
    T,
    A: Allocator = Global,
> {
    vec: &'a mut VVec<T, A>,
}

impl<T: fmt::Debug, A: Allocator> fmt::Debug for VVecPeekMut<'_, T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("VVecPeekMut").field(self.deref()).finish()
    }
}

impl<'a, T, A: Allocator> VVecPeekMut<'a, T, A> {
    pub(super) fn new(vec: &'a mut VVec<T, A>) -> Option<Self> {
        if vec.is_empty() { None } else { Some(Self { vec }) }
    }

    /// Removes the peeked value from the vector and returns it.

    pub fn pop(this: Self) -> T {
        // SAFETY: VVecPeekMut is only constructed if the vec is non-empty
        unsafe { this.vec.pop().unwrap_unchecked() }
    }
}

impl<'a, T, A: Allocator> Deref for VVecPeekMut<'a, T, A> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        let idx = self.vec.len() - 1;
        // SAFETY: VVecPeekMut is only constructed if the vec is non-empty
        unsafe { self.vec.get_unchecked(idx) }
    }
}

impl<'a, T, A: Allocator> DerefMut for VVecPeekMut<'a, T, A> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let idx = self.vec.len() - 1;
        // SAFETY: VVecPeekMut is only constructed if the vec is non-empty
        unsafe { self.vec.get_unchecked_mut(idx) }
    }
}
