// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! Faithful Divan port of the upstream std-library `VecDeque` benchmark suite
//! (`rust/library/alloctests/benches/vec_deque.rs`, Rust 1.97.0), run against our two extracted
//! variants (`rustseal::vec_deque::std::VVecDeque` and
//! `rustseal::vec_deque::lazy_loss_recovery::VVecDeque`).
//!
//! One Divan bench per upstream `#[bench]` (name minus the `bench_` prefix), all 15. The REAL
//! element types are preserved: `i32` for the `(0..1000).collect()` iteration benches and the
//! `new`/`grow_1025` construction benches; `usize` for the `into_iter` family and `from_array_1000`;
//! `u8` for `extend_bytes`/`extend_vec`; `u16` for the three `extend_*trustedlen`/`extend_chained_*`
//! benches. A single `BenchDeque<T>` trait carries the workload surface; a macro implements it for
//! each (variant × element-type) pair, and per-pair `type` aliases feed the `types = [..]` lists.
//!
//! The upstream `into_iter`, `into_iter_try_fold`, and `into_iter_next_chunk` benches use an
//! `unsafe` `into_iter_helper` (`Vec::from_raw_parts` buffer reuse) *only* to keep the per-iteration
//! allocation out of libtest's timed region. Here that is replaced with Divan's native untimed
//! setup — `with_inputs(|| build_deque()).bench_values(|d| consume(d))` — which measures the
//! identical consumption (plus the `rotate_left(len/2)` variant each of those benches also runs)
//! with no `unsafe` code. `bench_into_iter_fold` already reallocated every iteration upstream, so it
//! is ported literally with `bench_local`.
//!
//! Run: `scripts/bench.sh --bench real_vec_deque` (read the `fastest` column per variant).
#![feature(iter_next_chunk)]

use rustseal::vec_deque::lazy_loss_recovery::VVecDeque as LazyDeque;
use rustseal::vec_deque::std::VVecDeque as StdDeque;

fn main() {
    divan::main();
}

/// The slice of the deque API the 15 workloads need, generic over the element type `T`, so one
/// generic bench body runs against each (variant, element-type) pair. The upstream benches sum into
/// the element type (`sum: T`), so the fold/sum methods return `T`.
trait BenchDeque<T: Copy>: FromIterator<T> + Sized {
    fn new() -> Self;
    fn with_capacity(n: usize) -> Self;
    fn from_array(array: [T; 1000]) -> Self;
    fn push_front(&mut self, x: T);
    fn clear(&mut self);
    fn rotate_left(&mut self, mid: usize);

    // --- iteration (borrowing) ---
    fn iter_sum(&self) -> T;
    fn iter_mut_sum(&mut self) -> T;
    fn iter_try_fold_sum(&self) -> Option<T>;

    // --- iteration (consuming) ---
    fn into_iter_sum(self) -> T;
    fn into_iter_fold_sum(self) -> T;
    fn into_iter_any_eq(self, target: T) -> bool;
    fn into_iter_next_chunk_buf(self) -> [T; 64];

    // --- extend (the five upstream shapes) ---
    fn extend_slice_ref(&mut self, input: &[T]);
    fn extend_owned_vec(&mut self, input: std::vec::Vec<T>);
    fn extend_range(&mut self, r: core::ops::Range<T>);
    fn extend_chained_range(&mut self, a: core::ops::Range<T>, b: core::ops::Range<T>);
    fn extend_chained_slices(&mut self, a: &[T], b: &[T]);
}

