// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! `vec` — variant-comparison harness for `VVec` (the `alloc::vec::Vec` extraction), mirroring the
//! `vec_deque` and `binary_heap` N-way layout. Each variant is a full multi-file extraction in its
//! own directory under `src/vec/`; this dispatcher exposes them side by side so tests and benches
//! can compare them.
//!
//! The variants:
//!   * `std`                — the faithful extraction of `alloc::vec` (rust-libs 1.96.0), V-renamed
//!     (`Vec` -> `VVec`, `IntoIter` -> `VVecIntoIter`, `Drain` -> `VVecDrain`, `Splice` ->
//!     `VVecSplice`, `ExtractIf` -> `VVecExtractIf`), backed by `VRawVec` (a shim over the public
//!     `Vec<T, A>`). The baseline.
//!   * `lazy_loss_recovery` — `std` plus forget-safety: `drain`/`extract_if`/`splice` do not lose the
//!     vec's own elements on `mem::forget`; the deque's `pending`-note + lazy `restore_wf_wo_data_loss`
//!     mechanism ported over.

#[path = "std/vec.rs"] pub mod std;
#[path = "lazy_loss_recovery/vec.rs"] pub mod lazy_loss_recovery;
