// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! `binary_heap` — seven re-implementations of `alloc::collections::binary_heap` extracted
//! from rust-libs 1.96.0, used to study the cost of safety in the heap algorithm and the
//! `peek_mut` forget-safety design space. Each variant is one file under `src/binary_heap/`.
//!
//! See `src/binary_heap/README.md` for the full design + performance comparison and the
//! winning variant (`unsafe_lazy_hole_binary_heap`), and
//! `docs/leak-amplification-and-forget-safety.md` for the Rust-semantics reasoning.
//!
//! The variants, by `peek_mut` mechanism and sift body:
//!   * `unsafe_binary_heap`            — faithful std: `set_len` leak amplification, `Hole` sift.
//!   * `safe_binary_heap`              — zero unsafe blocks: tail-split `peek_mut`, swap sift.
//!   * `safe_opt_binary_heap`          — `&mut data[0]` + sift-on-drop (drops forget guarantee).
//!   * `safe_but_for_index_binary_heap`— `safe_opt` + unchecked indexing (isolates bounds-check cost).
//!   * `unsafe_nopanic_binary_heap`    — + bare hole sift, no panic guard (isolates swap-vs-hole).
//!   * `unsafe_lazy_binary_heap`       — lazy-reconcile `peek_mut` (forget-safe, no leak), swap sift.
//!   * `unsafe_lazy_hole_binary_heap`  — lazy-reconcile `peek_mut` on the panic-safe `Hole` sift.
//!   * `unsafe_lazy_hole_resort_binary_heap` — `unsafe_lazy_hole` + ORDER recovery after a comparison
//!     panic: a bit-packed `possibly_mal_formed` byte tracks well-formedness, and a comparison panic
//!     mid-sift (recorded by the sift's panic protection) triggers a full *O*(n) resort on the next op.

#[path = "unsafe_binary_heap.rs"] pub mod unsafe_binary_heap;
#[path = "losers/safe_binary_heap.rs"] pub mod safe_binary_heap;
#[path = "losers/safe_opt_binary_heap.rs"] pub mod safe_opt_binary_heap;
#[path = "losers/safe_but_for_index_binary_heap.rs"] pub mod safe_but_for_index_binary_heap;
#[path = "losers/unsafe_nopanic_binary_heap.rs"] pub mod unsafe_nopanic_binary_heap;
#[path = "losers/unsafe_lazy_binary_heap.rs"] pub mod unsafe_lazy_binary_heap;
#[path = "losers/unsafe_lazy_hole_binary_heap.rs"] pub mod unsafe_lazy_hole_binary_heap;
#[path = "unsafe_lazy_hole_resort_binary_heap.rs"] pub mod unsafe_lazy_hole_resort_binary_heap;
