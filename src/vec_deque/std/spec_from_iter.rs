// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.
use super::{VVecDequeIntoIter, VVecDeque};

/// Specialization trait used for `VVecDeque::from_iter`
pub(super) trait SpecFromIter<T, I> {
    fn spec_from_iter(iter: I) -> Self;
}

impl<T, I> SpecFromIter<T, I> for VVecDeque<T>
where
    I: Iterator<Item = T>,
{
    default fn spec_from_iter(iterator: I) -> Self {
        // Since converting is O(1) now, just re-use the `Vec` logic for
        // anything where we can't do something extra-special for `VVecDeque`,
        // especially as that could save us some monomorphization work
        // if one uses the same iterators (like slice ones) with both.
        Vec::from_iter(iterator).into()
    }
}

// CORPSE (ProcessCommentingStandard): the `vec::IntoIter` specialization of SpecFromIter
// calls the alloc-private `vec::IntoIter::into_vecdeque`, not nameable outside `alloc`.
// `from_iter` over a `Vec`'s `IntoIter` still works via the generic default impl above
// (build a `Vec` then `.into()` a `VVecDeque`); only the zero-copy reuse is dropped.
// impl<T> SpecFromIter<T, std::vec::IntoIter<T>> for VVecDeque<T> {
//     #[inline]
//     fn spec_from_iter(iterator: std::vec::IntoIter<T>) -> Self {
//         iterator.into_vecdeque()
//     }
// }

impl<T> SpecFromIter<T, VVecDequeIntoIter<T>> for VVecDeque<T> {
    #[inline]
    fn spec_from_iter(iterator: VVecDequeIntoIter<T>) -> Self {
        iterator.into_vecdeque()
    }
}
