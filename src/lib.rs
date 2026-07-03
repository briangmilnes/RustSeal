// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! rustseal — the collections algorithms of `rust-libs` (alloc::collections),
//! extracted into a standalone crate as a step toward verus verification.
//!
//! Pure Rust to start (no verus yet). Each algorithm is brought in FAITHFULLY from
//! rust-libs — the full public surface (including the allocator generic) — and every
//! extracted type/trait/free-fn is renamed with a `V` prefix so it does not collide
//! with the std type it was copied from.
//!
//! Several library features the source relies on are unstable in Rust; the build
//! unlocks them with `RUSTC_BOOTSTRAP=1` (the same mechanism
//! `build-verified-algorithms-core` uses), so this stays "pure Rust" (no verus) while
//! keeping the extraction faithful:
//!   * `allocator_api`        — the `A: Allocator = Global` generic on `Vec`/heap.
//!   * `trusted_len`          — `TrustedLen` impls on `VIntoIterSorted` / `VDrainSorted`.
//!   * `exact_size_is_empty`  — the `ExactSizeIterator::is_empty` overrides.
//!   * `extend_one`           — the `Extend::extend_one` / `extend_reserve` overrides.
//!
//! Source: products/RRMs/rust-libs-1.96.0/.rust-src-library/alloc/src/collections/.
#![feature(allocator_api)]
#![feature(trusted_len)]
#![feature(exact_size_is_empty)]
#![feature(extend_one)]
#![feature(slice_swap_unchecked)] // safe_but_for_index_binary_heap's `swap_unchecked`
// vec_deque extraction: features the faithful VecDeque source relies on.
#![feature(min_specialization)] // SpecExtend / SpecFromIter / SpecExtendFromWithin
#![feature(never_type)] // `Ok::<B, !>` in VVecDequeIntoIter fold/rfold
#![feature(std_internals)] // core::iter::ByRefSized in the TrustedLen extend path
#![feature(try_reserve_kind)] // TryReserveErrorKind::CapacityOverflow
#![feature(hasher_prefixfree_extras)] // Hasher::write_length_prefix in Hash impl
// `R: Try` bound in the try_fold/try_rfold overrides. The stable build needs this gate,
// but the verus toolchain pre-enables `try_trait_v2`, so enabling it again under `verus`
// is a hard error ("feature already enabled") — gate it to non-verus builds only.
#![cfg_attr(not(feature = "verus"), feature(try_trait_v2))]
#![feature(trivial_clone)] // TrivialClone fast path of SpecExtendFromWithin
#![feature(slice_range)] // slice::range in slice_ranges
#![feature(sized_type_properties)] // T::IS_ZST (SizedTypeProperties)
#![feature(extend_one_unchecked)] // Extend::extend_one_unchecked override
#![feature(iter_advance_by)] // advance_by / advance_back_by overrides
#![feature(slice_iter_mut_as_mut_slice)] // slice::IterMut::as_mut_slice in IterMut
#![feature(array_into_iter_constructors)] // array::IntoIter::new_unchecked in next_chunk
#![feature(maybe_uninit_uninit_array_transpose)] // MaybeUninit array transpose in next_chunk
#![feature(iter_next_chunk)] // Iterator::next_chunk override
#![feature(rev_into_inner)] // Rev::into_inner in SpecExtendFront
#![feature(copied_into_inner)] // Copied::into_inner in SpecExtendFront
#![feature(dropck_eyepatch)] // #[may_dangle] on the Drop impl
#![feature(vec_deque_iter_as_slices)] // Iter/IterMut as_slices surface
#![feature(vec_deque_extract_if)] // VVecDequeExtractIf
#![feature(deque_extend_front)] // VVecDequeSplice / extend_front
#![feature(trusted_random_access)] // Iter/IterMut TrustedRandomAccess impls
// vec extraction (alloc::vec / `Vec`): features the faithful Vec source relies on.
#![feature(core_intrinsics)] // core::intrinsics in low-level Vec methods
#![feature(ub_checks)] // core::ub_checks debug assertions
#![feature(freeze)] // core::marker::Freeze bound on some Vec impls
#![feature(transmutability)] // core::mem::{Assume, TransmuteFrom} in the copy-extend fast path
#![feature(trusted_fused)] // TrustedFused impl on VVecIntoIter
#![feature(likely_unlikely)] // core::hint::{likely, unlikely}
#![feature(deref_pure_trait)] // DerefPure impl
#![feature(cast_maybe_uninit)] // MaybeUninit cast in next_chunk
#![feature(panic_internals)] // core::panicking internals in the capacity-overflow path
#![feature(fmt_arguments_from_str)] // fmt::Arguments::from_str
#![feature(decl_macro)] // `macro non_null { .. }` decl-macro 2.0 in into_iter
#![feature(stmt_expr_attributes)] // `#![allow(unused_unsafe)]` inside the non_null macro body
#![feature(slice_ptr_get)] // <*mut [T]>::as_mut_ptr / get_unchecked_mut on raw slices
#![feature(set_ptr_value)] // ptr::with_metadata_of in split_off
#![feature(ptr_alignment_type)] // alignment helpers
#![feature(box_vec_non_null)] // Box<[T]> <-> NonNull bridges
// `std_internals` (ByRefSized) is flagged by the `internal_features` lint (warn-by-default).
// This is a FAITHFUL extraction of alloc, which legitimately uses that std-internal item;
// allow the lint here rather than diverge from the source (rule 5.8 justified allow).
#![allow(internal_features)]

