// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! verus-lab — RustSeal's verus scratch lab.
//!
//! `src/experiments/*.rs` are single-file `verus!` sources verified directly by
//! the verus binary via `scripts/validate.sh` (no cargo build). This `lib.rs` is
//! intentionally empty of code and does NOT `mod`-declare the experiments, so
//! `cargo build` never tries to compile the `verus!` macro sources with rustc
//! (they need the verus toolchain). It exists only so the directory is a valid,
//! detached cargo crate. See `README.md` for the experiment index.
