// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.

//! FAITHFUL Divan port of the real Rust standard-library `Vec` benchmark suite
//! `library/alloctests/benches/vec.rs` (Rust 1.97.0), run against our two extracted
//! variants interleaved in one process:
//!   - `rustseal::vec::std::VVec`               (the `std` baseline)
//!   - `rustseal::vec::lazy_loss_recovery::VVec` (the fixed variant)
//!
//! One Divan bench per upstream `#[bench]`, same name minus the `bench_` prefix, at the
//! upstream element type (u8 / u32 / u128 / usize / i32 / a custom `Droppable`). The
//! `in_place` macro family is expanded across u8/u32/u128 exactly as upstream. Each bench
//! is generic over a `BenchVec<T>` trait implemented (by macro) for both variants at every
//! element type used; per-(variant,type) aliases feed the Divan `types = [..]` lists.
//!
//! Fidelity: `b.iter(|| X)` -> `bencher.bench_local(|| X)`; the timed region reproduces the
//! upstream `b.iter` body verbatim (including any `clone`/`extend`/allocation it times);
//! state declared outside `b.iter` persists across iterations via a captured `&mut`; every
//! `test::black_box` is kept as `divan::black_box`. Helper/input vectors stay `std::vec::Vec`.
//! The seeded XorShift `bench_rng` matches the upstream `crate::bench_rng()`.
//!
//! Two deliberate ports of "what is timed", so the variant is actually exercised (upstream
//! builds a `std` `Vec` there regardless of the type under test):
//!   - `from_slice`: upstream times `src.as_slice().to_vec()` (a `std` `Vec`); here it builds
//!     `V::from(slice)` so the variant's allocation/copy is measured.
//!
//! Run: `scripts/bench.sh --bench real_vec` (read the `fastest` column on a quiescent machine).

#![feature(iter_next_chunk)] // Iterator::next_chunk in bench_next_chunk
#![feature(slice_partition_dedup)] // slice::partition_dedup in the dedup_slice_truncate benches

use std::iter::repeat;

use rand::RngCore;

use rustseal::vec::lazy_loss_recovery::VVec as LazyVVec;
use rustseal::vec::std::VVec as StdVVec;

fn main() {
    divan::main();
}

/// Seeded XorShift RNG (matches the upstream `crate::bench_rng()`), deterministic inputs.
fn bench_rng() -> rand_xorshift::XorShiftRng {
    const SEED: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    rand::SeedableRng::from_seed(SEED)
}

#[cfg(not(target_os = "emscripten"))]
const LEN: usize = 16384;
#[cfg(target_os = "emscripten")]
const LEN: usize = 4096;

/// The slice of the `VVec` API the workloads touch, so one generic body runs against each
/// variant at each element type. Everything reachable through a standard trait bound
/// (`FromIterator`, `Extend`, `IntoIterator`, `Deref`/`DerefMut`, `Clone`, `Default`) is a
/// supertrait; the remaining inherent methods are forwarded verbatim by `impl_bench_vec!`.
trait BenchVec<T>:
    Sized
    + Clone
    + Default
    + FromIterator<T>
    + Extend<T>
    + IntoIterator<Item = T>
    + core::ops::Deref<Target = [T]>
    + core::ops::DerefMut
{
    fn v_new() -> Self;
    fn v_with_capacity(n: usize) -> Self;
    fn v_from_elem(elem: T, n: usize) -> Self;
    fn v_from_slice(s: &[T]) -> Self;
    fn v_clear(&mut self);
    fn v_truncate(&mut self, len: usize);
    fn v_clone_from(&mut self, src: &Self);
    fn v_extend_from_slice(&mut self, s: &[T]);
    fn v_dedup(&mut self)
    where
        T: PartialEq;
    fn v_retain<F: FnMut(&T) -> bool>(&mut self, f: F);
    /// `Extend<&T>` (the `T: Copy` bulk-copy path) used by the ref-`extend` benches.
    fn v_extend_copied<'a, I: IntoIterator<Item = &'a T>>(&mut self, it: I)
    where
        T: Copy + 'a;
    fn v_as_mut_ptr(&mut self) -> *mut T;
    unsafe fn v_set_len(&mut self, new_len: usize);
}

