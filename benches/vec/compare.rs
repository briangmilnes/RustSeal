// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! Statistical comparison bench, **std vs lazy_loss_recovery**, via Divan — the reliable replacement
//! for the noisy single-shot libtest suite (`benches/vec/{std,lazy_loss_recovery}/vec.rs`).
//!
//! Covers every workload FAMILY of the 101 upstream `alloctests/benches/vec.rs` benches, each run
//! against both variants interleaved in one process, parameterized over a `BenchVec` trait and (for
//! the size families) Divan `args`. The type-variant upstream benches (`in_place` u8/u32/u128) are
//! represented by their `u32` form; the rest map one-to-one. `lazy_loss_recovery` adds forget-safety
//! (a boxed `pending` note + a guard at mutating entry points); these workloads exercise where that
//! could cost — construct/clone (the boxed field), `extend`/bulk-copy, and `drain`.
//!
//! Run: `scripts/bench.sh --bench vec_compare` (best on a quiescent machine; read the `fastest`
//! column — noise only adds time).

use rand::RngCore;

use rustseal::vec::lazy_loss_recovery::VVec as LazyVec;
use rustseal::vec::std::VVec as StdVec;

fn main() {
    divan::main();
}

fn bench_rng() -> rand_xorshift::XorShiftRng {
    const SEED: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    rand::SeedableRng::from_seed(SEED)
}

/// A sorted, deduplicable random `u32` buffer (matches the upstream dedup fill).
fn random_sorted(sz: usize) -> Vec<u32> {
    let mut seed = 0x43u32;
    let mask = if sz < 8192 { 0xFF } else if sz < 200_000 { 0xFFFF } else { 0xFFFF_FFFF };
    let mut buf = vec![0u32; sz];
    for item in buf.iter_mut() {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        *item = seed & mask;
    }
    buf.sort();
    buf
}

/// The slice of the vec API the workloads need, so one generic bench body runs against each variant.
trait BenchVec: FromIterator<u32> + Extend<u32> + Clone + Send + Sync + Sized {
    fn new() -> Self;
    fn with_capacity(n: usize) -> Self;
    fn from_slice(s: &[u32]) -> Self;
    fn push(&mut self, x: u32);
    fn pop(&mut self) -> Option<u32>;
    fn extend_from_slice(&mut self, s: &[u32]);
    fn dedup(&mut self);
    fn retain_even(&mut self);
    fn first_is_some(&self) -> bool;
    fn iter_sum(&self) -> u64;
    fn into_iter_sum(self) -> u64;
    fn drain_sum(&mut self) -> u64;
    /// `into_iter().enumerate().map(|(i,e)| i as u32 ^ e).collect()` — the in-place-collect workload.
    fn in_place_xor(self) -> Self;
    fn clone_from_src(&mut self, src: &Self);
}

macro_rules! impl_bench_vec {
    ($ty:ident) => {
        impl BenchVec for $ty<u32> {
            fn new() -> Self { $ty::new() }
            fn with_capacity(n: usize) -> Self { $ty::with_capacity(n) }
            fn from_slice(s: &[u32]) -> Self { $ty::from(s) }
            fn push(&mut self, x: u32) { $ty::push(self, x) }
            fn pop(&mut self) -> Option<u32> { $ty::pop(self) }
            fn extend_from_slice(&mut self, s: &[u32]) { $ty::extend_from_slice(self, s) }
            fn dedup(&mut self) { $ty::dedup(self) }
            fn retain_even(&mut self) { $ty::retain(self, |x| x & 1 == 0) }
            fn first_is_some(&self) -> bool { self.first().is_some() }
            fn iter_sum(&self) -> u64 { self.iter().map(|&x| x as u64).sum() }
            fn into_iter_sum(self) -> u64 { self.into_iter().map(|x| x as u64).sum() }
            fn drain_sum(&mut self) -> u64 { self.drain(..).map(|x| x as u64).sum() }
            fn in_place_xor(self) -> Self {
                self.into_iter().enumerate().map(|(i, e)| i as u32 ^ e).collect()
            }
            fn clone_from_src(&mut self, src: &Self) { self.clone_from(src) }
        }
    };
}
impl_bench_vec!(StdVec);
impl_bench_vec!(LazyVec);

type Std = StdVec<u32>;
type Lazy = LazyVec<u32>;

