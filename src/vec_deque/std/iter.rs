// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.
use core::iter::{FusedIterator, TrustedLen, TrustedRandomAccess, TrustedRandomAccessNoCoerce};
use core::num::NonZero;
use core::ops::Try;
use core::{fmt, mem, slice};

/// An iterator over the elements of a `VVecDeque`.
///
/// This `struct` is created by the [`iter`] method on [`super::VVecDeque`]. See its
/// documentation for more.
///
/// [`iter`]: super::VVecDeque::iter
pub struct VVecDequeIter<'a, T: 'a> {
    i1: slice::Iter<'a, T>,
    i2: slice::Iter<'a, T>,
}

impl<'a, T> VVecDequeIter<'a, T> {
    pub(super) fn new(i1: slice::Iter<'a, T>, i2: slice::Iter<'a, T>) -> Self {
        Self { i1, i2 }
    }

    /// Views the underlying data as a pair of subslices of the original data.
    ///
    /// The slices contain, in order, the contents of the deque not yet yielded
    /// by the iterator.
    ///
    /// This has the same lifetime as the original `VVecDeque`, and so the
    /// iterator can continue to be used while this exists.
    ///
    /// # Examples
    ///
    pub fn as_slices(&self) -> (&'a [T], &'a [T]) {
        (self.i1.as_slice(), self.i2.as_slice())
    }
}

impl<T: fmt::Debug> fmt::Debug for VVecDequeIter<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("VVecDequeIter").field(&self.i1.as_slice()).field(&self.i2.as_slice()).finish()
    }
}

impl<T> Default for VVecDequeIter<'_, T> {
    /// Creates an empty `vec_deque::VVecDequeIter`.
    ///
    fn default() -> Self {
        VVecDequeIter { i1: Default::default(), i2: Default::default() }
    }
}

// FIXME(#26925) Remove in favor of `#[derive(Clone)]`
impl<T> Clone for VVecDequeIter<'_, T> {
    fn clone(&self) -> Self {
        VVecDequeIter { i1: self.i1.clone(), i2: self.i2.clone() }
    }
}

impl<'a, T> Iterator for VVecDequeIter<'a, T> {
    type Item = &'a T;

    #[inline]
    fn next(&mut self) -> Option<&'a T> {
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
        let remaining = self.i1.advance_by(n);
        match remaining {
            Ok(()) => Ok(()),
            Err(n) => {
                mem::swap(&mut self.i1, &mut self.i2);
                self.i1.advance_by(n.get())
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
    fn last(mut self) -> Option<&'a T> {
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

impl<'a, T> DoubleEndedIterator for VVecDequeIter<'a, T> {
    #[inline]
    fn next_back(&mut self) -> Option<&'a T> {
        match self.i2.next_back() {
            Some(val) => Some(val),
            None => {
                // most of the time, the iterator will either always
                // call next(), or always call next_back(). By swapping
                // the iterators once the second one is empty, we ensure
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
            Err(n) => {
                mem::swap(&mut self.i1, &mut self.i2);
                self.i2.advance_back_by(n.get())
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

impl<T> ExactSizeIterator for VVecDequeIter<'_, T> {
    fn len(&self) -> usize {
        self.i1.len() + self.i2.len()
    }

    fn is_empty(&self) -> bool {
        self.i1.is_empty() && self.i2.is_empty()
    }
}

impl<T> FusedIterator for VVecDequeIter<'_, T> {}

unsafe impl<T> TrustedLen for VVecDequeIter<'_, T> {}

#[doc(hidden)]
unsafe impl<T> TrustedRandomAccess for VVecDequeIter<'_, T> {}

#[doc(hidden)]
unsafe impl<T> TrustedRandomAccessNoCoerce for VVecDequeIter<'_, T> {
    const MAY_HAVE_SIDE_EFFECT: bool = false;
}