macro_rules! impl_bench_vec {
    ($ty:ident, $from_elem:path) => {
        impl<T: Clone> BenchVec<T> for $ty<T> {
            fn v_new() -> Self {
                $ty::new()
            }
            fn v_with_capacity(n: usize) -> Self {
                $ty::with_capacity(n)
            }
            fn v_from_elem(elem: T, n: usize) -> Self {
                $from_elem(elem, n)
            }
            fn v_from_slice(s: &[T]) -> Self {
                $ty::from(s)
            }
            fn v_clear(&mut self) {
                $ty::clear(self)
            }
            fn v_truncate(&mut self, len: usize) {
                $ty::truncate(self, len)
            }
            fn v_clone_from(&mut self, src: &Self) {
                Clone::clone_from(self, src)
            }
            fn v_extend_from_slice(&mut self, s: &[T]) {
                $ty::extend_from_slice(self, s)
            }
            fn v_dedup(&mut self)
            where
                T: PartialEq,
            {
                $ty::dedup(self)
            }
            fn v_retain<F: FnMut(&T) -> bool>(&mut self, f: F) {
                $ty::retain(self, f)
            }
            fn v_extend_copied<'a, I: IntoIterator<Item = &'a T>>(&mut self, it: I)
            where
                T: Copy + 'a,
            {
                Extend::extend(self, it)
            }
            fn v_as_mut_ptr(&mut self) -> *mut T {
                $ty::as_mut_ptr(self)
            }
            unsafe fn v_set_len(&mut self, new_len: usize) {
                unsafe { $ty::set_len(self, new_len) }
            }
        }
    };
}
impl_bench_vec!(StdVVec, rustseal::vec::std::from_elem);
impl_bench_vec!(LazyVVec, rustseal::vec::lazy_loss_recovery::from_elem);

/// The `bench_transmute` round-trip (`u32 -> i32 -> u32`), which changes the element type and
/// so cannot ride the single `BenchVec<T>` type parameter. Implemented per variant.
trait BenchTransmute: Sized {
    fn transmute_roundtrip(self) -> Self;
}
macro_rules! impl_bench_transmute {
    ($ty:ident) => {
        impl BenchTransmute for $ty<u32> {
            fn transmute_roundtrip(self) -> Self {
                fn cast<X, Y>(input: $ty<X>) -> $ty<Y> {
                    input.into_iter().map(|e| unsafe { core::mem::transmute_copy(&e) }).collect()
                }
                let v = divan::black_box(cast::<u32, i32>(self));
                divan::black_box(cast::<i32, u32>(v))
            }
        }
    };
}
impl_bench_transmute!(StdVVec);
impl_bench_transmute!(LazyVVec);

// Per-(variant, element-type) aliases for the Divan `types = [..]` lists.
type SU8 = StdVVec<u8>;
type LU8 = LazyVVec<u8>;
type SU32 = StdVVec<u32>;
type LU32 = LazyVVec<u32>;
type SU128 = StdVVec<u128>;
type LU128 = LazyVVec<u128>;
type SUsize = StdVVec<usize>;
type LUsize = LazyVVec<usize>;
type SI32 = StdVVec<i32>;
type LI32 = LazyVVec<i32>;
type SDrop = StdVVec<Droppable>;
type LDrop = LazyVVec<Droppable>;

#[derive(Clone)]
struct Droppable(usize);

impl Drop for Droppable {
    fn drop(&mut self) {
        divan::black_box(self);
    }
}

/// Fill `buf` with a seeded xorshift stream masked by length, then sort (upstream helper).
fn random_sorted_fill(mut seed: u32, buf: &mut [u32]) {
    let mask = if buf.len() < 8192 {
        0xFF
    } else if buf.len() < 200_000 {
        0xFFFF
    } else {
        0xFFFF_FFFF
    };
    for item in buf.iter_mut() {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        *item = seed & mask;
    }
    buf.sort();
}

// ---- construction ------------------------------------------------------------------------------

/// `bench_new` — construct an empty `VVec<u32>`.
#[divan::bench(types = [SU32, LU32])]
fn new<V: BenchVec<u32>>(bencher: divan::Bencher) {
    bencher.bench_local(|| V::v_new());
}

fn body_with_capacity<V: BenchVec<u32>>(bencher: divan::Bencher, src_len: usize) {
    bencher.bench_local(|| V::v_with_capacity(divan::black_box(src_len)));
}
#[divan::bench(types = [SU32, LU32])]
fn with_capacity_0000<V: BenchVec<u32>>(bencher: divan::Bencher) {
    body_with_capacity::<V>(bencher, 0)
}
#[divan::bench(types = [SU32, LU32])]
fn with_capacity_0010<V: BenchVec<u32>>(bencher: divan::Bencher) {
    body_with_capacity::<V>(bencher, 10)
}
#[divan::bench(types = [SU32, LU32])]
fn with_capacity_0100<V: BenchVec<u32>>(bencher: divan::Bencher) {
    body_with_capacity::<V>(bencher, 100)
}
#[divan::bench(types = [SU32, LU32])]
fn with_capacity_1000<V: BenchVec<u32>>(bencher: divan::Bencher) {
    body_with_capacity::<V>(bencher, 1000)
}