const SIZES: &[usize] = &[0, 10, 100, 1000];
const DEDUP_SIZES: &[usize] = &[100, 1000, 10000, 100_000];
const LEN: usize = 16384;

// ---- construction ------------------------------------------------------------------------------

#[divan::bench(types = [Std, Lazy], args = SIZES)]
fn with_capacity<V: BenchVec>(n: usize) -> V {
    V::with_capacity(divan::black_box(n))
}

#[divan::bench(types = [Std, Lazy], args = SIZES)]
fn collect_range<V: BenchVec>(bencher: divan::Bencher, n: usize) {
    bencher.bench_local(|| (0..divan::black_box(n) as u32).collect::<V>());
}

#[divan::bench(types = [Std, Lazy], args = SIZES)]
fn from_elem<V: BenchVec>(bencher: divan::Bencher, n: usize) {
    bencher.bench_local(|| core::iter::repeat(5u32).take(divan::black_box(n)).collect::<V>());
}

#[divan::bench(types = [Std, Lazy], args = SIZES)]
fn from_slice<V: BenchVec>(bencher: divan::Bencher, n: usize) {
    let src: Vec<u32> = (0..n as u32).collect();
    bencher.bench_local(|| V::from_slice(divan::black_box(&src)));
}

#[divan::bench(types = [Std, Lazy], args = SIZES)]
fn from_iter<V: BenchVec>(bencher: divan::Bencher, n: usize) {
    let src: V = (0..n as u32).collect();
    bencher.bench_local(|| src.clone());
}

// ---- extend ------------------------------------------------------------------------------------

#[divan::bench(types = [Std, Lazy], args = SIZES)]
fn extend_from0<V: BenchVec>(bencher: divan::Bencher, n: usize) {
    let src: Vec<u32> = (0..n as u32).collect();
    bencher.bench_local(|| {
        let mut d = V::new();
        d.extend(divan::black_box(src.iter().copied()));
        d
    });
}

#[divan::bench(types = [Std, Lazy], args = SIZES)]
fn extend_sym<V: BenchVec>(bencher: divan::Bencher, n: usize) {
    let src: Vec<u32> = (0..n as u32).collect();
    bencher.with_inputs(|| (0..n as u32).collect::<V>()).bench_values(|mut d| {
        d.extend(src.iter().copied());
        d
    });
}

#[divan::bench(types = [Std, Lazy], args = SIZES)]
fn extend_from_slice_sym<V: BenchVec>(bencher: divan::Bencher, n: usize) {
    let src: Vec<u32> = (0..n as u32).collect();
    bencher.with_inputs(|| (0..n as u32).collect::<V>()).bench_values(|mut d| {
        d.extend_from_slice(&src);
        d
    });
}

// ---- clone -------------------------------------------------------------------------------------

#[divan::bench(types = [Std, Lazy], args = [0, 10, 100, 1000, 50_000])]
fn clone<V: BenchVec>(bencher: divan::Bencher, n: usize) {
    let src: V = (0..n as u32).collect();
    bencher.bench_local(|| divan::black_box(src.clone()));
}

#[divan::bench(types = [Std, Lazy], args = [0, 10, 100, 1000, 50_000])]
fn clone_from<V: BenchVec>(bencher: divan::Bencher, n: usize) {
    let src: V = (0..n as u32).collect();
    bencher.with_inputs(|| (0..n as u32).collect::<V>()).bench_values(|mut d| {
        d.clone_from_src(&src);
        d
    });
}

// ---- in-place collect --------------------------------------------------------------------------

#[divan::bench(types = [Std, Lazy], args = [10, 100, 1000])]
fn in_place_xor<V: BenchVec>(bencher: divan::Bencher, n: usize) {
    bencher.with_inputs(|| (0..n as u32).collect::<V>()).bench_values(|v| v.in_place_xor());
}

// ---- dedup / retain ----------------------------------------------------------------------------

#[divan::bench(types = [Std, Lazy], args = DEDUP_SIZES)]
fn dedup_random<V: BenchVec>(bencher: divan::Bencher, sz: usize) {
    let template = random_sorted(sz);
    bencher.with_inputs(|| V::from_slice(&template)).bench_values(|mut v| {
        v.dedup();
        divan::black_box(v.first_is_some());
        v
    });
}