macro_rules! impl_bench_deque {
    ($ty:ident, $t:ty) => {
        impl BenchDeque<$t> for $ty<$t> {
            fn new() -> Self {
                $ty::new()
            }
            fn with_capacity(n: usize) -> Self {
                $ty::with_capacity(n)
            }
            fn from_array(array: [$t; 1000]) -> Self {
                $ty::from(array)
            }
            fn push_front(&mut self, x: $t) {
                $ty::push_front(self, x)
            }
            fn clear(&mut self) {
                $ty::clear(self)
            }
            fn rotate_left(&mut self, mid: usize) {
                $ty::rotate_left(self, mid)
            }

            fn iter_sum(&self) -> $t {
                let mut sum: $t = 0;
                for &i in self.iter() {
                    sum += i;
                }
                sum
            }
            fn iter_mut_sum(&mut self) -> $t {
                let mut sum: $t = 0;
                for i in self.iter_mut() {
                    sum += *i;
                }
                sum
            }
            fn iter_try_fold_sum(&self) -> Option<$t> {
                self.iter().try_fold(0 as $t, |a, b| Some(a + b))
            }

            fn into_iter_sum(self) -> $t {
                let mut sum: $t = 0;
                for i in self {
                    sum += i;
                }
                sum
            }
            fn into_iter_fold_sum(self) -> $t {
                self.into_iter().fold(0 as $t, |a, b| a + b)
            }
            fn into_iter_any_eq(self, target: $t) -> bool {
                self.into_iter().any(|i| i == target)
            }
            fn into_iter_next_chunk_buf(self) -> [$t; 64] {
                let mut buf = [0 as $t; 64];
                let mut it = self.into_iter();
                while let Ok(a) = it.next_chunk::<64>() {
                    buf = a;
                }
                buf
            }

            fn extend_slice_ref(&mut self, input: &[$t]) {
                $ty::extend(self, divan::black_box(input));
            }
            fn extend_owned_vec(&mut self, input: std::vec::Vec<$t>) {
                $ty::extend(self, divan::black_box(input));
            }
            fn extend_range(&mut self, r: core::ops::Range<$t>) {
                $ty::extend(self, divan::black_box(r));
            }
            fn extend_chained_range(&mut self, a: core::ops::Range<$t>, b: core::ops::Range<$t>) {
                $ty::extend(self, divan::black_box(a.chain(b)));
            }
            fn extend_chained_slices(&mut self, a: &[$t], b: &[$t]) {
                $ty::extend(self, divan::black_box(a.iter().chain(b.iter())));
            }
        }
    };
}

impl_bench_deque!(StdDeque, i32);
impl_bench_deque!(StdDeque, usize);
impl_bench_deque!(StdDeque, u8);
impl_bench_deque!(StdDeque, u16);
impl_bench_deque!(LazyDeque, i32);
impl_bench_deque!(LazyDeque, usize);
impl_bench_deque!(LazyDeque, u8);
impl_bench_deque!(LazyDeque, u16);

// Per-(variant, element-type) aliases for the `types = [..]` lists — one pair per upstream bench's
// real element type.
type StdI32 = StdDeque<i32>;
type LazyI32 = LazyDeque<i32>;
type StdUsize = StdDeque<usize>;
type LazyUsize = LazyDeque<usize>;
type StdU8 = StdDeque<u8>;
type LazyU8 = LazyDeque<u8>;
type StdU16 = StdDeque<u16>;
type LazyU16 = LazyDeque<u16>;

// ---- construction / growth (i32 / usize) -------------------------------------------------------

#[divan::bench(types = [StdI32, LazyI32])]
fn new<D: BenchDeque<i32>>(bencher: divan::Bencher) {
    bencher.bench_local(|| {
        let ring = D::new();
        divan::black_box(ring);
    });
}

#[divan::bench(types = [StdI32, LazyI32])]
fn grow_1025<D: BenchDeque<i32>>(bencher: divan::Bencher) {
    bencher.bench_local(|| {
        let mut deq = D::new();
        for i in 0..1025 {
            deq.push_front(i);
        }
        divan::black_box(deq);
    });
}

// ---- iteration (borrowing, i32) ----------------------------------------------------------------

#[divan::bench(types = [StdI32, LazyI32])]
fn iter_1000<D: BenchDeque<i32>>(bencher: divan::Bencher) {
    let ring: D = (0..1000).collect();
    bencher.bench_local(|| {
        divan::black_box(ring.iter_sum());
    });
}

#[divan::bench(types = [StdI32, LazyI32])]
fn mut_iter_1000<D: BenchDeque<i32>>(bencher: divan::Bencher) {
    let mut ring: D = (0..1000).collect();
    bencher.bench_local(|| {
        divan::black_box(ring.iter_mut_sum());
    });
}

#[divan::bench(types = [StdI32, LazyI32])]
fn try_fold<D: BenchDeque<i32>>(bencher: divan::Bencher) {
    let ring: D = (0..1000).collect();
    bencher.bench_local(|| divan::black_box(ring.iter_try_fold_sum()));
}

// ---- iteration (consuming, usize) --------------------------------------------------------------
//
// Upstream reused one allocation across iterations via unsafe `Vec::from_raw_parts`, purely to keep
// the deque build out of libtest's timing. Here `with_inputs` builds the two deques untimed and
// `bench_values` times only the consumption (plain, then rotate_left(512) + consume) — the same
// measurement, no unsafe.

const LEN: usize = 1024;