fn body_from_fn<V: BenchVec<usize>>(bencher: divan::Bencher, src_len: usize) {
    bencher.bench_local(|| (0..divan::black_box(src_len)).collect::<V>());
}
#[divan::bench(types = [SUsize, LUsize])]
fn from_fn_0000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_from_fn::<V>(bencher, 0)
}
#[divan::bench(types = [SUsize, LUsize])]
fn from_fn_0010<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_from_fn::<V>(bencher, 10)
}
#[divan::bench(types = [SUsize, LUsize])]
fn from_fn_0100<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_from_fn::<V>(bencher, 100)
}
#[divan::bench(types = [SUsize, LUsize])]
fn from_fn_1000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_from_fn::<V>(bencher, 1000)
}

fn body_from_elem<V: BenchVec<usize>>(bencher: divan::Bencher, src_len: usize) {
    bencher.bench_local(|| repeat(5usize).take(divan::black_box(src_len)).collect::<V>());
}
#[divan::bench(types = [SUsize, LUsize])]
fn from_elem_0000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_from_elem::<V>(bencher, 0)
}
#[divan::bench(types = [SUsize, LUsize])]
fn from_elem_0010<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_from_elem::<V>(bencher, 10)
}
#[divan::bench(types = [SUsize, LUsize])]
fn from_elem_0100<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_from_elem::<V>(bencher, 100)
}
#[divan::bench(types = [SUsize, LUsize])]
fn from_elem_1000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_from_elem::<V>(bencher, 1000)
}

fn body_from_slice<V: BenchVec<usize>>(bencher: divan::Bencher, src_len: usize) {
    let src: Vec<usize> = (0..src_len).collect();
    bencher.bench_local(|| V::v_from_slice(divan::black_box(&src)));
}
#[divan::bench(types = [SUsize, LUsize])]
fn from_slice_0000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_from_slice::<V>(bencher, 0)
}
#[divan::bench(types = [SUsize, LUsize])]
fn from_slice_0010<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_from_slice::<V>(bencher, 10)
}
#[divan::bench(types = [SUsize, LUsize])]
fn from_slice_0100<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_from_slice::<V>(bencher, 100)
}
#[divan::bench(types = [SUsize, LUsize])]
fn from_slice_1000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_from_slice::<V>(bencher, 1000)
}

fn body_from_iter<V: BenchVec<usize>>(bencher: divan::Bencher, src_len: usize) {
    let src: V = (0..src_len).collect();
    bencher.bench_local(|| src.iter().cloned().collect::<V>());
}
#[divan::bench(types = [SUsize, LUsize])]
fn from_iter_0000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_from_iter::<V>(bencher, 0)
}
#[divan::bench(types = [SUsize, LUsize])]
fn from_iter_0010<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_from_iter::<V>(bencher, 10)
}
#[divan::bench(types = [SUsize, LUsize])]
fn from_iter_0100<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_from_iter::<V>(bencher, 100)
}
#[divan::bench(types = [SUsize, LUsize])]
fn from_iter_1000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_from_iter::<V>(bencher, 1000)
}

// ---- extend ------------------------------------------------------------------------------------

fn body_extend<V: BenchVec<usize>>(bencher: divan::Bencher, dst_len: usize, src_len: usize) {
    let dst: V = (0..dst_len).collect();
    let src: V = (dst_len..dst_len + src_len).collect();
    bencher.bench_local(|| {
        let mut dst = dst.clone();
        dst.extend(src.clone());
        dst
    });
}
#[divan::bench(types = [SUsize, LUsize])]
fn extend_0000_0000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_extend::<V>(bencher, 0, 0)
}
#[divan::bench(types = [SUsize, LUsize])]
fn extend_0000_0010<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_extend::<V>(bencher, 0, 10)
}
#[divan::bench(types = [SUsize, LUsize])]
fn extend_0000_0100<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_extend::<V>(bencher, 0, 100)
}
#[divan::bench(types = [SUsize, LUsize])]
fn extend_0000_1000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_extend::<V>(bencher, 0, 1000)
}
#[divan::bench(types = [SUsize, LUsize])]
fn extend_0010_0010<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_extend::<V>(bencher, 10, 10)
}
#[divan::bench(types = [SUsize, LUsize])]
fn extend_0100_0100<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_extend::<V>(bencher, 100, 100)
}
#[divan::bench(types = [SUsize, LUsize])]
fn extend_1000_1000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_extend::<V>(bencher, 1000, 1000)
}

