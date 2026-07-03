// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! Statistical comparison bench, **original vs winner**, via Divan.
//!
//! The per-variant libtest `#[bench]` files give one noisy sample each, measured in *separate*
//! processes — so two variants can't be compared fairly (different machine state per run). This
//! harness measures both heaps **interleaved in one process** with Divan's statistics (median +
//! spread over many samples), parameterized over a `BenchHeap` trait so the *same* workload runs
//! against each type. The same six rust-libs workloads are reproduced; setup is kept out of the
//! timed region (`with_inputs`), and the otherwise dead-store `peek_mut_deref_mut` loop is kept
//! only for parity (it is still a non-measurement — see the note on that bench).
//!
//! Run: `scripts/bench.sh --bench compare` (best on a quiescent machine).

use rand::seq::SliceRandom;
use rustseal::binary_heap::unsafe_binary_heap::UnsafeBinaryHeap;
use rustseal::binary_heap::lazy_hole_resort_binary_heap::LazyHoleResortBinaryHeap;

fn main() {
    divan::main();
}

/// Seeded XorShift RNG (same seed as the libtest benches) so inputs are deterministic.
fn bench_rng() -> rand_xorshift::XorShiftRng {
    const SEED: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    rand::SeedableRng::from_seed(SEED)
}

fn shuffled(n: u32) -> Vec<u32> {
    let mut v: Vec<u32> = (0..n).collect();
    v.shuffle(&mut bench_rng());
    v
}

/// The slice of the heap API the six workloads need, so one generic bench body runs against each
/// variant. All variants share the identical std-derived surface, so the impls are mechanical.
trait BenchHeap: From<Vec<u32>> + FromIterator<u32> + Extend<u32> + Clone + Send + Sync + Sized {
    fn with_capacity(n: usize) -> Self;
    fn push(&mut self, x: u32);
    fn pop(&mut self) -> Option<u32>;
    fn into_sorted_vec(self) -> Vec<u32>;
    /// `find_smallest`'s inner op: take a `peek_mut`, replace the max iff `x` is smaller.
    fn replace_max_if_less(&mut self, x: u32);
    /// `peek_mut_deref_mut`'s inner op: one guard, write every value through it, then forget it.
    fn peek_mut_deref_forget(&mut self, vals: &[u32]);
}

macro_rules! impl_bench_heap {
    ($ty:ident) => {
        impl BenchHeap for $ty<u32> {
            fn with_capacity(n: usize) -> Self {
                $ty::with_capacity(n)
            }
            fn push(&mut self, x: u32) {
                $ty::push(self, x)
            }
            fn pop(&mut self) -> Option<u32> {
                $ty::pop(self)
            }
            fn into_sorted_vec(self) -> Vec<u32> {
                $ty::into_sorted_vec(self)
            }
            fn replace_max_if_less(&mut self, x: u32) {
                let mut max = self.peek_mut().unwrap();
                if x < *max {
                    *max = x;
                }
            }
            fn peek_mut_deref_forget(&mut self, vals: &[u32]) {
                let mut pm = self.peek_mut().unwrap();
                for &i in vals {
                    *pm = i;
                }
                std::mem::forget(pm);
            }
        }
    };
}
impl_bench_heap!(UnsafeBinaryHeap);
impl_bench_heap!(LazyHoleResortBinaryHeap);

type Original = UnsafeBinaryHeap<u32>;
type Winner = LazyHoleResortBinaryHeap<u32>;

/// Build a heap of the first 1000 of a shuffled 100k stream, then walk the remaining 99k keeping the
/// 1000 smallest via `peek_mut` (the max is replaced ~1% of the time). The whole loop is timed.
#[divan::bench(types = [Original, Winner])]
fn find_smallest_1000<H: BenchHeap>(bencher: divan::Bencher) {
    let vec = shuffled(100_000);
    bencher.bench_local(|| {
        let mut iter = vec.iter().copied();
        let mut heap: H = iter.by_ref().take(1000).collect();
        for x in iter {
            heap.replace_max_if_less(x);
        }
        heap
    });
}

/// `From<Vec>` of a shuffled 100k vec = wrap + one *O*(n) `rebuild`. Setup (the clone) is untimed.
#[divan::bench(types = [Original, Winner])]
fn from_vec<H: BenchHeap>(bencher: divan::Bencher) {
    let vec = shuffled(100_000);
    bencher.with_inputs(|| vec.clone()).bench_values(|v| H::from(v));
}

/// `into_sorted_vec` of a 10k heap. The clone is untimed.
#[divan::bench(types = [Original, Winner])]
fn into_sorted_vec<H: BenchHeap>(bencher: divan::Bencher) {
    let heap: H = (0..10_000u32).collect();
    bencher.with_inputs(|| heap.clone()).bench_values(|h| h.into_sorted_vec());
}

/// NON-MEASUREMENT, kept for parity: a dead-store loop writing 1M values through one `peek_mut`
/// guard then forgetting it. The optimizer deletes the loop for most variants, so its number
/// measures the compiler, not the heap.
#[divan::bench(types = [Original, Winner])]
fn peek_mut_deref_mut<H: BenchHeap>(bencher: divan::Bencher) {
    let vals: Vec<u32> = (0..1_000_000).collect();
    bencher
        .with_inputs(|| H::from(vec![42u32]))
        .bench_values(|mut heap| {
            heap.peek_mut_deref_forget(divan::black_box(&vals));
            heap
        });
}

/// Pop a 10k heap empty. The heap build (`with_capacity` + reversed extend) is untimed; only the
/// pop loop is measured (cleaner than the libtest bench, which timed the build too).
#[divan::bench(types = [Original, Winner])]
fn pop<H: BenchHeap>(bencher: divan::Bencher) {
    bencher
        .with_inputs(|| {
            let mut h = H::with_capacity(10_000);
            h.extend((0..10_000u32).rev());
            h
        })
        .bench_values(|mut h| {
            while let Some(e) = h.pop() {
                divan::black_box(e);
            }
        });
}

/// Push a shuffled 50k stream into a pre-reserved heap. The empty heap is the (untimed) input.
#[divan::bench(types = [Original, Winner])]
fn push<H: BenchHeap>(bencher: divan::Bencher) {
    let vec = shuffled(50_000);
    bencher
        .with_inputs(|| H::with_capacity(50_000))
        .bench_values(|mut h| {
            for &i in &vec {
                h.push(i);
            }
            h
        });
}
