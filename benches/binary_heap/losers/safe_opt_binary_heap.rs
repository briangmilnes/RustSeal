// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! Benchmarks for `SafeOptBinaryHeap`, a plain copy of the rust-libs 1.96.0 binary_heap
//! benchmarks (alloctests/benches/binary_heap.rs) with the type name changed. The seeded
//! `bench_rng` is inlined. `feature(test)` is unlocked via RUSTC_BOOTSTRAP=1 (.cargo/config.toml).

#![feature(test)]

extern crate test;

use rand::seq::SliceRandom;
use test::{black_box, Bencher};

use rustseal::binary_heap::safe_opt_binary_heap::SafeOptBinaryHeap;

/// A `rand::Rng` seeded with a consistent seed, to avoid nondeterminism in results.
fn bench_rng() -> rand_xorshift::XorShiftRng {
    const SEED: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    rand::SeedableRng::from_seed(SEED)
}

#[bench]
fn bench_find_smallest_1000(b: &mut Bencher) {
    let mut rng = bench_rng();
    let mut vec: Vec<u32> = (0..100_000).collect();
    vec.shuffle(&mut rng);

    b.iter(|| {
        let mut iter = vec.iter().copied();
        let mut heap: SafeOptBinaryHeap<_> = iter.by_ref().take(1000).collect();

        for x in iter {
            let mut max = heap.peek_mut().unwrap();
            // This comparison should be true only 1% of the time.
            // Unnecessary `sift_down`s will degrade performance.
            if x < *max {
                *max = x;
            }
        }

        heap
    })
}

#[bench]
fn bench_peek_mut_deref_mut(b: &mut Bencher) {
    let mut bheap = SafeOptBinaryHeap::from(vec![42]);
    let vec: Vec<u32> = (0..1_000_000).collect();

    b.iter(|| {
        let vec = black_box(&vec);
        let mut peek_mut = bheap.peek_mut().unwrap();
        // The compiler shouldn't be able to optimize away the `sift_down`
        // assignment in `PeekMut`'s `DerefMut` implementation since
        // the loop might not run.
        for &i in vec.iter() {
            *peek_mut = i;
        }
        // Remove the already minimal overhead of the sift_down.
        std::mem::forget(peek_mut);
    })
}

#[bench]
fn bench_from_vec(b: &mut Bencher) {
    let mut rng = bench_rng();
    let mut vec: Vec<u32> = (0..100_000).collect();
    vec.shuffle(&mut rng);

    b.iter(|| SafeOptBinaryHeap::from(vec.clone()))
}

#[bench]
fn bench_into_sorted_vec(b: &mut Bencher) {
    let bheap: SafeOptBinaryHeap<i32> = (0..10_000).collect();

    b.iter(|| bheap.clone().into_sorted_vec())
}

#[bench]
fn bench_push(b: &mut Bencher) {
    let mut bheap = SafeOptBinaryHeap::with_capacity(50_000);
    let mut rng = bench_rng();
    let mut vec: Vec<u32> = (0..50_000).collect();
    vec.shuffle(&mut rng);

    b.iter(|| {
        for &i in vec.iter() {
            bheap.push(i);
        }
        black_box(&mut bheap);
        bheap.clear();
    })
}

#[bench]
fn bench_pop(b: &mut Bencher) {
    let mut bheap = SafeOptBinaryHeap::with_capacity(10_000);

    b.iter(|| {
        bheap.extend((0..10_000).rev());
        black_box(&mut bheap);
        while let Some(elem) = bheap.pop() {
            black_box(elem);
        }
    })
}