/// `bench_extend_recycle` — persistent i32 buffer taken/rebuilt each iter and re-extended.
#[divan::bench(types = [SI32, LI32])]
fn extend_recycle<V: BenchVec<i32>>(bencher: divan::Bencher) {
    let mut data: V = V::v_from_elem(0, 1000);
    bencher.bench_local(|| {
        let tmp = core::mem::take(&mut data);
        let mut to_extend = divan::black_box(V::v_new());
        to_extend.extend(tmp.into_iter());
        data = divan::black_box(to_extend);
    });
    divan::black_box(&data);
}

fn body_extend_from_slice<V: BenchVec<usize>>(
    bencher: divan::Bencher,
    dst_len: usize,
    src_len: usize,
) {
    let dst: V = (0..dst_len).collect();
    let src: V = (dst_len..dst_len + src_len).collect();
    bencher.bench_local(|| {
        let mut dst = dst.clone();
        dst.v_extend_from_slice(&src);
        dst
    });
}
#[divan::bench(types = [SUsize, LUsize])]
fn extend_from_slice_0000_0000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_extend_from_slice::<V>(bencher, 0, 0)
}
#[divan::bench(types = [SUsize, LUsize])]
fn extend_from_slice_0000_0010<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_extend_from_slice::<V>(bencher, 0, 10)
}
#[divan::bench(types = [SUsize, LUsize])]
fn extend_from_slice_0000_0100<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_extend_from_slice::<V>(bencher, 0, 100)
}
#[divan::bench(types = [SUsize, LUsize])]
fn extend_from_slice_0000_1000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_extend_from_slice::<V>(bencher, 0, 1000)
}
#[divan::bench(types = [SUsize, LUsize])]
fn extend_from_slice_0010_0010<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_extend_from_slice::<V>(bencher, 10, 10)
}
#[divan::bench(types = [SUsize, LUsize])]
fn extend_from_slice_0100_0100<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_extend_from_slice::<V>(bencher, 100, 100)
}
#[divan::bench(types = [SUsize, LUsize])]
fn extend_from_slice_1000_1000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_extend_from_slice::<V>(bencher, 1000, 1000)
}

// ---- clone -------------------------------------------------------------------------------------

fn body_clone<V: BenchVec<usize>>(bencher: divan::Bencher, src_len: usize) {
    let src: V = (0..src_len).collect();
    bencher.bench_local(|| src.clone());
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_0000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone::<V>(bencher, 0)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_0010<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone::<V>(bencher, 10)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_0100<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone::<V>(bencher, 100)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_1000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone::<V>(bencher, 1000)
}

fn body_clone_from<V: BenchVec<usize>>(
    bencher: divan::Bencher,
    times: usize,
    dst_len: usize,
    src_len: usize,
) {
    let dst: V = (0..src_len).collect();
    let src: V = (dst_len..dst_len + src_len).collect();
    bencher.bench_local(|| {
        let mut dst = dst.clone();
        for _ in 0..times {
            dst.v_clone_from(&src);
            dst = divan::black_box(dst);
        }
        dst
    });
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_from_01_0000_0000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone_from::<V>(bencher, 1, 0, 0)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_from_01_0000_0010<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone_from::<V>(bencher, 1, 0, 10)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_from_01_0000_0100<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone_from::<V>(bencher, 1, 0, 100)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_from_01_0000_1000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone_from::<V>(bencher, 1, 0, 1000)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_from_01_0010_0010<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone_from::<V>(bencher, 1, 10, 10)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_from_01_0100_0100<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone_from::<V>(bencher, 1, 100, 100)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_from_01_1000_1000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone_from::<V>(bencher, 1, 1000, 1000)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_from_01_0010_0100<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone_from::<V>(bencher, 1, 10, 100)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_from_01_0100_1000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone_from::<V>(bencher, 1, 100, 1000)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_from_01_0010_0000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone_from::<V>(bencher, 1, 10, 0)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_from_01_0100_0010<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone_from::<V>(bencher, 1, 100, 10)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_from_01_1000_0100<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone_from::<V>(bencher, 1, 1000, 100)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_from_10_0000_0000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone_from::<V>(bencher, 10, 0, 0)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_from_10_0000_0010<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone_from::<V>(bencher, 10, 0, 10)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_from_10_0000_0100<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone_from::<V>(bencher, 10, 0, 100)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_from_10_0000_1000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone_from::<V>(bencher, 10, 0, 1000)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_from_10_0010_0010<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone_from::<V>(bencher, 10, 10, 10)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_from_10_0100_0100<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone_from::<V>(bencher, 10, 100, 100)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_from_10_1000_1000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone_from::<V>(bencher, 10, 1000, 1000)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_from_10_0010_0100<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone_from::<V>(bencher, 10, 10, 100)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_from_10_0100_1000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone_from::<V>(bencher, 10, 100, 1000)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_from_10_0010_0000<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone_from::<V>(bencher, 10, 10, 0)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_from_10_0100_0010<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone_from::<V>(bencher, 10, 100, 10)
}
#[divan::bench(types = [SUsize, LUsize])]
fn clone_from_10_1000_0100<V: BenchVec<usize>>(bencher: divan::Bencher) {
    body_clone_from::<V>(bencher, 10, 1000, 100)
}