#[divan::bench(types = [Std, Lazy], args = DEDUP_SIZES)]
fn dedup_none<V: BenchVec>(bencher: divan::Bencher, sz: usize) {
    let mut template = vec![0u32; sz];
    template.chunks_exact_mut(2).for_each(|w| {
        w[0] = 0;
        w[1] = 5;
    });
    bencher.with_inputs(|| V::from_slice(&template)).bench_values(|mut v| {
        v.dedup();
        divan::black_box(v.first_is_some());
        v
    });
}

#[divan::bench(types = [Std, Lazy], args = DEDUP_SIZES)]
fn dedup_all<V: BenchVec>(bencher: divan::Bencher, sz: usize) {
    let template = vec![0u32; sz];
    bencher.with_inputs(|| V::from_slice(&template)).bench_values(|mut v| {
        v.dedup();
        divan::black_box(v.first_is_some());
        v
    });
}

#[divan::bench(types = [Std, Lazy])]
fn retain_even_100k<V: BenchVec>(bencher: divan::Bencher) {
    bencher.with_inputs(|| (1..=100_000u32).collect::<V>()).bench_values(|mut v| {
        v.retain_even();
        v
    });
}

// ---- push / pop / iter / drain -----------------------------------------------------------------

#[divan::bench(types = [Std, Lazy])]
fn grow_50k<V: BenchVec>(bencher: divan::Bencher) {
    bencher.with_inputs(|| V::with_capacity(50_000)).bench_values(|mut v| {
        for i in 0..50_000u32 {
            v.push(i);
        }
        v
    });
}

#[divan::bench(types = [Std, Lazy])]
fn pop_50k<V: BenchVec>(bencher: divan::Bencher) {
    bencher.with_inputs(|| (0..50_000u32).collect::<V>()).bench_values(|mut v| {
        while let Some(e) = v.pop() {
            divan::black_box(e);
        }
    });
}

#[divan::bench(types = [Std, Lazy])]
fn iter_sum_50k<V: BenchVec>(bencher: divan::Bencher) {
    let v: V = (0..50_000u32).collect();
    bencher.with_inputs(|| v.clone()).bench_values(|v| divan::black_box(v.iter_sum()));
}

#[divan::bench(types = [Std, Lazy])]
fn into_iter_sum_50k<V: BenchVec>(bencher: divan::Bencher) {
    let v: V = (0..50_000u32).collect();
    bencher.with_inputs(|| v.clone()).bench_values(|v| divan::black_box(v.into_iter_sum()));
}

#[divan::bench(types = [Std, Lazy])]
fn drain_sum_50k<V: BenchVec>(bencher: divan::Bencher) {
    let v: V = (0..50_000u32).collect();
    bencher.with_inputs(|| v.clone()).bench_values(|mut v| divan::black_box(v.drain_sum()));
}

// ---- collect shapes (chain / range) ------------------------------------------------------------

#[divan::bench(types = [Std, Lazy])]
fn chain_collect<V: BenchVec>(bencher: divan::Bencher) {
    let data = divan::black_box([0u32; LEN]);
    bencher.bench_local(|| data.iter().copied().chain([1]).collect::<V>());
}

#[divan::bench(types = [Std, Lazy])]
fn range_map_collect<V: BenchVec>(bencher: divan::Bencher) {
    bencher.bench_local(|| (0..LEN).map(|_| u32::default()).collect::<V>());
}

#[divan::bench(types = [Std, Lazy])]
fn extend_recycle<V: BenchVec>(bencher: divan::Bencher) {
    let src = vec![0u32; 1000];
    bencher.bench_local(|| {
        let mut v = V::new();
        v.extend(divan::black_box(src.iter().copied()));
        v
    });
}

#[divan::bench(types = [Std, Lazy])]
fn zip_fill_1000<V: BenchVec>(bencher: divan::Bencher) {
    let mut subst = vec![0u8; 1000];
    bench_rng().fill_bytes(&mut subst[..]);
    let data = vec![0u32; 1000];
    bencher.bench_local(|| {
        data.iter()
            .copied()
            .zip(subst.iter().copied())
            .enumerate()
            .map(|(i, (d, s))| d.wrapping_add(i as u32) ^ s as u32)
            .collect::<V>()
    });
}