// Under verus (the `verus` feature, set by scripts/validate.sh), bring vstd into scope so
// verus_builtin is linked (without it, verify=true fails V078). The stable build/test/
// bench never enable this feature, so they never see vstd. Both lines are no-ops outside
// verus.
#[cfg(feature = "verus")]
#[allow(unused_imports)]
use vstd::prelude::*;

// The seven binary-heap variants live under `src/binary_heap/` (one file per variant). See
// `src/binary_heap.rs` for the module list and `src/binary_heap/README.md` for the design /
// performance comparison and the winning variant. No `verus!{}` specs yet, so the whole
// `binary_heap` subtree is `#[verifier::external]` under verus only (the attr is gated by
// `cfg_attr(feature = "verus")` so stable rustc never sees the `verifier` tool); spec work
// later moves items out of `external` into `verus!{}`.
#[cfg_attr(feature = "verus", verifier::external)]
#[path = "binary_heap/binary_heap.rs"]
pub mod binary_heap;

// `vec_deque` — variant-comparison harness for the growable-ring-buffer deque (binary_heap
// N-way layout). `vec_deque/vec_deque.rs` is the dispatcher; each variant is a full multi-file
// extraction in its own directory: `std` is the faithful extraction of
// `alloc::collections::vec_deque` (rust-libs 1.96.0), V-renamed (`VecDeque` -> `VVecDeque`,
// `Iter` -> `VVecDequeIter`, `Drain` -> `VVecDequeDrain`, etc.); `lazy_loss_recovery` is the alternative
// slot (currently a copy of `std`). The storage backing `RawVec` is alloc-private and its 1.96
// source uses const-trait syntax the installed rustc cannot compile, so the deque is backed by
// `VRawVec`, a shim over the public `Vec<T, A>` with identical amortized growth (see each
// variant's raw_vec_shim.rs). No `verus!{}` specs yet, so the subtree is
// `#[verifier::external]` under verus only.
#[cfg_attr(feature = "verus", verifier::external)]
#[path = "vec_deque/vec_deque.rs"]
pub mod vec_deque;

// `vec` — variant-comparison harness for `VVec` (the `alloc::vec` / `Vec<T, A>` extraction from
// rust-libs 1.96.0, V-renamed: `Vec` -> `VVec`, `IntoIter` -> `VVecIntoIter`, `Drain` -> `VVecDrain`,
// `Splice` -> `VVecSplice`, `ExtractIf` -> `VVecExtractIf`, `PeekMut` -> `VVecPeekMut`,
// `SetLenOnDrop` -> `VVecSetLenOnDrop`). `vec/vec.rs` is the dispatcher; each variant is a full
// multi-file extraction in its own directory: `std` is the faithful baseline; `lazy_loss_recovery`
// adds forget-safety to `drain`/`extract_if`/`splice` (the vec_deque `pending`-note mechanism).
// Storage backing `RawVec` is alloc-private and its 1.96 source uses const-trait syntax the installed
// rustc cannot compile, so `VVec` is backed by `VRawVec`, a shim over the public `Vec<T, A>` (see each
// variant's raw_vec_shim.rs). The in-place-collect / specialization machinery (`in_place_collect`,
// `in_place_drop`, `spec_from_iter`, `is_zero`) is perf-only and cannot be expressed over the shim, so
// it is commented out as corpses; `FromIterator`/`from_elem` use the naive fallbacks. No `verus!{}`
// specs yet, so the subtree is `#[verifier::external]` under verus only.
#[cfg_attr(feature = "verus", verifier::external)]
#[path = "vec/vec.rs"]
pub mod vec;