// ---- in-place collect (type-variant family over u8/u32/u128) -----------------------------------

macro_rules! bench_in_place {
    ($($fname:ident, $type:ty, $count:expr, $init:expr, $s:ty, $l:ty);* $(;)?) => {
        $(
            #[divan::bench(types = [$s, $l])]
            fn $fname<V: BenchVec<$type>>(bencher: divan::Bencher) {
                bencher.bench_local(|| {
                    let src: V = divan::black_box(V::v_from_elem($init, $count));
                    src.into_iter()
                        .enumerate()
                        .map(|(idx, e)| idx as $type ^ e)
                        .collect::<V>()
                });
            }
        )+
    };
}

bench_in_place![
    in_place_xxu8_0010_i0,   u8,   10, 0, SU8,   LU8;
    in_place_xxu8_0100_i0,   u8,  100, 0, SU8,   LU8;
    in_place_xxu8_1000_i0,   u8, 1000, 0, SU8,   LU8;
    in_place_xxu8_0010_i1,   u8,   10, 1, SU8,   LU8;
    in_place_xxu8_0100_i1,   u8,  100, 1, SU8,   LU8;
    in_place_xxu8_1000_i1,   u8, 1000, 1, SU8,   LU8;
    in_place_xu32_0010_i0,  u32,   10, 0, SU32,  LU32;
    in_place_xu32_0100_i0,  u32,  100, 0, SU32,  LU32;
    in_place_xu32_1000_i0,  u32, 1000, 0, SU32,  LU32;
    in_place_xu32_0010_i1,  u32,   10, 1, SU32,  LU32;
    in_place_xu32_0100_i1,  u32,  100, 1, SU32,  LU32;
    in_place_xu32_1000_i1,  u32, 1000, 1, SU32,  LU32;
    in_place_u128_0010_i0, u128,   10, 0, SU128, LU128;
    in_place_u128_0100_i0, u128,  100, 0, SU128, LU128;
    in_place_u128_1000_i0, u128, 1000, 0, SU128, LU128;
    in_place_u128_0010_i1, u128,   10, 1, SU128, LU128;
    in_place_u128_0100_i1, u128,  100, 1, SU128, LU128;
    in_place_u128_1000_i1, u128, 1000, 1, SU128, LU128;
];

/// `bench_in_place_recycle` — persistent usize buffer recycled through enumerate/map/collect.
#[divan::bench(types = [SUsize, LUsize])]
fn in_place_recycle<V: BenchVec<usize>>(bencher: divan::Bencher) {
    let mut data: V = V::v_from_elem(0, 1000);
    bencher.bench_local(|| {
        let tmp = core::mem::take(&mut data);
        data = divan::black_box(
            tmp.into_iter()
                .enumerate()
                .map(|(idx, e)| idx.wrapping_add(e))
                .fuse()
                .collect::<V>(),
        );
    });
    divan::black_box(&data);
}

/// `bench_in_place_zip_recycle` — recycle a u8 buffer zipped against a seeded random buffer.
#[divan::bench(types = [SU8, LU8])]
fn in_place_zip_recycle<V: BenchVec<u8>>(bencher: divan::Bencher) {
    let mut data: V = V::v_from_elem(0u8, 1000);
    let mut subst = vec![0u8; 1000];
    bench_rng().fill_bytes(&mut subst[..]);
    bencher.bench_local(|| {
        let tmp = core::mem::take(&mut data);
        let mangled = tmp
            .into_iter()
            .zip(subst.iter().copied())
            .enumerate()
            .map(|(i, (d, s))| d.wrapping_add(i as u8) ^ s)
            .collect::<V>();
        data = divan::black_box(mangled);
    });
    divan::black_box(&data);
}

