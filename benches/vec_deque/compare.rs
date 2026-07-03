// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! Statistical comparison bench, **std vs lazy_loss_recovery**, via Divan — the reliable replacement
//! for the noisy single-shot libtest suite (`benches/vec_deque/{std,lazy_loss_recovery}/vec_deque.rs`).
//!
//! Covers all 15 upstream `alloctests/benches/vec_deque.rs` workloads (the u8/u16 element-type
//! variants are represented by `u32`) plus the forget-safety ops (`drain`, `pop_front`) that are the
//! point of the `lazy_loss_recovery` comparison. Both variants run interleaved in one process.
//!
//! Run: `scripts/bench.sh --bench vec_deque_compare` (read the `fastest` column).
#![feature(iter_next_chunk)]

use rustseal::vec_deque::lazy_loss_recovery::VVecDeque as LazyDeque;
use rustseal::vec_deque::std::VVecDeque as StdDeque;

fn main() {
    divan::main();
}

/// The slice of the deque API the workloads need, so one generic body runs against each variant.
trait BenchDeque: FromIterator<u32> + Extend<u32> + Clone + Send + Sync + Sized {
    fn new() -> Self;
    fn with_capacity(n: usize) -> Self;
    fn push_front(&mut self, x: u32);
    fn pop_front(&mut self) -> Option<u32>;
    fn from_array_1000() -> Self;
    fn iter_sum(&self) -> u64;
    fn iter_mut_sum(&mut self) -> u64;
    fn iter_try_fold_sum(&self) -> u64;
    fn into_iter_fold_sum(self) -> u64;
    fn into_iter_any_last(self) -> bool;
    fn into_iter_next_chunk_sum(self) -> u64;
    fn drain_sum(&mut self) -> u64;
}

macro_rules! impl_bench_deque {
    ($ty:ident) => {
        impl BenchDeque for $ty<u32> {
            fn new() -> Self { $ty::new() }
            fn with_capacity(n: usize) -> Self { $ty::with_capacity(n) }
            fn push_front(&mut self, x: u32) { $ty::push_front(self, x) }
            fn pop_front(&mut self) -> Option<u32> { $ty::pop_front(self) }
            fn from_array_1000() -> Self {
                let mut a = [0u32; 1000];
                for i in 0..1000 {
                    a[i] = i as u32;
                }
                $ty::from(a)
            }
            fn iter_sum(&self) -> u64 { self.iter().map(|&x| x as u64).sum() }
            fn iter_mut_sum(&mut self) -> u64 { self.iter_mut().map(|x| *x as u64).sum() }
            fn iter_try_fold_sum(&self) -> u64 {
                self.iter().try_fold(0u64, |a, &b| Some(a + b as u64)).unwrap()
            }
            fn into_iter_fold_sum(self) -> u64 { self.into_iter().fold(0u64, |a, b| a + b as u64) }
            fn into_iter_any_last(self) -> bool { self.into_iter().any(|i| i == 1023) }
            fn into_iter_next_chunk_sum(self) -> u64 {
                let mut it = self.into_iter();
                let mut sum = 0u64;
                while let Ok(chunk) = it.next_chunk::<64>() {
                    for c in chunk {
                        sum += c as u64;
                    }
                }
                sum
            }
            fn drain_sum(&mut self) -> u64 { self.drain(..).map(|x| x as u64).sum() }
        }
    };
}
impl_bench_deque!(StdDeque);
impl_bench_deque!(LazyDeque);

type Std = StdDeque<u32>;
type Lazy = LazyDeque<u32>;

// ---- construction / growth ---------------------------------------------------------------------

#[divan::bench(types = [Std, Lazy])]
fn new<D: BenchDeque>() -> D {
    D::new()
}

#[divan::bench(types = [Std, Lazy])]
fn grow_1025<D: BenchDeque>(bencher: divan::Bencher) {
    bencher.bench_local(|| {
        let mut d = D::new();
        for i in 0..1025u32 {
            d.push_front(i);
        }
        d
    });
}

#[divan::bench(types = [Std, Lazy])]
fn from_array_1000<D: BenchDeque>(bencher: divan::Bencher) {
    bencher.bench_local(|| D::from_array_1000());
}

// ---- iteration ---------------------------------------------------------------------------------

