// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! `vec_deque` — variant comparison harness for the growable-ring-buffer deque, mirroring
//! the `binary_heap` N-way layout. Each variant is a full multi-file extraction living in
//! its own directory under `src/vec_deque/`; this file is the dispatcher that exposes them
//! side by side so tests and benches can compare them.
//!
//! The variants:
//!   * `std`      — the faithful extraction of `alloc::collections::vec_deque` (rust-libs
//!     1.96.0), V-renamed (`VecDeque` -> `VVecDeque`, `Iter` -> `VVecDequeIter`, `Drain` ->
//!     `VVecDequeDrain`, etc.), backed by `VRawVec` (a shim over the public `Vec<T, A>`
//!     because alloc's `RawVec` is module-private and its 1.96 source uses const-trait
//!     syntax the installed rustc cannot compile). This is the baseline.
//!   * `lazy_loss_recovery` — currently an exact copy of `std`; the slot for an alternative
//!     implementation to be measured against the baseline.

#[path = "std/vec_deque.rs"] pub mod std;
#[path = "lazy_loss_recovery/vec_deque.rs"] pub mod lazy_loss_recovery;