/// `bench_in_place_zip_iter_mut` — in-place `iter_mut` mangle of a persistent 256-byte buffer.
#[divan::bench(types = [SU8, LU8])]
fn in_place_zip_iter_mut<V: BenchVec<u8>>(bencher: divan::Bencher) {
    let mut data: V = V::v_from_elem(0u8, 256);
    let mut subst = vec![0u8; 1000];
    bench_rng().fill_bytes(&mut subst[..]);
    bencher.bench_local(|| {
        data.iter_mut().enumerate().for_each(|(i, d)| {
            *d = d.wrapping_add(i as u8) ^ subst[i];
        });
    });
    divan::black_box(&data);
}

/// `bench_transmute` — a persistent `u32` vec cast `u32 -> i32 -> u32` in place each iter.
#[divan::bench(types = [SU32, LU32])]
fn transmute<V: BenchVec<u32> + BenchTransmute>(bencher: divan::Bencher) {
    let mut vec: V = V::v_from_elem(10u32, 100);
    bencher.bench_local(|| {
        let v = core::mem::take(&mut vec);
        vec = v.transmute_roundtrip();
    });
    divan::black_box(&vec);
}

/// `bench_in_place_collect_droppable` — clone/skip/enumerate/map/collect over `Droppable`.
#[divan::bench(types = [SDrop, LDrop])]
fn in_place_collect_droppable<V: BenchVec<Droppable>>(bencher: divan::Bencher) {
    let v: V = std::iter::repeat_with(|| Droppable(0)).take(1000).collect();
    bencher.bench_local(|| {
        v.clone()
            .into_iter()
            .skip(100)
            .enumerate()
            .map(|(i, e)| Droppable(i ^ e.0))
            .collect::<V>()
    });
}

// ---- collect shapes (chain / range / map) ------------------------------------------------------

/// `bench_chain_collect`
#[divan::bench(types = [SI32, LI32])]
fn chain_collect<V: BenchVec<i32>>(bencher: divan::Bencher) {
    let data = divan::black_box([0i32; LEN]);
    bencher.bench_local(|| data.iter().cloned().chain([1]).collect::<V>());
}

/// `bench_chain_chain_collect`
#[divan::bench(types = [SI32, LI32])]
fn chain_chain_collect<V: BenchVec<i32>>(bencher: divan::Bencher) {
    let data = divan::black_box([0i32; LEN]);
    bencher.bench_local(|| data.iter().cloned().chain([1]).chain([2]).collect::<V>());
}

/// `bench_nest_chain_chain_collect`
#[divan::bench(types = [SI32, LI32])]
fn nest_chain_chain_collect<V: BenchVec<i32>>(bencher: divan::Bencher) {
    let data = divan::black_box([0i32; LEN]);
    bencher.bench_local(|| {
        data.iter().cloned().chain([1].iter().chain([2].iter()).cloned()).collect::<V>()
    });
}

/// `bench_range_map_collect`
#[divan::bench(types = [SU32, LU32])]
fn range_map_collect<V: BenchVec<u32>>(bencher: divan::Bencher) {
    bencher.bench_local(|| (0..LEN).map(|_| u32::default()).collect::<V>());
}

/// `bench_chain_extend_ref`
#[divan::bench(types = [SU32, LU32])]
fn chain_extend_ref<V: BenchVec<u32>>(bencher: divan::Bencher) {
    let data = divan::black_box([0u32; LEN]);
    bencher.bench_local(|| {
        let mut v = V::v_with_capacity(data.len() + 1);
        v.v_extend_copied(data.iter().chain([1].iter()));
        v
    });
}

/// `bench_chain_extend_value`
#[divan::bench(types = [SU32, LU32])]
fn chain_extend_value<V: BenchVec<u32>>(bencher: divan::Bencher) {
    let data = divan::black_box([0u32; LEN]);
    bencher.bench_local(|| {
        let mut v = V::v_with_capacity(data.len() + 1);
        v.extend(data.iter().cloned().chain(Some(1)));
        v
    });
}

/// `bench_rev_1`
#[divan::bench(types = [SU32, LU32])]
fn rev_1<V: BenchVec<u32>>(bencher: divan::Bencher) {
    let data = divan::black_box([0u32; LEN]);
    bencher.bench_local(|| {
        let mut v = V::v_new();
        v.v_extend_copied(data.iter().rev());
        v
    });
}

/// `bench_rev_2`
#[divan::bench(types = [SU32, LU32])]
fn rev_2<V: BenchVec<u32>>(bencher: divan::Bencher) {
    let data = divan::black_box([0u32; LEN]);
    bencher.bench_local(|| {
        let mut v = V::v_with_capacity(data.len());
        v.v_extend_copied(data.iter().rev());
        v
    });
}

