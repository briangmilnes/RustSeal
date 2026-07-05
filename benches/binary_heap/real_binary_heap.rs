// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! FAITHFUL Divan port of the real Rust benchmark suite
//! `library/alloctests/benches/binary_heap.rs` (Rust 1.97.0), run against our two
//! heaps interleaved in one process: the original (`unsafe_binary_heap`) vs the
//! winner (`lazy_hole_resort_binary_heap`). `ratio = winner / original`.
//!
//! Fidelity rules (r0005): every bench body reproduces the real `b.iter(|| …)` body
//! exactly — the timed region includes the setup the real bench times (the `clone`
//! in `from_vec`/`into_sorted_vec`, the `extend`/`clear` in `pop`/`push`), state
//! persists across iterations where the real bench declares it outside `b.iter`, and
//! every `test::black_box` is kept as `divan::black_box`. `b.iter` → `bench_local`.
//!
//! Element type: the real suite uses `u32` for find_smallest/from_vec/push and `i32`
//! for into_sorted_vec/pop (and `i32` for the `vec![42]` in peek_mut_deref_mut). For
//! a heap of a 4-byte signed-vs-unsigned integer the cost is identical, so all six
//! run at `u32` here to share one `BenchHeap<u32>` trait; noted as the one deliberate
//! type deviation (it does not affect timing).
//!
//! Run: `scripts/bench.sh --bench real_binary_heap`.

use rand::seq::SliceRandom;
use rustseal::binary_heap::lazy_hole_resort_binary_heap::LazyHoleResortBinaryHeap;
use rustseal::binary_heap::unsafe_binary_heap::UnsafeBinaryHeap;

fn main() {
    divan::main();
}

/// Seeded XorShift RNG (matches the libtest `crate::bench_rng()`), deterministic inputs.
fn bench_rng() -> rand_xorshift::XorShiftRng {
    const SEED: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    rand::SeedableRng::from_seed(SEED)
}

/// The heap API the six real benches touch, so one generic body runs against each variant.
/// The guard-based ops (`peek_mut`) are wrapped as trait methods that reproduce the real
/// inner statements verbatim.
trait BenchHeap: From<Vec<u32>> + FromIterator<u32> + Extend<u32> + Clone + Send + Sync + Sized {
    fn with_capacity(n: usize) -> Self;
    fn push(&mut self, x: u32);
    fn pop(&mut self) -> Option<u32>;
    fn clear(&mut self);
    fn into_sorted_vec(self) -> Vec<u32>;
    /// `find_smallest`'s inner op verbatim: `let mut max = heap.peek_mut().unwrap(); if x < *max { *max = x; }`
    fn replace_max_if_less(&mut self, x: u32);
    /// `peek_mut_deref_mut`'s inner op verbatim: one guard, write every value through it, forget it.
    fn peek_mut_deref_forget(&mut self, vals: &[u32]);
}

macro_rules! impl_bench_heap {
    ($ty:ident) => {
        impl BenchHeap for $ty<u32> {
            fn with_capacity(n: usize) -> Self { $ty::with_capacity(n) }
            fn push(&mut self, x: u32) { $ty::push(self, x) }
            fn pop(&mut self) -> Option<u32> { $ty::pop(self) }
            fn clear(&mut self) { $ty::clear(self) }
            fn into_sorted_vec(self) -> Vec<u32> { $ty::into_sorted_vec(self) }
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

/// `bench_find_smallest_1000` — build a heap of the first 1000 of a shuffled 100k stream,
/// then walk the remaining 99k keeping the 1000 smallest via `peek_mut`. Whole loop timed.
#[divan::bench(types = [Original, Winner])]
fn find_smallest_1000<H: BenchHeap>(bencher: divan::Bencher) {
    let mut vec: Vec<u32> = (0..100_000).collect();
    vec.shuffle(&mut bench_rng());
    bencher.bench_local(|| {
        let mut iter = vec.iter().copied();
        let mut heap: H = iter.by_ref().take(1000).collect();
        for x in iter {
            heap.replace_max_if_less(x);
        }
        heap
    });
}

/// `bench_peek_mut_deref_mut` — a persistent 1-element heap; each iteration writes 1M values
/// through one `peek_mut` guard then forgets it. `vec` is `black_box`ed exactly as upstream.
#[divan::bench(types = [Original, Winner])]
fn peek_mut_deref_mut<H: BenchHeap>(bencher: divan::Bencher) {
    let mut bheap = H::from(vec![42u32]);
    let vec: Vec<u32> = (0..1_000_000).collect();
    bencher.bench_local(|| {
        bheap.peek_mut_deref_forget(divan::black_box(&vec[..]));
    });
}

/// `bench_from_vec` — `From<Vec>` of a shuffled 100k vec; the `clone` is inside the timed region.
#[divan::bench(types = [Original, Winner])]
fn from_vec<H: BenchHeap>(bencher: divan::Bencher) {
    let mut vec: Vec<u32> = (0..100_000).collect();
    vec.shuffle(&mut bench_rng());
    bencher.bench_local(|| H::from(vec.clone()));
}

/// `bench_into_sorted_vec` — `clone().into_sorted_vec()` of a 10k heap; the `clone` is timed.
#[divan::bench(types = [Original, Winner])]
fn into_sorted_vec<H: BenchHeap>(bencher: divan::Bencher) {
    let bheap: H = (0..10_000u32).collect();
    bencher.bench_local(|| bheap.clone().into_sorted_vec());
}

/// `bench_push` — a persistent pre-reserved heap; each iteration pushes a shuffled 50k stream,
/// `black_box`es the heap, then `clear`s it. All three steps are timed, as upstream.
#[divan::bench(types = [Original, Winner])]
fn push<H: BenchHeap>(bencher: divan::Bencher) {
    let mut bheap = H::with_capacity(50_000);
    let mut vec: Vec<u32> = (0..50_000).collect();
    vec.shuffle(&mut bench_rng());
    bencher.bench_local(|| {
        for &i in &vec {
            bheap.push(i);
        }
        divan::black_box(&mut bheap);
        bheap.clear();
    });
}

/// `bench_pop` — a persistent pre-reserved heap; each iteration `extend`s 10k reversed, then
/// pops the heap empty. The `extend` is inside the timed region, as upstream.
#[divan::bench(types = [Original, Winner])]
fn pop<H: BenchHeap>(bencher: divan::Bencher) {
    let mut bheap = H::with_capacity(10_000);
    bencher.bench_local(|| {
        bheap.extend((0..10_000u32).rev());
        divan::black_box(&mut bheap);
        while let Some(elem) = bheap.pop() {
            divan::black_box(elem);
        }
    });
}
