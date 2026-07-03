// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.
use core::iter::{FusedIterator, TrustedLen, TrustedRandomAccess, TrustedRandomAccessNoCoerce};
use core::num::NonZero;
use core::ops::Try;
use core::{fmt, mem, slice};

/// A mutable iterator over the elements of a `VVecDeque`.
///
/// This `struct` is created by the [`iter_mut`] method on [`super::VVecDeque`]. See its
/// documentation for more.
///
/// [`iter_mut`]: super::VVecDeque::iter_mut
pub struct VVecDequeIterMut<'a, T: 'a> {
    i1: slice::IterMut<'a, T>,
    i2: slice::IterMut<'a, T>,
}

impl<'a, T> VVecDequeIterMut<'a, T> {
    pub(super) fn new(i1: slice::IterMut<'a, T>, i2: slice::IterMut<'a, T>) -> Self {
        Self { i1, i2 }
    }

    /// Views the underlying data as a pair of subslices of the original data.
    ///
    /// The slices contain, in order, the contents of the deque not yet yielded
    /// by the iterator.
    ///
    /// To avoid creating `&mut` references that alias, this is forced to
    /// consume the iterator.
    ///
    /// # Examples
    ///
    pub fn into_slices(self) -> (&'a mut [T], &'a mut [T]) {
        (self.i1.into_slice(), self.i2.into_slice())
    }

    /// Views the underlying data as a pair of subslices of the original data.
    ///
    /// The slices contain, in order, the contents of the deque not yet yielded
    /// by the iterator.
    ///
    /// To avoid creating `&mut [T]` references that alias, the returned slices
    /// borrow their lifetimes from the iterator the method is applied on.
    ///
    /// # Examples
    ///
    pub fn as_slices(&self) -> (&[T], &[T]) {
        (self.i1.as_slice(), self.i2.as_slice())
    }

    /// Views the underlying data as a pair of subslices of the original data.
    ///
    /// The slices contain, in order, the contents of the deque not yet yielded
    /// by the iterator.
    ///
    /// To avoid creating `&mut [T]` references that alias, the returned slices
    /// borrow their lifetimes from the iterator the method is applied on.
    ///
    /// # Examples
    ///
    pub fn as_mut_slices(&mut self) -> (&mut [T], &mut [T]) {
        (self.i1.as_mut_slice(), self.i2.as_mut_slice())
    }
}

impl<T: fmt::Debug> fmt::Debug for VVecDequeIterMut<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("VVecDequeIterMut").field(&self.i1.as_slice()).field(&self.i2.as_slice()).finish()
    }
}

impl<T> Default for VVecDequeIterMut<'_, T> {
    /// Creates an empty `vec_deque::VVecDequeIterMut`.
    ///
    fn default() -> Self {
        VVecDequeIterMut { i1: Default::default(), i2: Default::default() }
    }
}

impl<'a, T> Iterator for VVecDequeIterMut<'a, T> {
    type Item = &'a mut T;

    #[inline]
    fn next(&mut self) -> Option<&'a mut T> {
        match self.i1.next() {
            Some(val) => Some(val),
            None => {
                // most of the time, the iterator will either always
                // call next(), or always call next_back(). By swapping
                // the iterators once the first one is empty, we ensure
                // that the first branch is taken as often as possible,
                // without sacrificing correctness, as i1 is empty anyways
                mem::swap(&mut self.i1, &mut self.i2);
                self.i1.next()
            }
        }
    }

    fn advance_by(&mut self, n: usize) -> Result<(), NonZero<usize>> {
        match self.i1.advance_by(n) {
            Ok(()) => Ok(()),
            Err(remaining) => {
                mem::swap(&mut self.i1, &mut self.i2);
                self.i1.advance_by(remaining.get())
            }
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }

    fn fold<Acc, F>(self, accum: Acc, mut f: F) -> Acc
    where
        F: FnMut(Acc, Self::Item) -> Acc,
    {
        let accum = self.i1.fold(accum, &mut f);
        self.i2.fold(accum, &mut f)
    }

    fn try_fold<B, F, R>(&mut self, init: B, mut f: F) -> R
    where
        F: FnMut(B, Self::Item) -> R,
        R: Try<Output = B>,
    {
        let acc = self.i1.try_fold(init, &mut f)?;
        self.i2.try_fold(acc, &mut f)
    }

    #[inline]
    fn last(mut self) -> Option<&'a mut T> {
        self.next_back()
    }

    #[inline]
    unsafe fn __iterator_get_unchecked(&mut self, idx: usize) -> Self::Item {
        // Safety: The TrustedRandomAccess contract requires that callers only pass an index
        // that is in bounds.
        unsafe {
            let i1_len = self.i1.len();
            if idx < i1_len {
                self.i1.__iterator_get_unchecked(idx)
            } else {
                self.i2.__iterator_get_unchecked(idx - i1_len)
            }
        }
    }
}

impl<'a, T> DoubleEndedIterator for VVecDequeIterMut<'a, T> {
    #[inline]
    fn next_back(&mut self) -> Option<&'a mut T> {
        match self.i2.next_back() {
            Some(val) => Some(val),
            None => {
                // most of the time, the iterator will either always
                // call next(), or always call next_back(). By swapping
                // the iterators once the first one is empty, we ensure
                // that the first branch is taken as often as possible,
                // without sacrificing correctness, as i2 is empty anyways
                mem::swap(&mut self.i1, &mut self.i2);
                self.i2.next_back()
            }
        }
    }

    fn advance_back_by(&mut self, n: usize) -> Result<(), NonZero<usize>> {
        match self.i2.advance_back_by(n) {
            Ok(()) => Ok(()),
            Err(remaining) => {
                mem::swap(&mut self.i1, &mut self.i2);
                self.i2.advance_back_by(remaining.get())
            }
        }
    }

    fn rfold<Acc, F>(self, accum: Acc, mut f: F) -> Acc
    where
        F: FnMut(Acc, Self::Item) -> Acc,
    {
        let accum = self.i2.rfold(accum, &mut f);
        self.i1.rfold(accum, &mut f)
    }

    fn try_rfold<B, F, R>(&mut self, init: B, mut f: F) -> R
    where
        F: FnMut(B, Self::Item) -> R,
        R: Try<Output = B>,
    {
        let acc = self.i2.try_rfold(init, &mut f)?;
        self.i1.try_rfold(acc, &mut f)
    }
}

impl<T> ExactSizeIterator for VVecDequeIterMut<'_, T> {
    fn len(&self) -> usize {
        self.i1.len() + self.i2.len()
    }

    fn is_empty(&self) -> bool {
        self.i1.is_empty() && self.i2.is_empty()
    }
}

impl<T> FusedIterator for VVecDequeIterMut<'_, T> {}

unsafe impl<T> TrustedLen for VVecDequeIterMut<'_, T> {}

#[doc(hidden)]
unsafe impl<T> TrustedRandomAccess for VVecDequeIterMut<'_, T> {}

#[doc(hidden)]
unsafe impl<T> TrustedRandomAccessNoCoerce for VVecDequeIterMut<'_, T> {
    const MAY_HAVE_SIDE_EFFECT: bool = false;
}