/// `bench_map_regular`
#[divan::bench(types = [SU32, LU32])]
fn map_regular<V: BenchVec<u32>>(bencher: divan::Bencher) {
    let data = divan::black_box([(0, 0); LEN]);
    bencher.bench_local(|| {
        let mut v = V::v_new();
        v.extend(data.iter().map(|t| t.1));
        v
    });
}

/// `bench_map_fast` — write through the raw pointer, `set_len(i)` per step (verbatim upstream).
#[divan::bench(types = [SU32, LU32])]
fn map_fast<V: BenchVec<u32>>(bencher: divan::Bencher) {
    let data = divan::black_box([(0, 0); LEN]);
    bencher.bench_local(|| {
        let mut result: V = V::v_with_capacity(data.len());
        for i in 0..data.len() {
            unsafe {
                *result.v_as_mut_ptr().add(i) = data[i].0;
                result.v_set_len(i);
            }
        }
        result
    });
}

// ---- dedup / retain ----------------------------------------------------------------------------

fn body_dedup_slice_truncate<V: BenchVec<u32>>(bencher: divan::Bencher, sz: usize) {
    let mut template = vec![0u32; sz];
    random_sorted_fill(0x43, &mut template);
    let mut vec: V = V::v_from_slice(&template);
    bencher.bench_local(|| {
        let vec = divan::black_box(&mut vec);
        let len = {
            let (dedup, _) = vec.partition_dedup();
            dedup.len()
        };
        vec.v_truncate(len);
        divan::black_box(vec.first());
        let vec = divan::black_box(vec);
        vec.v_clear();
        vec.v_extend_from_slice(&template);
    });
}

fn body_dedup_random<V: BenchVec<u32>>(bencher: divan::Bencher, sz: usize) {
    let mut template = vec![0u32; sz];
    random_sorted_fill(0x43, &mut template);
    let mut vec: V = V::v_from_slice(&template);
    bencher.bench_local(|| {
        let vec = divan::black_box(&mut vec);
        vec.v_dedup();
        divan::black_box(vec.first());
        let vec = divan::black_box(vec);
        vec.v_clear();
        vec.v_extend_from_slice(&template);
    });
}

fn body_dedup_none<V: BenchVec<u32>>(bencher: divan::Bencher, sz: usize) {
    let mut template = vec![0u32; sz];
    template.chunks_exact_mut(2).for_each(|w| {
        w[0] = divan::black_box(0);
        w[1] = divan::black_box(5);
    });
    let mut vec: V = V::v_from_slice(&template);
    bencher.bench_local(|| {
        let vec = divan::black_box(&mut vec);
        vec.v_dedup();
        divan::black_box(vec.first());
        // Unlike the other dedup benches this does not reinitialize `vec`: it measures how
        // efficient dedup is when no memory is written.
    });
}

fn body_dedup_all<V: BenchVec<u32>>(bencher: divan::Bencher, sz: usize) {
    let mut template = vec![0u32; sz];
    template.iter_mut().for_each(|w| {
        *w = divan::black_box(0);
    });
    let mut vec: V = V::v_from_slice(&template);
    bencher.bench_local(|| {
        let vec = divan::black_box(&mut vec);
        vec.v_dedup();
        divan::black_box(vec.first());
        let vec = divan::black_box(vec);
        vec.v_clear();
        vec.v_extend_from_slice(&template);
    });
}