#[divan::bench(types = [Std, Lazy])]
fn iter_1000<D: BenchDeque>(bencher: divan::Bencher) {
    let d: D = (0..1000u32).collect();
    bencher.with_inputs(|| d.clone()).bench_values(|d| divan::black_box(d.iter_sum()));
}

#[divan::bench(types = [Std, Lazy])]
fn mut_iter_1000<D: BenchDeque>(bencher: divan::Bencher) {
    let d: D = (0..1000u32).collect();
    bencher.with_inputs(|| d.clone()).bench_values(|mut d| divan::black_box(d.iter_mut_sum()));
}

#[divan::bench(types = [Std, Lazy])]
fn try_fold_1000<D: BenchDeque>(bencher: divan::Bencher) {
    let d: D = (0..1000u32).collect();
    bencher.with_inputs(|| d.clone()).bench_values(|d| divan::black_box(d.iter_try_fold_sum()));
}

#[divan::bench(types = [Std, Lazy])]
fn into_iter_fold_1024<D: BenchDeque>(bencher: divan::Bencher) {
    bencher.with_inputs(|| (0..1024u32).collect::<D>()).bench_values(|d| divan::black_box(d.into_iter_fold_sum()));
}

#[divan::bench(types = [Std, Lazy])]
fn into_iter_try_fold_1024<D: BenchDeque>(bencher: divan::Bencher) {
    bencher.with_inputs(|| (0..1024u32).collect::<D>()).bench_values(|d| divan::black_box(d.into_iter_any_last()));
}

#[divan::bench(types = [Std, Lazy])]
fn into_iter_next_chunk_1024<D: BenchDeque>(bencher: divan::Bencher) {
    bencher.with_inputs(|| (0..1024u32).collect::<D>()).bench_values(|d| divan::black_box(d.into_iter_next_chunk_sum()));
}

// ---- extend (the five upstream shapes, u32) ----------------------------------------------------

#[divan::bench(types = [Std, Lazy])]
fn extend_bytes<D: BenchDeque>(bencher: divan::Bencher) {
    let input: &[u32] = &[128; 512];
    bencher.with_inputs(|| D::with_capacity(1000)).bench_values(|mut d| {
        d.extend(divan::black_box(input.iter().copied()));
        d
    });
}

#[divan::bench(types = [Std, Lazy])]
fn extend_vec<D: BenchDeque>(bencher: divan::Bencher) {
    let input = vec![128u32; 512];
    bencher.with_inputs(|| (D::with_capacity(1000), input.clone())).bench_values(|(mut d, v)| {
        d.extend(divan::black_box(v));
        d
    });
}

#[divan::bench(types = [Std, Lazy])]
fn extend_trustedlen<D: BenchDeque>(bencher: divan::Bencher) {
    bencher.with_inputs(|| D::with_capacity(1000)).bench_values(|mut d| {
        d.extend(divan::black_box(0..512u32));
        d
    });
}

#[divan::bench(types = [Std, Lazy])]
fn extend_chained_trustedlen<D: BenchDeque>(bencher: divan::Bencher) {
    bencher.with_inputs(|| D::with_capacity(1000)).bench_values(|mut d| {
        d.extend(divan::black_box((0..256u32).chain(768..1024)));
        d
    });
}

#[divan::bench(types = [Std, Lazy])]
fn extend_chained_bytes<D: BenchDeque>(bencher: divan::Bencher) {
    let input1: &[u32] = &[128; 256];
    let input2: &[u32] = &[255; 256];
    bencher.with_inputs(|| D::with_capacity(1000)).bench_values(|mut d| {
        d.extend(divan::black_box(input1.iter().chain(input2.iter()).copied()));
        d
    });
}

// ---- forget-safety ops (the point of the comparison) -------------------------------------------

#[divan::bench(types = [Std, Lazy])]
fn pop_front_50k<D: BenchDeque>(bencher: divan::Bencher) {
    bencher.with_inputs(|| (0..50_000u32).collect::<D>()).bench_values(|mut d| {
        while let Some(e) = d.pop_front() {
            divan::black_box(e);
        }
    });
}

#[divan::bench(types = [Std, Lazy])]
fn drain_sum_50k<D: BenchDeque>(bencher: divan::Bencher) {
    let d: D = (0..50_000u32).collect();
    bencher.with_inputs(|| d.clone()).bench_values(|mut d| divan::black_box(d.drain_sum()));
}