#[divan::bench(types = [StdUsize, LazyUsize])]
fn into_iter<D: BenchDeque<usize>>(bencher: divan::Bencher) {
    bencher
        .with_inputs(|| ((0..LEN).collect::<D>(), (0..LEN).collect::<D>()))
        .bench_values(|(d1, mut d2): (D, D)| {
            divan::black_box(d1.into_iter_sum());
            // rotating a full deque doesn't move any memory.
            d2.rotate_left(LEN / 2);
            divan::black_box(d2.into_iter_sum());
        });
}

#[divan::bench(types = [StdUsize, LazyUsize])]
fn into_iter_fold<D: BenchDeque<usize>>(bencher: divan::Bencher) {
    // `fold` takes ownership of the iterator, so (like upstream) we reallocate every iteration.
    bencher.bench_local(|| {
        let deque: D = (0..LEN).collect();
        divan::black_box(deque.into_iter_fold_sum());

        // rotating a full deque doesn't move any memory.
        let mut deque: D = (0..LEN).collect();
        deque.rotate_left(LEN / 2);
        divan::black_box(deque.into_iter_fold_sum());
    });
}

#[divan::bench(types = [StdUsize, LazyUsize])]
fn into_iter_try_fold<D: BenchDeque<usize>>(bencher: divan::Bencher) {
    // Iterator::any uses Iterator::try_fold under the hood.
    bencher
        .with_inputs(|| ((0..LEN).collect::<D>(), (0..LEN).collect::<D>()))
        .bench_values(|(d1, mut d2): (D, D)| {
            divan::black_box(d1.into_iter_any_eq(LEN - 1));
            d2.rotate_left(LEN / 2);
            divan::black_box(d2.into_iter_any_eq(LEN - 1));
        });
}

#[divan::bench(types = [StdUsize, LazyUsize])]
fn into_iter_next_chunk<D: BenchDeque<usize>>(bencher: divan::Bencher) {
    bencher
        .with_inputs(|| ((0..LEN).collect::<D>(), (0..LEN).collect::<D>()))
        .bench_values(|(d1, mut d2): (D, D)| {
            divan::black_box(d1.into_iter_next_chunk_buf());
            d2.rotate_left(LEN / 2);
            divan::black_box(d2.into_iter_next_chunk_buf());
        });
}

// ---- from array (usize) ------------------------------------------------------------------------

#[divan::bench(types = [StdUsize, LazyUsize])]
fn from_array_1000<D: BenchDeque<usize>>(bencher: divan::Bencher) {
    const N: usize = 1000;
    let mut array: [usize; N] = [0; N];
    for i in 0..N {
        array[i] = i;
    }
    bencher.bench_local(|| {
        let deq = D::from_array(array);
        divan::black_box(deq);
    });
}

// ---- extend (u8 / u16), persistent `ring` cleared + refilled each iteration ---------------------

#[divan::bench(types = [StdU8, LazyU8])]
fn extend_bytes<D: BenchDeque<u8>>(bencher: divan::Bencher) {
    let mut ring = D::with_capacity(1000);
    let input: &[u8] = &[128; 512];
    bencher.bench_local(|| {
        ring.clear();
        ring.extend_slice_ref(input);
    });
}

#[divan::bench(types = [StdU8, LazyU8])]
fn extend_vec<D: BenchDeque<u8>>(bencher: divan::Bencher) {
    let mut ring = D::with_capacity(1000);
    let input = vec![128u8; 512];
    bencher.bench_local(|| {
        ring.clear();
        // upstream clones inside the timed region, so the clone is timed here too.
        ring.extend_owned_vec(input.clone());
    });
}

#[divan::bench(types = [StdU16, LazyU16])]
fn extend_trustedlen<D: BenchDeque<u16>>(bencher: divan::Bencher) {
    let mut ring = D::with_capacity(1000);
    bencher.bench_local(|| {
        ring.clear();
        ring.extend_range(0..512);
    });
}

#[divan::bench(types = [StdU16, LazyU16])]
fn extend_chained_trustedlen<D: BenchDeque<u16>>(bencher: divan::Bencher) {
    let mut ring = D::with_capacity(1000);
    bencher.bench_local(|| {
        ring.clear();
        ring.extend_chained_range(0..256, 768..1024);
    });
}

#[divan::bench(types = [StdU16, LazyU16])]
fn extend_chained_bytes<D: BenchDeque<u16>>(bencher: divan::Bencher) {
    let mut ring = D::with_capacity(1000);
    let input1: &[u16] = &[128; 256];
    let input2: &[u16] = &[255; 256];
    bencher.bench_local(|| {
        ring.clear();
        ring.extend_chained_slices(input1, input2);
    });
}