#[divan::bench(types = [SU32, LU32])]
fn dedup_slice_truncate_100<V: BenchVec<u32>>(bencher: divan::Bencher) {
    body_dedup_slice_truncate::<V>(bencher, 100)
}
#[divan::bench(types = [SU32, LU32])]
fn dedup_random_100<V: BenchVec<u32>>(bencher: divan::Bencher) {
    body_dedup_random::<V>(bencher, 100)
}
#[divan::bench(types = [SU32, LU32])]
fn dedup_none_100<V: BenchVec<u32>>(bencher: divan::Bencher) {
    body_dedup_none::<V>(bencher, 100)
}
#[divan::bench(types = [SU32, LU32])]
fn dedup_all_100<V: BenchVec<u32>>(bencher: divan::Bencher) {
    body_dedup_all::<V>(bencher, 100)
}
#[divan::bench(types = [SU32, LU32])]
fn dedup_slice_truncate_1000<V: BenchVec<u32>>(bencher: divan::Bencher) {
    body_dedup_slice_truncate::<V>(bencher, 1000)
}
#[divan::bench(types = [SU32, LU32])]
fn dedup_random_1000<V: BenchVec<u32>>(bencher: divan::Bencher) {
    body_dedup_random::<V>(bencher, 1000)
}
#[divan::bench(types = [SU32, LU32])]
fn dedup_none_1000<V: BenchVec<u32>>(bencher: divan::Bencher) {
    body_dedup_none::<V>(bencher, 1000)
}
#[divan::bench(types = [SU32, LU32])]
fn dedup_all_1000<V: BenchVec<u32>>(bencher: divan::Bencher) {
    body_dedup_all::<V>(bencher, 1000)
}
#[divan::bench(types = [SU32, LU32])]
fn dedup_slice_truncate_10000<V: BenchVec<u32>>(bencher: divan::Bencher) {
    body_dedup_slice_truncate::<V>(bencher, 10000)
}
#[divan::bench(types = [SU32, LU32])]
fn dedup_random_10000<V: BenchVec<u32>>(bencher: divan::Bencher) {
    body_dedup_random::<V>(bencher, 10000)
}
#[divan::bench(types = [SU32, LU32])]
fn dedup_none_10000<V: BenchVec<u32>>(bencher: divan::Bencher) {
    body_dedup_none::<V>(bencher, 10000)
}
#[divan::bench(types = [SU32, LU32])]
fn dedup_all_10000<V: BenchVec<u32>>(bencher: divan::Bencher) {
    body_dedup_all::<V>(bencher, 10000)
}
#[divan::bench(types = [SU32, LU32])]
fn dedup_slice_truncate_100000<V: BenchVec<u32>>(bencher: divan::Bencher) {
    body_dedup_slice_truncate::<V>(bencher, 100000)
}
#[divan::bench(types = [SU32, LU32])]
fn dedup_random_100000<V: BenchVec<u32>>(bencher: divan::Bencher) {
    body_dedup_random::<V>(bencher, 100000)
}
#[divan::bench(types = [SU32, LU32])]
fn dedup_none_100000<V: BenchVec<u32>>(bencher: divan::Bencher) {
    body_dedup_none::<V>(bencher, 100000)
}
#[divan::bench(types = [SU32, LU32])]
fn dedup_all_100000<V: BenchVec<u32>>(bencher: divan::Bencher) {
    body_dedup_all::<V>(bencher, 100000)
}

/// `bench_flat_map_collect` — flat_map a `u32` source's big-endian bytes into a `VVec<u8>`.
#[divan::bench(types = [SU8, LU8])]
fn flat_map_collect<V: BenchVec<u8>>(bencher: divan::Bencher) {
    let v = vec![777u32; 500000];
    bencher.bench_local(|| {
        v.iter().flat_map(|color| color.rotate_left(8).to_be_bytes()).collect::<V>()
    });
}

/// `bench_retain_iter_100000` — the filter-collect reference `retain` competes with.
#[divan::bench(types = [SI32, LI32])]
fn retain_iter_100000<V: BenchVec<i32>>(bencher: divan::Bencher) {
    let mut v: V = V::v_with_capacity(100000);
    bencher.bench_local(|| {
        let mut tmp = core::mem::take(&mut v);
        tmp.v_clear();
        tmp.extend(divan::black_box(1..=100000));
        v = tmp.into_iter().filter(|x| x & 1 == 0).collect();
    });
    divan::black_box(&v);
}

/// `bench_retain_100000`
#[divan::bench(types = [SI32, LI32])]
fn retain_100000<V: BenchVec<i32>>(bencher: divan::Bencher) {
    let mut v: V = V::v_with_capacity(100000);
    bencher.bench_local(|| {
        v.v_clear();
        v.extend(divan::black_box(1..=100000));
        v.v_retain(|x| x & 1 == 0)
    });
    divan::black_box(&v);
}

/// `bench_retain_whole_100000` — predicate keeps every element (worst case scan).
#[divan::bench(types = [SU32, LU32])]
fn retain_whole_100000<V: BenchVec<u32>>(bencher: divan::Bencher) {
    let mut v: V = divan::black_box(V::v_from_elem(826u32, 100000));
    bencher.bench_local(|| v.v_retain(|x| *x == 826u32));
    divan::black_box(&v);
}

/// `bench_next_chunk` — drain a cloned 2048-byte vec in chunks of 8 via `Iterator::next_chunk`.
#[divan::bench(types = [SU8, LU8])]
fn next_chunk<V: BenchVec<u8>>(bencher: divan::Bencher) {
    let v: V = V::v_from_elem(13u8, 2048);
    bencher.bench_local(|| {
        const CHUNK: usize = 8;
        let mut sum = [0u32; CHUNK];
        let mut iter = divan::black_box(v.clone()).into_iter();
        while let Ok(chunk) = iter.next_chunk::<CHUNK>() {
            for i in 0..CHUNK {
                sum[i] += chunk[i] as u32;
            }
        }
        sum
    });
}
