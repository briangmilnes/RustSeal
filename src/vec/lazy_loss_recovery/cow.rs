// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! CORPSE MODULE (ProcessCommentingStandard).
//!
//! Every impl in `alloc`'s `vec/cow.rs` converts between `Vec`/slices/arrays and
//! `Cow<'a, [T]>`. In `alloc`, both `Vec` and `Cow` are local types, so these impls are
//! legal. In this extraction `std::borrow::Cow` is a FOREIGN type and `From`/`FromIterator`
//! are FOREIGN traits, so `impl From<..> for Cow<..>` / `impl FromIterator<..> for Cow<..>`
//! all violate the orphan rule (E0117/E0210) — and `Cow::Owned` would additionally require
//! `<[T] as ToOwned>::Owned == VVec<T>` (E0271), which is false (it is `std::vec::Vec<T>`).
//! The whole module is therefore a corpse; the `mod cow;` declaration in `vec.rs` is
//! commented out. The bodies are preserved below for reference.

// impl<'a, T: Clone> From<&'a [T]> for Cow<'a, [T]> {
//     fn from(s: &'a [T]) -> Cow<'a, [T]> {
//         Cow::Borrowed(s)
//     }
// }
//
// impl<'a, T: Clone, const N: usize> From<&'a [T; N]> for Cow<'a, [T]> {
//     fn from(s: &'a [T; N]) -> Cow<'a, [T]> {
//         Cow::Borrowed(s as &[_])
//     }
// }
//
// impl<'a, T: Clone> From<VVec<T>> for Cow<'a, [T]> {
//     fn from(v: VVec<T>) -> Cow<'a, [T]> {
//         Cow::Owned(v)
//     }
// }
//
// impl<'a, T: Clone> From<&'a VVec<T>> for Cow<'a, [T]> {
//     fn from(v: &'a VVec<T>) -> Cow<'a, [T]> {
//         Cow::Borrowed(v.as_slice())
//     }
// }
//
// impl<'a, T> FromIterator<T> for Cow<'a, [T]>
// where
//     T: Clone,
// {
//     fn from_iter<I: IntoIterator<Item = T>>(it: I) -> Cow<'a, [T]> {
//         Cow::Owned(FromIterator::from_iter(it))
//     }
// }
