// Copyright (C) 2026 Brian G. Milnes <briangmilnes@gmail.com>, All Rights Reserved.
//! A contiguous growable array type with heap-allocated contents, written
//! `VVec<T>`.
//!
//! Vectors have *O*(1) indexing, amortized *O*(1) push (to the end) and
//! *O*(1) pop (from the end).
//!
//! Vectors ensure they never allocate more than `isize::MAX` bytes.
//!
//! # Examples
//!
//! You can explicitly create a [`VVec`] with [`VVec::new`]:
//!
//!
//! ...or by using the [`vec!`] macro:
//!
//!
//! You can [`push`] values onto the end of a vector (which will grow the vector
//! as needed):
//!
//!
//! Popping values works in much the same way:
//!
//!
//! Vectors also support indexing (through the [`Index`] and [`IndexMut`] traits):
//!
//!
//! # Memory layout
//!
//! When the type is non-zero-sized and the capacity is nonzero, [`VVec`] uses the [`Global`]
//! allocator for its allocation. It is valid to convert both ways between such a [`VVec`] and a raw
//! pointer allocated with the [`Global`] allocator, provided that the [`Layout`] used with the
//! allocator is correct for a sequence of `capacity` elements of the type, and the first `len`
//! values pointed to by the raw pointer are valid. More precisely, a `ptr: *mut T` that has been
//! allocated with the [`Global`] allocator with [`Layout::array::<T>(capacity)`][Layout::array] may
//! be converted into a vec using
//! [`VVec::<T>::from_raw_parts(ptr, len, capacity)`](VVec::from_raw_parts). Conversely, the memory
//! backing a `value: *mut T` obtained from [`VVec::<T>::as_mut_ptr`] may be deallocated using the
//! [`Global`] allocator with the same layout.
//!
//! For zero-sized types (ZSTs), or when the capacity is zero, the `VVec` pointer must be non-null
//! and sufficiently aligned. The recommended way to build a `VVec` of ZSTs if [`vec!`] cannot be
//! used is to use [`ptr::NonNull::dangling`].
//!
//! [`push`]: VVec::push
//! [`ptr::NonNull::dangling`]: NonNull::dangling
//! [`Layout`]: std::alloc::Layout
//! [Layout::array]: std::alloc::Layout::array

use core::clone::TrivialClone;
use core::cmp::Ordering;
use core::hash::{Hash, Hasher};

use core::iter;

use core::marker::{Freeze, PhantomData};
use core::mem::{self, Assume, ManuallyDrop, MaybeUninit, SizedTypeProperties, TransmuteFrom};
use core::ops::{self, Index, IndexMut, Range, RangeBounds};
use core::ptr::{self, NonNull};
use core::slice::{self, SliceIndex};
use core::{cmp, fmt, hint, intrinsics};

pub use self::extract_if::VVecExtractIf;
use std::alloc::{Allocator, Global};
use std::borrow::{Cow, ToOwned};
use std::collections::TryReserveError;

// VRawVec is our shim standing in for alloc's module-private `RawVec` (see
// raw_vec_shim.rs for why a byte-faithful RawVec copy does not compile on this
// toolchain). The Vec body below uses it exactly as upstream uses RawVec.
#[path = "raw_vec_shim.rs"] mod raw_vec_shim;
use raw_vec_shim::VRawVec;

#[path = "extract_if.rs"] mod extract_if;

pub use self::splice::VVecSplice;

#[path = "splice.rs"] mod splice;

pub use self::drain::VVecDrain;

#[path = "drain.rs"] mod drain;

// CORPSE (ProcessCommentingStandard): every impl in `cow` converts to/from
// `std::borrow::Cow`, a FOREIGN type — all orphan-rule violations here (see cow.rs).
// #[path = "cow.rs"] mod cow;

// CORPSE (ProcessCommentingStandard): `in_place_collect` is the in-place iteration
// specialization — it reuses an `VVecIntoIter`'s backing allocation as the output
// buffer while collecting. It manipulates the raw allocation directly and cannot be
// expressed over the `VRawVec`-over-`std::Vec` shim, so it is dropped. `FromIterator`
// still works via the naive `spec_from_iter_nested` path (build then extend); only the
// zero-copy in-place reuse is lost (a perf optimization, not a behavior).
// pub(crate) use self::in_place_collect::AsVecIntoIter;
// #[path = "in_place_collect.rs"] mod in_place_collect;
pub use self::into_iter::VVecIntoIter;

#[path = "into_iter.rs"] mod into_iter;

// CORPSE (ProcessCommentingStandard): `is_zero` provides the `IsZero` trait used only by
// the `from_elem` fast path (zeroed allocation for zero-valued elements). That path needs
// `VRawVec::with_capacity_zeroed_in`, which the shim cannot provide over public `std::Vec`.
// `from_elem` falls back to the generic `T: Clone` path in `spec_from_elem`.
// use self::is_zero::IsZero;
// #[path = "is_zero.rs"] mod is_zero;

#[path = "partial_eq.rs"] mod partial_eq;

pub use self::peek_mut::VVecPeekMut;

#[path = "peek_mut.rs"] mod peek_mut;

use self::spec_from_elem::SpecFromElem;

#[path = "spec_from_elem.rs"] mod spec_from_elem;

use self::set_len_on_drop::VVecSetLenOnDrop;

#[path = "set_len_on_drop.rs"] mod set_len_on_drop;

// CORPSE (ProcessCommentingStandard): `in_place_drop` provides the drop guards
// (`InPlaceDrop`, `InPlaceDstDataSrcBufDrop`) used exclusively by `in_place_collect`,
// which is itself a corpse (see above). Nothing else references them.
// use self::in_place_drop::{InPlaceDrop, InPlaceDstDataSrcBufDrop};
// #[path = "in_place_drop.rs"] mod in_place_drop;

use self::spec_from_iter_nested::SpecFromIterNested;

#[path = "spec_from_iter_nested.rs"] mod spec_from_iter_nested;

// CORPSE (ProcessCommentingStandard): the top-level `spec_from_iter` specialization
// dispatches the `VVecIntoIter` case into the in-place machinery (`in_place_collect`),
// a corpse. `FromIterator::from_iter` below routes directly to `SpecFromIterNested`
// (the naive build-then-extend path), which is the generic fallback this file used to
// reach through `spec_from_iter`'s default impl anyway.
// use self::spec_from_iter::SpecFromIter;
// #[path = "spec_from_iter.rs"] mod spec_from_iter;

use self::spec_extend::SpecExtend;

#[path = "spec_extend.rs"] mod spec_extend;

/// A contiguous growable array type, written as `VVec<T>`, short for 'vector'.
///
/// # Examples
///
///
/// The [`vec!`] macro is provided for convenient initialization:
///
///
/// It can also initialize each element of a `VVec<T>` with a given value.
/// This may be more efficient than performing allocation and initialization
/// in separate steps, especially when initializing a vector of zeros:
///
///
/// For more information, see
/// [Capacity and Reallocation](#capacity-and-reallocation).
///
/// Use a `VVec<T>` as an efficient stack:
///
///
/// # Indexing
///
/// The `VVec` type allows access to values by index, because it implements the
/// [`Index`] trait. An example will be more explicit:
///
///
/// However be careful: if you try to access an index which isn't in the `VVec`,
/// your software will panic! You cannot do this:
///
///
/// Use [`get`] and [`get_mut`] if you want to check whether the index is in
/// the `VVec`.
///
/// # Slicing
///
/// A `VVec` can be mutable. On the other hand, slices are read-only objects.
/// To get a [slice][prim@slice], use [`&`]. Example:
///
///
/// In Rust, it's more common to pass slices as arguments rather than vectors
/// when you just want to provide read access. The same goes for [`String`] and
/// [`&str`].
///
/// # Capacity and reallocation
///
/// The capacity of a vector is the amount of space allocated for any future
/// elements that will be added onto the vector. This is not to be confused with
/// the *length* of a vector, which specifies the number of actual elements
/// within the vector. If a vector's length exceeds its capacity, its capacity
/// will automatically be increased, but its elements will have to be
/// reallocated.
///
/// For example, a vector with capacity 10 and length 0 would be an empty vector
/// with space for 10 more elements. Pushing 10 or fewer elements onto the
/// vector will not change its capacity or cause reallocation to occur. However,
/// if the vector's length is increased to 11, it will have to reallocate, which
/// can be slow. For this reason, it is recommended to use [`VVec::with_capacity`]
/// whenever possible to specify how big the vector is expected to get.
///
/// # Guarantees
///
/// Due to its incredibly fundamental nature, `VVec` makes a lot of guarantees
/// about its design. This ensures that it's as low-overhead as possible in
/// the general case, and can be correctly manipulated in primitive ways
/// by unsafe code. Note that these guarantees refer to an unqualified `VVec<T>`.
/// If additional type parameters are added (e.g., to support custom allocators),
/// overriding their defaults may change the behavior.
///
/// Most fundamentally, `VVec` is and always will be a (pointer, capacity, length)
/// triplet. No more, no less. The order of these fields is completely
/// unspecified, and you should use the appropriate methods to modify these.
/// The pointer will never be null, so this type is null-pointer-optimized.
///
/// However, the pointer might not actually point to allocated memory. In particular,
/// if you construct a `VVec` with capacity 0 via [`VVec::new`], [`vec![]`][`vec!`],
/// [`VVec::with_capacity(0)`][`VVec::with_capacity`], or by calling [`shrink_to_fit`]
/// on an empty VVec, it will not allocate memory. Similarly, if you store zero-sized
/// types inside a `VVec`, it will not allocate space for them. *Note that in this case
/// the `VVec` might not report a [`capacity`] of 0*. `VVec` will allocate if and only
/// if <code>[size_of::\<T>]\() * [capacity]\() > 0</code>. In general, `VVec`'s allocation
/// details are very subtle --- if you intend to allocate memory using a `VVec`
/// and use it for something else (either to pass to unsafe code, or to build your
/// own memory-backed collection), be sure to deallocate this memory by using
/// `from_raw_parts` to recover the `VVec` and then dropping it.
///
/// If a `VVec` *has* allocated memory, then the memory it points to is on the heap
/// (as defined by the allocator Rust is configured to use by default), and its
/// pointer points to [`len`] initialized, contiguous elements in order (what
/// you would see if you coerced it to a slice), followed by <code>[capacity] - [len]</code>
/// logically uninitialized, contiguous elements.
///
/// A vector containing the elements `'a'` and `'b'` with capacity 4 can be
/// visualized as below. The top part is the `VVec` struct, it contains a
/// pointer to the head of the allocation in the heap, length and capacity.
/// The bottom part is the allocation on the heap, a contiguous memory block.
///
///
/// - **uninit** represents memory that is not initialized, see [`MaybeUninit`].
/// - Note: the ABI is not stable and `VVec` makes no guarantees about its memory
///   layout (including the order of fields).
///
/// `VVec` will never perform a "small optimization" where elements are actually
/// stored on the stack for two reasons:
///
/// * It would make it more difficult for unsafe code to correctly manipulate
///   a `VVec`. The contents of a `VVec` wouldn't have a stable address if it were
///   only moved, and it would be more difficult to determine if a `VVec` had
///   actually allocated memory.
///
/// * It would penalize the general case, incurring an additional branch
///   on every access.
///
/// `VVec` will never automatically shrink itself, even if completely empty. This
/// ensures no unnecessary allocations or deallocations occur. Emptying a `VVec`
/// and then filling it back up to the same [`len`] should incur no calls to
/// the allocator. If you wish to free up unused memory, use
/// [`shrink_to_fit`] or [`shrink_to`].
///
/// [`push`] and [`insert`] will never (re)allocate if the reported capacity is
/// sufficient. [`push`] and [`insert`] *will* (re)allocate if
/// <code>[len] == [capacity]</code>. That is, the reported capacity is completely
/// accurate, and can be relied on. It can even be used to manually free the memory
/// allocated by a `VVec` if desired. Bulk insertion methods *may* reallocate, even
/// when not necessary.
///
/// `VVec` does not guarantee any particular growth strategy when reallocating
/// when full, nor when [`reserve`] is called. The current strategy is basic
/// and it may prove desirable to use a non-constant growth factor. Whatever
/// strategy is used will of course guarantee *O*(1) amortized [`push`].
///
/// It is guaranteed, in order to respect the intentions of the programmer, that
/// all of `vec![e_1, e_2, ..., e_n]`, `vec![x; n]`, and [`VVec::with_capacity(n)`] produce a `VVec`
/// that requests an allocation of the exact size needed for precisely `n` elements from the allocator,
/// and no other size (such as, for example: a size rounded up to the nearest power of 2).
/// The allocator will return an allocation that is at least as large as requested, but it may be larger.
///
/// It is guaranteed that the [`VVec::capacity`] method returns a value that is at least the requested capacity
/// and not more than the allocated capacity.
///
/// The method [`VVec::shrink_to_fit`] will attempt to discard excess capacity an allocator has given to a `VVec`.
/// If <code>[len] == [capacity]</code>, then a `VVec<T>` can be converted
/// to and from a [`Box<[T]>`][owned slice] without reallocating or moving the elements.
/// `VVec` exploits this fact as much as reasonable when implementing common conversions
/// such as [`into_boxed_slice`].
///
/// `VVec` will not specifically overwrite any data that is removed from it,
/// but also won't specifically preserve it. Its uninitialized memory is
/// scratch space that it may use however it wants. It will generally just do
/// whatever is most efficient or otherwise easy to implement. Do not rely on
/// removed data to be erased for security purposes. Even if you drop a `VVec`, its
/// buffer may simply be reused by another allocation. Even if you zero a `VVec`'s memory
/// first, that might not actually happen because the optimizer does not consider
/// this a side-effect that must be preserved. There is one case which we will
/// not break, however: using `unsafe` code to write to the excess capacity,
/// and then increasing the length to match, is always valid.
///
/// Currently, `VVec` does not guarantee the order in which elements are dropped.
/// The order has changed in the past and may change again.
///
/// [`get`]: slice::get
/// [`get_mut`]: slice::get_mut
/// [`String`]: std::string::String
/// [`&str`]: type@str
/// [`shrink_to_fit`]: VVec::shrink_to_fit
/// [`shrink_to`]: VVec::shrink_to
/// [capacity]: VVec::capacity
/// [`capacity`]: VVec::capacity
/// [`VVec::capacity`]: VVec::capacity
/// [size_of::\<T>]: size_of
/// [len]: VVec::len
/// [`len`]: VVec::len
/// [`push`]: VVec::push
/// [`insert`]: VVec::insert
/// [`reserve`]: VVec::reserve
/// [`VVec::with_capacity(n)`]: VVec::with_capacity
/// [`MaybeUninit`]: core::mem::MaybeUninit
/// [owned slice]: Box
/// [`into_boxed_slice`]: VVec::into_boxed_slice

#[doc(alias = "list")]
#[doc(alias = "vector")]
pub struct VVec<T, A: Allocator = Global> {
    buf: VRawVec<T, A>,
    len: usize,
    // Forget/leak resistance for `drain`/`extract_if`/`splice` (the lazy_loss_recovery difference
    // from `std`). While such an iterator is live, `len` is parked short exactly as std does; std
    // relies solely on the iterator's `drop` to finish (restore the tail + `len`), so a `mem::forget`
    // loses elements. Here the constructor also records the finish work in `pending`, the iterator
    // mirrors its progress into it, and if the iterator is forgotten the next deque op finishes it
    // via `restore_wf_wo_data_loss` (guard at every mutating entry point + in `VVec`'s own `Drop`).
    // `len()` reports the post-removal length while parked, so the vec looks finished from outside.
    // The cases are mutually exclusive (one iterator at a time), so a single slot suffices.
    //
    // BOXED so this field is one nullable pointer (8 B) rather than 40 B inline: `pop` and friends
    // are so cheap that a 40 B inline field measurably fattened `VVec` and slowed them; the boxed
    // `Pending` allocates only on the rare forget-prone ops (`drain`/`extract_if`/`splice`), which
    // already touch the heap.
    pending: Option<Box<Pending>>,
}

// The work `restore_wf_wo_data_loss` must do to finish a forgotten iterator — the same finish the
// iterator's own `drop` would have done. All fields are `usize` (offsets into the buffer), so the
// enum is non-generic.
enum Pending {
    // `drain`: `drop_offset`/`drop_len` delimit the un-yielded drained range (front-advanced by
    // `next`, back-retreated by `next_back`); `tail_start`/`tail_len` are the elements after the
    // range (constant). Parked `len` == the head length.
    Drain { tail_start: usize, tail_len: usize, drop_offset: usize, drop_len: usize },
    // `extract_if`: `idx` is the next element to inspect, `del` how many have been removed so far,
    // `old_len` the length before extraction. Parked `len` == 0. Finish == compact the un-inspected
    // tail back over the `del` holes and set `len = old_len - del`.
    ExtractIf { old_len: usize, idx: usize, del: usize },
}

////////////////////////////////////////////////////////////////////////////////
// Inherent methods
////////////////////////////////////////////////////////////////////////////////

impl<T> VVec<T> {
    /// Constructs a new, empty `VVec<T>`.
    ///
    /// The vector will not allocate until elements are pushed onto it.
    ///
    /// # Examples
    ///
    #[inline]

    #[must_use]
    pub const fn new() -> Self {
        VVec { buf: VRawVec::new(), len: 0, pending: None }
    }

    /// Constructs a new, empty `VVec<T>` with at least the specified capacity.
    ///
    /// The vector will be able to hold at least `capacity` elements without
    /// reallocating. This method is allowed to allocate for more elements than
    /// `capacity`. If `capacity` is zero, the vector will not allocate.
    ///
    /// It is important to note that although the returned vector has the
    /// minimum *capacity* specified, the vector will have a zero *length*. For
    /// an explanation of the difference between length and capacity, see
    /// *[Capacity and reallocation]*.
    ///
    /// If it is important to know the exact allocated capacity of a `VVec`,
    /// always use the [`capacity`] method after construction.
    ///
    /// For `VVec<T>` where `T` is a zero-sized type, there will be no allocation
    /// and the capacity will always be `usize::MAX`.
    ///
    /// [Capacity and reallocation]: #capacity-and-reallocation
    /// [`capacity`]: VVec::capacity
    ///
    /// # Panics
    ///
    /// Panics if the new capacity exceeds `isize::MAX` _bytes_.
    ///
    /// # Examples
    ///

    #[inline]

    #[must_use]

    pub fn with_capacity(capacity: usize) -> Self {
        Self::with_capacity_in(capacity, Global)
    }

    /// Constructs a new, empty `VVec<T>` with at least the specified capacity.
    ///
    /// The vector will be able to hold at least `capacity` elements without
    /// reallocating. This method is allowed to allocate for more elements than
    /// `capacity`. If `capacity` is zero, the vector will not allocate.
    ///
    /// # Errors
    ///
    /// Returns an error if the capacity exceeds `isize::MAX` _bytes_,
    /// or if the allocator reports allocation failure.
    #[inline]

    pub fn try_with_capacity(capacity: usize) -> Result<Self, TryReserveError> {
        Self::try_with_capacity_in(capacity, Global)
    }

    /// Creates a `VVec<T>` directly from a pointer, a length, and a capacity.
    ///
    /// # Safety
    ///
    /// This is highly unsafe, due to the number of invariants that aren't
    /// checked:
    ///
    /// * If `T` is not a zero-sized type and the capacity is nonzero, `ptr` must have
    ///   been allocated using the global allocator, such as via the [`alloc::alloc`]
    ///   function. If `T` is a zero-sized type or the capacity is zero, `ptr` need
    ///   only be non-null and aligned.
    /// * `T` needs to have the same alignment as what `ptr` was allocated with,
    ///   if the pointer is required to be allocated.
    ///   (`T` having a less strict alignment is not sufficient, the alignment really
    ///   needs to be equal to satisfy the [`dealloc`] requirement that memory must be
    ///   allocated and deallocated with the same layout.)
    /// * The size of `T` times the `capacity` (i.e. the allocated size in bytes), if
    ///   nonzero, needs to be the same size as the pointer was allocated with.
    ///   (Because similar to alignment, [`dealloc`] must be called with the same
    ///   layout `size`.)
    /// * `length` needs to be less than or equal to `capacity`.
    /// * The first `length` values must be properly initialized values of type `T`.
    /// * `capacity` needs to be the capacity that the pointer was allocated with,
    ///   if the pointer is required to be allocated.
    /// * The allocated size in bytes must be no larger than `isize::MAX`.
    ///   See the safety documentation of [`pointer::offset`].
    ///
    /// These requirements are always upheld by any `ptr` that has been allocated
    /// via `VVec<T>`. Other allocation sources are allowed if the invariants are
    /// upheld.
    ///
    /// Violating these may cause problems like corrupting the allocator's
    /// internal data structures. For example it is normally **not** safe
    /// to build a `VVec<u8>` from a pointer to a C `char` array with length
    /// `size_t`, doing so is only safe if the array was initially allocated by
    /// a `VVec` or `String`.
    /// It's also not safe to build one from a `VVec<u16>` and its length, because
    /// the allocator cares about the alignment, and these two types have different
    /// alignments. The buffer was allocated with alignment 2 (for `u16`), but after
    /// turning it into a `VVec<u8>` it'll be deallocated with alignment 1. To avoid
    /// these issues, it is often preferable to do casting/transmuting using
    /// [`slice::from_raw_parts`] instead.
    ///
    /// The ownership of `ptr` is effectively transferred to the
    /// `VVec<T>` which may then deallocate, reallocate or change the
    /// contents of memory pointed to by the pointer at will. Ensure
    /// that nothing else uses the pointer after calling this
    /// function.
    ///
    /// [`String`]: std::string::String
    /// [`alloc::alloc`]: std::alloc::alloc
    /// [`dealloc`]: std::alloc::GlobalAlloc::dealloc
    ///
    /// # Examples
    ///
    ///
    /// Using memory that was allocated elsewhere:
    ///
    #[inline]

    pub unsafe fn from_raw_parts(ptr: *mut T, length: usize, capacity: usize) -> Self {
        unsafe { Self::from_raw_parts_in(ptr, length, capacity, Global) }
    }

    #[doc(alias = "from_non_null_parts")]
    /// Creates a `VVec<T>` directly from a `NonNull` pointer, a length, and a capacity.
    ///
    /// # Safety
    ///
    /// This is highly unsafe, due to the number of invariants that aren't
    /// checked:
    ///
    /// * `ptr` must have been allocated using the global allocator, such as via
    ///   the [`alloc::alloc`] function.
    /// * `T` needs to have the same alignment as what `ptr` was allocated with.
    ///   (`T` having a less strict alignment is not sufficient, the alignment really
    ///   needs to be equal to satisfy the [`dealloc`] requirement that memory must be
    ///   allocated and deallocated with the same layout.)
    /// * The size of `T` times the `capacity` (i.e. the allocated size in bytes) needs
    ///   to be the same size as the pointer was allocated with. (Because similar to
    ///   alignment, [`dealloc`] must be called with the same layout `size`.)
    /// * `length` needs to be less than or equal to `capacity`.
    /// * The first `length` values must be properly initialized values of type `T`.
    /// * `capacity` needs to be the capacity that the pointer was allocated with.
    /// * The allocated size in bytes must be no larger than `isize::MAX`.
    ///   See the safety documentation of [`pointer::offset`].
    ///
    /// These requirements are always upheld by any `ptr` that has been allocated
    /// via `VVec<T>`. Other allocation sources are allowed if the invariants are
    /// upheld.
    ///
    /// Violating these may cause problems like corrupting the allocator's
    /// internal data structures. For example it is normally **not** safe
    /// to build a `VVec<u8>` from a pointer to a C `char` array with length
    /// `size_t`, doing so is only safe if the array was initially allocated by
    /// a `VVec` or `String`.
    /// It's also not safe to build one from a `VVec<u16>` and its length, because
    /// the allocator cares about the alignment, and these two types have different
    /// alignments. The buffer was allocated with alignment 2 (for `u16`), but after
    /// turning it into a `VVec<u8>` it'll be deallocated with alignment 1. To avoid
    /// these issues, it is often preferable to do casting/transmuting using
    /// [`NonNull::slice_from_raw_parts`] instead.
    ///
    /// The ownership of `ptr` is effectively transferred to the
    /// `VVec<T>` which may then deallocate, reallocate or change the
    /// contents of memory pointed to by the pointer at will. Ensure
    /// that nothing else uses the pointer after calling this
    /// function.
    ///
    /// [`String`]: std::string::String
    /// [`alloc::alloc`]: std::alloc::alloc
    /// [`dealloc`]: std::alloc::GlobalAlloc::dealloc
    ///
    /// # Examples
    ///
    ///
    /// Using memory that was allocated elsewhere:
    ///
    #[inline]

    pub unsafe fn from_parts(ptr: NonNull<T>, length: usize, capacity: usize) -> Self {
        unsafe { Self::from_parts_in(ptr, length, capacity, Global) }
    }

    /// Creates a `VVec<T>` where each element is produced by calling `f` with
    /// that element's index while walking forward through the `VVec<T>`.
    ///
    /// This is essentially the same as writing
    ///
    /// and is similar to `(0..i).map(f)`, just for `VVec<T>`s not iterators.
    ///
    /// If `length == 0`, this produces an empty `VVec<T>` without ever calling `f`.
    ///
    /// # Example
    ///
    ///
    /// The `VVec<T>` is generated in ascending index order, starting from the front
    /// and going towards the back, so you can use closures with mutable state:

    #[inline]

    pub fn from_fn<F>(length: usize, f: F) -> Self
    where
        F: FnMut(usize) -> T,
    {
        (0..length).map(f).collect()
    }

    /// Decomposes a `VVec<T>` into its raw components: `(pointer, length, capacity)`.
    ///
    /// Returns the raw pointer to the underlying data, the length of
    /// the vector (in elements), and the allocated capacity of the
    /// data (in elements). These are the same arguments in the same
    /// order as the arguments to [`from_raw_parts`].
    ///
    /// After calling this function, the caller is responsible for the
    /// memory previously managed by the `VVec`. Most often, one does
    /// this by converting the raw pointer, length, and capacity back
    /// into a `VVec` with the [`from_raw_parts`] function; more generally,
    /// if `T` is non-zero-sized and the capacity is nonzero, one may use
    /// any method that calls [`dealloc`] with a layout of
    /// `Layout::array::<T>(capacity)`; if `T` is zero-sized or the
    /// capacity is zero, nothing needs to be done.
    ///
    /// [`from_raw_parts`]: VVec::from_raw_parts
    /// [`dealloc`]: std::alloc::GlobalAlloc::dealloc
    ///
    /// # Examples
    ///
    #[must_use = "losing the pointer will leak memory"]

    pub fn into_raw_parts(self) -> (*mut T, usize, usize) {
        let mut me = ManuallyDrop::new(self);
        (me.as_mut_ptr(), me.len(), me.capacity())
    }

    #[doc(alias = "into_non_null_parts")]
    /// Decomposes a `VVec<T>` into its raw components: `(NonNull pointer, length, capacity)`.
    ///
    /// Returns the `NonNull` pointer to the underlying data, the length of
    /// the vector (in elements), and the allocated capacity of the
    /// data (in elements). These are the same arguments in the same
    /// order as the arguments to [`from_parts`].
    ///
    /// After calling this function, the caller is responsible for the
    /// memory previously managed by the `VVec`. The only way to do
    /// this is to convert the `NonNull` pointer, length, and capacity back
    /// into a `VVec` with the [`from_parts`] function, allowing
    /// the destructor to perform the cleanup.
    ///
    /// [`from_parts`]: VVec::from_parts
    ///
    /// # Examples
    ///
    #[must_use = "losing the pointer will leak memory"]

    pub fn into_parts(self) -> (NonNull<T>, usize, usize) {
        let (ptr, len, capacity) = self.into_raw_parts();
        // SAFETY: A `VVec` always has a non-null pointer.
        (unsafe { NonNull::new_unchecked(ptr) }, len, capacity)
    }

    /// Interns the `VVec<T>`, making the underlying memory read-only. This method should be
    /// called during compile time. (This is a no-op if called during runtime)
    ///
    /// This method must be called if the memory used by `VVec` needs to appear in the final
    /// values of constants.

    // Adapted (not const): the upstream method is `const fn` and uses the
    // `core::intrinsics::const_make_global` intrinsic to intern the heap allocation into a
    // constant's final value at compile time. That intrinsic is not stable-callable on this
    // toolchain, and the `VRawVec`-over-`std::Vec` shim is not const anyway (its `std::Vec`
    // operations are not const). As the upstream docs note, `const_make_global` is a no-op
    // at runtime, so the runtime behavior — leaking the buffer and returning a `'static`
    // slice — is preserved; only compile-time interning is lost.
    pub fn const_make_global(self) -> &'static [T]
    where
        T: Freeze,
    {
        // The buffer pointer is only valid to leak when `self.capacity()` is nonzero and `T`
        // is not a ZST; otherwise return a dangling slice of the right length.
        if self.capacity() == 0 || T::IS_ZST {
            let me = ManuallyDrop::new(self);
            unsafe { slice::from_raw_parts(NonNull::<T>::dangling().as_ptr(), me.len) }
        } else {
            let me = ManuallyDrop::new(self);
            unsafe { slice::from_raw_parts(me.as_ptr(), me.len) }
        }
    }
}

// Adapted (not a const impl): upstream this is
// `const impl<T, A: [const] Allocator + [const] Destruct> VVec<T, A>`, a const-trait impl
// giving const-evaluable `with_capacity_in` / `push` / `push_mut`. The const-trait surface
// (`const impl`, `[const]` bounds, `Destruct`) is not supported on this toolchain, and the
// `VRawVec`-over-`std::Vec` shim is not const regardless, so this is a regular impl. The
// methods are unchanged; only their const-evaluability is lost.
impl<T, A: Allocator> VVec<T, A> {
    /// Constructs a new, empty `VVec<T, A>` with at least the specified capacity
    /// with the provided allocator.
    ///
    /// The vector will be able to hold at least `capacity` elements without
    /// reallocating. This method is allowed to allocate for more elements than
    /// `capacity`. If `capacity` is zero, the vector will not allocate.
    ///
    /// It is important to note that although the returned vector has the
    /// minimum *capacity* specified, the vector will have a zero *length*. For
    /// an explanation of the difference between length and capacity, see
    /// *[Capacity and reallocation]*.
    ///
    /// If it is important to know the exact allocated capacity of a `VVec`,
    /// always use the [`capacity`] method after construction.
    ///
    /// For `VVec<T, A>` where `T` is a zero-sized type, there will be no allocation
    /// and the capacity will always be `usize::MAX`.
    ///
    /// [Capacity and reallocation]: #capacity-and-reallocation
    /// [`capacity`]: VVec::capacity
    ///
    /// # Panics
    ///
    /// Panics if the new capacity exceeds `isize::MAX` _bytes_.
    ///
    /// # Examples
    ///
    #[inline]

    pub fn with_capacity_in(capacity: usize, alloc: A) -> Self {
        VVec { buf: VRawVec::with_capacity_in(capacity, alloc), len: 0, pending: None }
    }

    /// Appends an element to the back of a collection.
    ///
    /// # Panics
    ///
    /// Panics if the new capacity exceeds `isize::MAX` _bytes_.
    ///
    /// # Examples
    ///
    ///
    /// # Time complexity
    ///
    /// Takes amortized *O*(1) time. If the vector's length would exceed its
    /// capacity after the push, *O*(*capacity*) time is taken to copy the
    /// vector's elements to a larger allocation. This expensive operation is
    /// offset by the *capacity* *O*(1) insertions it allows.
    #[inline]

    pub fn push(&mut self, value: T) {
        let _ = self.push_mut(value);
    }

    /// Appends an element to the back of a collection, returning a reference to it.
    ///
    /// # Panics
    ///
    /// Panics if the new capacity exceeds `isize::MAX` _bytes_.
    ///
    /// # Examples
    ///
    ///
    /// # Time complexity
    ///
    /// Takes amortized *O*(1) time. If the vector's length would exceed its
    /// capacity after the push, *O*(*capacity*) time is taken to copy the
    /// vector's elements to a larger allocation. This expensive operation is
    /// offset by the *capacity* *O*(1) insertions it allows.
    #[inline]

    #[must_use = "if you don't need a reference to the value, use `VVec::push` instead"]
    pub fn push_mut(&mut self, value: T) -> &mut T {
        self.restore_wf_wo_data_loss(); // lazy_loss_recovery: finish a forgotten drain before mutating
        // Inform codegen that the length does not change across grow_one().
        let len = self.len;
        // This will panic or abort if we would allocate > isize::MAX bytes
        // or if the length increment would overflow for zero-sized types.
        if len == self.buf.capacity() {
            self.buf.grow_one();
        }
        unsafe {
            let end = self.as_mut_ptr().add(len);
            ptr::write(end, value);
            self.len = len + 1;
            // SAFETY: We just wrote a value to the pointer that will live the lifetime of the reference.
            &mut *end
        }
    }
}

impl<T, A: Allocator> VVec<T, A> {
    /// Constructs a new, empty `VVec<T, A>`.
    ///
    /// The vector will not allocate until elements are pushed onto it.
    ///
    /// # Examples
    ///
    #[inline]

    pub const fn new_in(alloc: A) -> Self {
        VVec { buf: VRawVec::new_in(alloc), len: 0, pending: None }
    }

    /// Constructs a new, empty `VVec<T, A>` with at least the specified capacity
    /// with the provided allocator.
    ///
    /// The vector will be able to hold at least `capacity` elements without
    /// reallocating. This method is allowed to allocate for more elements than
    /// `capacity`. If `capacity` is zero, the vector will not allocate.
    ///
    /// # Errors
    ///
    /// Returns an error if the capacity exceeds `isize::MAX` _bytes_,
    /// or if the allocator reports allocation failure.
    #[inline]

    //
    pub fn try_with_capacity_in(capacity: usize, alloc: A) -> Result<Self, TryReserveError> {
        Ok(VVec { buf: VRawVec::try_with_capacity_in(capacity, alloc)?, len: 0, pending: None })
    }

    /// Creates a `VVec<T, A>` directly from a pointer, a length, a capacity,
    /// and an allocator.
    ///
    /// # Safety
    ///
    /// This is highly unsafe, due to the number of invariants that aren't
    /// checked:
    ///
    /// * `ptr` must be [*currently allocated*] via the given allocator `alloc`.
    /// * `T` needs to have the same alignment as what `ptr` was allocated with.
    ///   (`T` having a less strict alignment is not sufficient, the alignment really
    ///   needs to be equal to satisfy the [`dealloc`] requirement that memory must be
    ///   allocated and deallocated with the same layout.)
    /// * The size of `T` times the `capacity` (i.e. the allocated size in bytes) needs
    ///   to be the same size as the pointer was allocated with. (Because similar to
    ///   alignment, [`dealloc`] must be called with the same layout `size`.)
    /// * `length` needs to be less than or equal to `capacity`.
    /// * The first `length` values must be properly initialized values of type `T`.
    /// * `capacity` needs to [*fit*] the layout size that the pointer was allocated with.
    /// * The allocated size in bytes must be no larger than `isize::MAX`.
    ///   See the safety documentation of [`pointer::offset`].
    ///
    /// These requirements are always upheld by any `ptr` that has been allocated
    /// via `VVec<T, A>`. Other allocation sources are allowed if the invariants are
    /// upheld.
    ///
    /// Violating these may cause problems like corrupting the allocator's
    /// internal data structures. For example it is **not** safe
    /// to build a `VVec<u8>` from a pointer to a C `char` array with length `size_t`.
    /// It's also not safe to build one from a `VVec<u16>` and its length, because
    /// the allocator cares about the alignment, and these two types have different
    /// alignments. The buffer was allocated with alignment 2 (for `u16`), but after
    /// turning it into a `VVec<u8>` it'll be deallocated with alignment 1.
    ///
    /// The ownership of `ptr` is effectively transferred to the
    /// `VVec<T>` which may then deallocate, reallocate or change the
    /// contents of memory pointed to by the pointer at will. Ensure
    /// that nothing else uses the pointer after calling this
    /// function.
    ///
    /// [`String`]: std::string::String
    /// [`dealloc`]: std::alloc::GlobalAlloc::dealloc
    /// [*currently allocated*]: std::alloc::Allocator#currently-allocated-memory
    /// [*fit*]: std::alloc::Allocator#memory-fitting
    ///
    /// # Examples
    ///
    ///
    /// Using memory that was allocated elsewhere:
    ///
    #[inline]

    pub unsafe fn from_raw_parts_in(
        ptr: *mut T,
        length: usize,
        capacity: usize,
        alloc: A,
    ) -> Self {
        // Adapted: upstream uses the internal `ub_checks::assert_unsafe_precondition!`
        // (which expands to a compiler-internal attribute); `debug_assert!` checks the same
        // safety precondition over the public surface.
        debug_assert!(
            length <= capacity,
            "VVec::from_raw_parts_in requires that length <= capacity"
        );
        unsafe { VVec { buf: VRawVec::from_raw_parts_in(ptr, capacity, alloc), len: length, pending: None } }
    }

    #[doc(alias = "from_non_null_parts_in")]
    /// Creates a `VVec<T, A>` directly from a `NonNull` pointer, a length, a capacity,
    /// and an allocator.
    ///
    /// # Safety
    ///
    /// This is highly unsafe, due to the number of invariants that aren't
    /// checked:
    ///
    /// * `ptr` must be [*currently allocated*] via the given allocator `alloc`.
    /// * `T` needs to have the same alignment as what `ptr` was allocated with.
    ///   (`T` having a less strict alignment is not sufficient, the alignment really
    ///   needs to be equal to satisfy the [`dealloc`] requirement that memory must be
    ///   allocated and deallocated with the same layout.)
    /// * The size of `T` times the `capacity` (i.e. the allocated size in bytes) needs
    ///   to be the same size as the pointer was allocated with. (Because similar to
    ///   alignment, [`dealloc`] must be called with the same layout `size`.)
    /// * `length` needs to be less than or equal to `capacity`.
    /// * The first `length` values must be properly initialized values of type `T`.
    /// * `capacity` needs to [*fit*] the layout size that the pointer was allocated with.
    /// * The allocated size in bytes must be no larger than `isize::MAX`.
    ///   See the safety documentation of [`pointer::offset`].
    ///
    /// These requirements are always upheld by any `ptr` that has been allocated
    /// via `VVec<T, A>`. Other allocation sources are allowed if the invariants are
    /// upheld.
    ///
    /// Violating these may cause problems like corrupting the allocator's
    /// internal data structures. For example it is **not** safe
    /// to build a `VVec<u8>` from a pointer to a C `char` array with length `size_t`.
    /// It's also not safe to build one from a `VVec<u16>` and its length, because
    /// the allocator cares about the alignment, and these two types have different
    /// alignments. The buffer was allocated with alignment 2 (for `u16`), but after
    /// turning it into a `VVec<u8>` it'll be deallocated with alignment 1.
    ///
    /// The ownership of `ptr` is effectively transferred to the
    /// `VVec<T>` which may then deallocate, reallocate or change the
    /// contents of memory pointed to by the pointer at will. Ensure
    /// that nothing else uses the pointer after calling this
    /// function.
    ///
    /// [`String`]: std::string::String
    /// [`dealloc`]: std::alloc::GlobalAlloc::dealloc
    /// [*currently allocated*]: std::alloc::Allocator#currently-allocated-memory
    /// [*fit*]: std::alloc::Allocator#memory-fitting
    ///
    /// # Examples
    ///
    ///
    /// Using memory that was allocated elsewhere:
    ///
    #[inline]

    //
    pub unsafe fn from_parts_in(
        ptr: NonNull<T>,
        length: usize,
        capacity: usize,
        alloc: A,
    ) -> Self {
        // Adapted: see `from_raw_parts_in` — `debug_assert!` replaces the internal
        // `ub_checks::assert_unsafe_precondition!`.
        debug_assert!(
            length <= capacity,
            "VVec::from_parts_in requires that length <= capacity"
        );
        unsafe { VVec { buf: VRawVec::from_nonnull_in(ptr, capacity, alloc), len: length, pending: None } }
    }

    /// Decomposes a `VVec<T>` into its raw components: `(pointer, length, capacity, allocator)`.
    ///
    /// Returns the raw pointer to the underlying data, the length of the vector (in elements),
    /// the allocated capacity of the data (in elements), and the allocator. These are the same
    /// arguments in the same order as the arguments to [`from_raw_parts_in`].
    ///
    /// After calling this function, the caller is responsible for the
    /// memory previously managed by the `VVec`. The only way to do
    /// this is to convert the raw pointer, length, and capacity back
    /// into a `VVec` with the [`from_raw_parts_in`] function, allowing
    /// the destructor to perform the cleanup.
    ///
    /// [`from_raw_parts_in`]: VVec::from_raw_parts_in
    ///
    /// # Examples
    ///
    #[must_use = "losing the pointer will leak memory"]

    pub fn into_raw_parts_with_alloc(self) -> (*mut T, usize, usize, A) {
        let mut me = ManuallyDrop::new(self);
        let len = me.len();
        let capacity = me.capacity();
        let ptr = me.as_mut_ptr();
        let alloc = unsafe { ptr::read(me.allocator()) };
        (ptr, len, capacity, alloc)
    }

    #[doc(alias = "into_non_null_parts_with_alloc")]
    /// Decomposes a `VVec<T>` into its raw components: `(NonNull pointer, length, capacity, allocator)`.
    ///
    /// Returns the `NonNull` pointer to the underlying data, the length of the vector (in elements),
    /// the allocated capacity of the data (in elements), and the allocator. These are the same
    /// arguments in the same order as the arguments to [`from_parts_in`].
    ///
    /// After calling this function, the caller is responsible for the
    /// memory previously managed by the `VVec`. The only way to do
    /// this is to convert the `NonNull` pointer, length, and capacity back
    /// into a `VVec` with the [`from_parts_in`] function, allowing
    /// the destructor to perform the cleanup.
    ///
    /// [`from_parts_in`]: VVec::from_parts_in
    ///
    /// # Examples
    ///
    #[must_use = "losing the pointer will leak memory"]

    //
    pub fn into_parts_with_alloc(self) -> (NonNull<T>, usize, usize, A) {
        let (ptr, len, capacity, alloc) = self.into_raw_parts_with_alloc();
        // SAFETY: A `VVec` always has a non-null pointer.
        (unsafe { NonNull::new_unchecked(ptr) }, len, capacity, alloc)
    }

    /// Returns the total number of elements the vector can hold without
    /// reallocating.
    ///
    /// # Examples
    ///
    ///
    /// A vector with zero-sized elements will always have a capacity of usize::MAX:
    ///
    #[inline]

    pub fn capacity(&self) -> usize {
        self.buf.capacity()
    }

    /// Reserves capacity for at least `additional` more elements to be inserted
    /// in the given `VVec<T>`. The collection may reserve more space to
    /// speculatively avoid frequent reallocations. After calling `reserve`,
    /// capacity will be greater than or equal to `self.len() + additional`.
    /// Does nothing if capacity is already sufficient.
    ///
    /// # Panics
    ///
    /// Panics if the new capacity exceeds `isize::MAX` _bytes_.
    ///
    /// # Examples
    ///

    pub fn reserve(&mut self, additional: usize) {
        self.buf.reserve(self.len, additional);
    }

    /// Reserves the minimum capacity for at least `additional` more elements to
    /// be inserted in the given `VVec<T>`. Unlike [`reserve`], this will not
    /// deliberately over-allocate to speculatively avoid frequent allocations.
    /// After calling `reserve_exact`, capacity will be greater than or equal to
    /// `self.len() + additional`. Does nothing if the capacity is already
    /// sufficient.
    ///
    /// Note that the allocator may give the collection more space than it
    /// requests. Therefore, capacity can not be relied upon to be precisely
    /// minimal. Prefer [`reserve`] if future insertions are expected.
    ///
    /// [`reserve`]: VVec::reserve
    ///
    /// # Panics
    ///
    /// Panics if the new capacity exceeds `isize::MAX` _bytes_.
    ///
    /// # Examples
    ///

    pub fn reserve_exact(&mut self, additional: usize) {
        self.buf.reserve_exact(self.len, additional);
    }

    /// Tries to reserve capacity for at least `additional` more elements to be inserted
    /// in the given `VVec<T>`. The collection may reserve more space to speculatively avoid
    /// frequent reallocations. After calling `try_reserve`, capacity will be
    /// greater than or equal to `self.len() + additional` if it returns
    /// `Ok(())`. Does nothing if capacity is already sufficient. This method
    /// preserves the contents even if an error occurs.
    ///
    /// # Errors
    ///
    /// If the capacity overflows, or the allocator reports a failure, then an error
    /// is returned.
    ///
    /// # Examples
    ///

    pub fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.buf.try_reserve(self.len, additional)
    }

    /// Tries to reserve the minimum capacity for at least `additional`
    /// elements to be inserted in the given `VVec<T>`. Unlike [`try_reserve`],
    /// this will not deliberately over-allocate to speculatively avoid frequent
    /// allocations. After calling `try_reserve_exact`, capacity will be greater
    /// than or equal to `self.len() + additional` if it returns `Ok(())`.
    /// Does nothing if the capacity is already sufficient.
    ///
    /// Note that the allocator may give the collection more space than it
    /// requests. Therefore, capacity can not be relied upon to be precisely
    /// minimal. Prefer [`try_reserve`] if future insertions are expected.
    ///
    /// [`try_reserve`]: VVec::try_reserve
    ///
    /// # Errors
    ///
    /// If the capacity overflows, or the allocator reports a failure, then an error
    /// is returned.
    ///
    /// # Examples
    ///

    pub fn try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.buf.try_reserve_exact(self.len, additional)
    }

    /// Shrinks the capacity of the vector as much as possible.
    ///
    /// The behavior of this method depends on the allocator, which may either shrink the vector
    /// in-place or reallocate. The resulting vector might still have some excess capacity, just as
    /// is the case for [`with_capacity`]. See [`Allocator::shrink`] for more details.
    ///
    /// [`with_capacity`]: VVec::with_capacity
    ///
    /// # Examples
    ///

    #[inline]
    pub fn shrink_to_fit(&mut self) {
        // The capacity is never less than the length, and there's nothing to do when
        // they are equal, so we can avoid the panic case in `VRawVec::shrink_to_fit`
        // by only calling it with a greater capacity.
        if self.capacity() > self.len {
            self.buf.shrink_to_fit(self.len);
        }
    }

    /// Shrinks the capacity of the vector with a lower bound.
    ///
    /// The capacity will remain at least as large as both the length
    /// and the supplied value.
    ///
    /// If the current capacity is less than the lower limit, this is a no-op.
    ///
    /// # Examples
    ///

    pub fn shrink_to(&mut self, min_capacity: usize) {
        if self.capacity() > min_capacity {
            self.buf.shrink_to_fit(cmp::max(self.len, min_capacity));
        }
    }

    /// Tries to shrink the capacity of the vector as much as possible
    ///
    /// The behavior of this method depends on the allocator, which may either shrink the vector
    /// in-place or reallocate. The resulting vector might still have some excess capacity, just as
    /// is the case for [`with_capacity`]. See [`Allocator::shrink`] for more details.
    ///
    /// [`with_capacity`]: VVec::with_capacity
    ///
    /// # Errors
    ///
    /// This function returns an error if the allocator fails to shrink the allocation,
    /// the vector thereafter is still safe to use, the capacity remains unchanged
    /// however. See [`Allocator::shrink`].
    ///
    /// # Examples
    ///

    #[inline]
    pub fn try_shrink_to_fit(&mut self) -> Result<(), TryReserveError> {
        if self.capacity() > self.len { self.buf.try_shrink_to_fit(self.len) } else { Ok(()) }
    }

    /// Shrinks the capacity of the vector with a lower bound.
    ///
    /// The capacity will remain at least as large as both the length
    /// and the supplied value.
    ///
    /// If the current capacity is less than the lower limit, this is a no-op.
    ///
    /// # Errors
    ///
    /// This function returns an error if the allocator fails to shrink the allocation,
    /// the vector thereafter is still safe to use, the capacity remains unchanged
    /// however. See [`Allocator::shrink`].
    ///
    /// # Examples
    ///

    #[inline]
    pub fn try_shrink_to(&mut self, min_capacity: usize) -> Result<(), TryReserveError> {
        if self.capacity() > min_capacity {
            self.buf.try_shrink_to_fit(cmp::max(self.len, min_capacity))
        } else {
            Ok(())
        }
    }

    /// Converts the vector into [`Box<[T]>`][owned slice].
    ///
    /// Before doing the conversion, this method discards excess capacity like [`shrink_to_fit`].
    ///
    /// [owned slice]: Box
    /// [`shrink_to_fit`]: VVec::shrink_to_fit
    ///
    /// # Examples
    ///
    ///
    /// Any excess capacity is removed:
    ///

    pub fn into_boxed_slice(mut self) -> Box<[T], A> {
        unsafe {
            self.shrink_to_fit();
            let me = ManuallyDrop::new(self);
            let buf = ptr::read(&me.buf);
            let len = me.len();
            buf.into_box(len).assume_init()
        }
    }

    /// Shortens the vector, keeping the first `len` elements and dropping
    /// the rest.
    ///
    /// If `len` is greater or equal to the vector's current length, this has
    /// no effect.
    ///
    /// The [`drain`] method can emulate `truncate`, but causes the excess
    /// elements to be returned instead of dropped.
    ///
    /// Note that this method has no effect on the allocated capacity
    /// of the vector.
    ///
    /// # Examples
    ///
    /// Truncating a five element vector to two elements:
    ///
    ///
    /// No truncation occurs when `len` is greater than the vector's current
    /// length:
    ///
    ///
    /// Truncating when `len == 0` is equivalent to calling the [`clear`]
    /// method.
    ///
    ///
    /// [`clear`]: VVec::clear
    /// [`drain`]: VVec::drain

    pub fn truncate(&mut self, len: usize) {
        self.restore_wf_wo_data_loss(); // lazy_loss_recovery: finish a forgotten drain before mutating
        // This is safe because:
        //
        // * the slice passed to `drop_in_place` is valid; the `len > self.len`
        //   case avoids creating an invalid slice, and
        // * the `len` of the vector is shrunk before calling `drop_in_place`,
        //   such that no value will be dropped twice in case `drop_in_place`
        //   were to panic once (if it panics twice, the program aborts).
        unsafe {
            // Note: It's intentional that this is `>` and not `>=`.
            //       Changing it to `>=` has negative performance
            //       implications in some cases. See #78884 for more.
            if len > self.len {
                return;
            }
            let remaining_len = self.len - len;
            let s = ptr::slice_from_raw_parts_mut(self.as_mut_ptr().add(len), remaining_len);
            self.len = len;
            ptr::drop_in_place(s);
        }
    }

    /// Extracts a slice containing the entire vector.
    ///
    /// Equivalent to `&s[..]`.
    ///
    /// # Examples
    ///
    #[inline]

    pub fn as_slice(&self) -> &[T] {
        // SAFETY: `slice::from_raw_parts` requires pointee is a contiguous, aligned buffer of size
        // `len` containing properly-initialized `T`s. Data must not be mutated for the returned
        // lifetime. Further, `len * size_of::<T>` <= `isize::MAX`, and allocation does not
        // "wrap" through overflowing memory addresses.
        //
        // * VVec API guarantees that self.buf:
        //      * contains only properly-initialized items within 0..len
        //      * is aligned, contiguous, and valid for `len` reads
        //      * obeys size and address-wrapping constraints
        //
        // * We only construct `&mut` references to `self.buf` through `&mut self` methods; borrow-
        //   check ensures that it is not possible to mutably alias `self.buf` within the
        //   returned lifetime.
        unsafe {
            // normally this would use `slice::from_raw_parts`, but it's
            // instantiated often enough that avoiding the UB check is worth it
            &*core::intrinsics::aggregate_raw_ptr::<*const [T], _, _>(self.as_ptr(), self.len)
        }
    }

    /// Extracts a mutable slice of the entire vector.
    ///
    /// Equivalent to `&mut s[..]`.
    ///
    /// # Examples
    ///
    #[inline]

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: `slice::from_raw_parts_mut` requires pointee is a contiguous, aligned buffer of
        // size `len` containing properly-initialized `T`s. Data must not be accessed through any
        // other pointer for the returned lifetime. Further, `len * size_of::<T>` <=
        // `isize::MAX` and allocation does not "wrap" through overflowing memory addresses.
        //
        // * VVec API guarantees that self.buf:
        //      * contains only properly-initialized items within 0..len
        //      * is aligned, contiguous, and valid for `len` reads
        //      * obeys size and address-wrapping constraints
        //
        // * We only construct references to `self.buf` through `&self` and `&mut self` methods;
        //   borrow-check ensures that it is not possible to construct a reference to `self.buf`
        //   within the returned lifetime.
        unsafe {
            // normally this would use `slice::from_raw_parts_mut`, but it's
            // instantiated often enough that avoiding the UB check is worth it
            &mut *core::intrinsics::aggregate_raw_ptr::<*mut [T], _, _>(self.as_mut_ptr(), self.len)
        }
    }

    /// Returns a raw pointer to the vector's buffer, or a dangling raw pointer
    /// valid for zero sized reads if the vector didn't allocate.
    ///
    /// The caller must ensure that the vector outlives the pointer this
    /// function returns, or else it will end up dangling.
    /// Modifying the vector may cause its buffer to be reallocated,
    /// which would also make any pointers to it invalid.
    ///
    /// The caller must also ensure that the memory the pointer (non-transitively) points to
    /// is never written to (except inside an `UnsafeCell`) using this pointer or any pointer
    /// derived from it. If you need to mutate the contents of the slice, use [`as_mut_ptr`].
    ///
    /// This method guarantees that for the purpose of the aliasing model, this method
    /// does not materialize a reference to the underlying slice, and thus the returned pointer
    /// will remain valid when mixed with other calls to [`as_ptr`], [`as_mut_ptr`],
    /// and [`as_non_null`].
    /// Note that calling other methods that materialize mutable references to the slice,
    /// or mutable references to specific elements you are planning on accessing through this pointer,
    /// as well as writing to those elements, may still invalidate this pointer.
    /// See the second example below for how this guarantee can be used.
    ///
    ///
    /// # Examples
    ///
    ///
    /// Due to the aliasing guarantee, the following code is legal:
    ///
    ///
    /// [`as_mut_ptr`]: VVec::as_mut_ptr
    /// [`as_ptr`]: VVec::as_ptr
    /// [`as_non_null`]: VVec::as_non_null

    #[inline]
    pub fn as_ptr(&self) -> *const T {
        // We shadow the slice method of the same name to avoid going through
        // `deref`, which creates an intermediate reference.
        self.buf.ptr()
    }

    /// Returns a raw mutable pointer to the vector's buffer, or a dangling
    /// raw pointer valid for zero sized reads if the vector didn't allocate.
    ///
    /// The caller must ensure that the vector outlives the pointer this
    /// function returns, or else it will end up dangling.
    /// Modifying the vector may cause its buffer to be reallocated,
    /// which would also make any pointers to it invalid.
    ///
    /// This method guarantees that for the purpose of the aliasing model, this method
    /// does not materialize a reference to the underlying slice, and thus the returned pointer
    /// will remain valid when mixed with other calls to [`as_ptr`], [`as_mut_ptr`],
    /// and [`as_non_null`].
    /// Note that calling other methods that materialize references to the slice,
    /// or references to specific elements you are planning on accessing through this pointer,
    /// may still invalidate this pointer.
    /// See the second example below for how this guarantee can be used.
    ///
    /// The method also guarantees that, as long as `T` is not zero-sized and the capacity is
    /// nonzero, the pointer may be passed into [`dealloc`] with a layout of
    /// `Layout::array::<T>(capacity)` in order to deallocate the backing memory. If this is done,
    /// be careful not to run the destructor of the `VVec`, as dropping it will result in
    /// double-frees. Wrapping the `VVec` in a [`ManuallyDrop`] is the typical way to achieve this.
    ///
    /// # Examples
    ///
    ///
    /// Due to the aliasing guarantee, the following code is legal:
    ///
    ///
    /// Deallocating a vector using [`Box`] (which uses [`dealloc`] internally):
    ///
    ///
    /// [`as_mut_ptr`]: VVec::as_mut_ptr
    /// [`as_ptr`]: VVec::as_ptr
    /// [`as_non_null`]: VVec::as_non_null
    /// [`dealloc`]: std::alloc::GlobalAlloc::dealloc
    /// [`ManuallyDrop`]: core::mem::ManuallyDrop

    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut T {
        // We shadow the slice method of the same name to avoid going through
        // `deref_mut`, which creates an intermediate reference.
        self.buf.ptr()
    }

    /// Returns a `NonNull` pointer to the vector's buffer, or a dangling
    /// `NonNull` pointer valid for zero sized reads if the vector didn't allocate.
    ///
    /// The caller must ensure that the vector outlives the pointer this
    /// function returns, or else it will end up dangling.
    /// Modifying the vector may cause its buffer to be reallocated,
    /// which would also make any pointers to it invalid.
    ///
    /// This method guarantees that for the purpose of the aliasing model, this method
    /// does not materialize a reference to the underlying slice, and thus the returned pointer
    /// will remain valid when mixed with other calls to [`as_ptr`], [`as_mut_ptr`],
    /// and [`as_non_null`].
    /// Note that calling other methods that materialize references to the slice,
    /// or references to specific elements you are planning on accessing through this pointer,
    /// may still invalidate this pointer.
    /// See the second example below for how this guarantee can be used.
    ///
    /// # Examples
    ///
    ///
    /// Due to the aliasing guarantee, the following code is legal:
    ///
    ///
    /// [`as_mut_ptr`]: VVec::as_mut_ptr
    /// [`as_ptr`]: VVec::as_ptr
    /// [`as_non_null`]: VVec::as_non_null

    #[inline]
    pub fn as_non_null(&mut self) -> NonNull<T> {
        self.buf.non_null()
    }

    /// Returns a reference to the underlying allocator.

    #[inline]
    pub fn allocator(&self) -> &A {
        self.buf.allocator()
    }

    /// Forces the length of the vector to `new_len`.
    ///
    /// This is a low-level operation that maintains none of the normal
    /// invariants of the type. Normally changing the length of a vector
    /// is done using one of the safe operations instead, such as
    /// [`truncate`], [`resize`], [`extend`], or [`clear`].
    ///
    /// [`truncate`]: VVec::truncate
    /// [`resize`]: VVec::resize
    /// [`extend`]: Extend::extend
    /// [`clear`]: VVec::clear
    ///
    /// # Safety
    ///
    /// - `new_len` must be less than or equal to [`capacity()`].
    /// - The elements at `old_len..new_len` must be initialized.
    ///
    /// [`capacity()`]: VVec::capacity
    ///
    /// # Examples
    ///
    /// See [`spare_capacity_mut()`] for an example with safe
    /// initialization of capacity elements and use of this method.
    ///
    /// `set_len()` can be useful for situations in which the vector
    /// is serving as a buffer for other code, particularly over FFI:
    ///
    ///
    /// While the following example is sound, there is a memory leak since
    /// the inner vectors were not freed prior to the `set_len` call:
    ///
    ///
    /// Normally, here, one would use [`clear`] instead to correctly drop
    /// the contents and thus not leak memory.
    ///
    /// [`spare_capacity_mut()`]: VVec::spare_capacity_mut
    #[inline]

    pub unsafe fn set_len(&mut self, new_len: usize) {
        // Adapted: see `from_raw_parts_in` — `debug_assert!` replaces the internal
        // `ub_checks::assert_unsafe_precondition!`.
        debug_assert!(
            new_len <= self.capacity(),
            "VVec::set_len requires that new_len <= capacity()"
        );

        self.len = new_len;
    }

    /// Removes an element from the vector and returns it.
    ///
    /// The removed element is replaced by the last element of the vector.
    ///
    /// This does not preserve ordering of the remaining elements, but is *O*(1).
    /// If you need to preserve the element order, use [`remove`] instead.
    ///
    /// [`remove`]: VVec::remove
    ///
    /// # Panics
    ///
    /// Panics if `index` is out of bounds.
    ///
    /// # Examples
    ///
    #[inline]

    pub fn swap_remove(&mut self, index: usize) -> T {
        self.restore_wf_wo_data_loss(); // lazy_loss_recovery: finish a forgotten drain before mutating
        #[cold]

        fn assert_failed(index: usize, len: usize) -> ! {
            panic!("swap_remove index (is {index}) should be < len (is {len})");
        }

        let len = self.len();
        if index >= len {
            assert_failed(index, len);
        }
        unsafe {
            // We replace self[index] with the last element. Note that if the
            // bounds check above succeeds there must be a last element (which
            // can be self[index] itself).
            let value = ptr::read(self.as_ptr().add(index));
            let base_ptr = self.as_mut_ptr();
            ptr::copy(base_ptr.add(len - 1), base_ptr.add(index), 1);
            self.set_len(len - 1);
            value
        }
    }

    /// Inserts an element at position `index` within the vector, shifting all
    /// elements after it to the right.
    ///
    /// # Panics
    ///
    /// Panics if `index > len`.
    ///
    /// # Examples
    ///
    ///
    /// # Time complexity
    ///
    /// Takes *O*([`VVec::len`]) time. All items after the insertion index must be
    /// shifted to the right. In the worst case, all elements are shifted when
    /// the insertion index is 0.

    #[track_caller]
    pub fn insert(&mut self, index: usize, element: T) {
        let _ = self.insert_mut(index, element);
    }

    /// Inserts an element at position `index` within the vector, shifting all
    /// elements after it to the right, and returning a reference to the new
    /// element.
    ///
    /// # Panics
    ///
    /// Panics if `index > len`.
    ///
    /// # Examples
    ///
    ///
    /// # Time complexity
    ///
    /// Takes *O*([`VVec::len`]) time. All items after the insertion index must be
    /// shifted to the right. In the worst case, all elements are shifted when
    /// the insertion index is 0.

    #[inline]

    #[track_caller]
    #[must_use = "if you don't need a reference to the value, use `VVec::insert` instead"]
    pub fn insert_mut(&mut self, index: usize, element: T) -> &mut T {
        self.restore_wf_wo_data_loss(); // lazy_loss_recovery: finish a forgotten drain before mutating
        #[cold]

        #[track_caller]

        fn assert_failed(index: usize, len: usize) -> ! {
            panic!("insertion index (is {index}) should be <= len (is {len})");
        }

        let len = self.len();
        if index > len {
            assert_failed(index, len);
        }

        // space for the new element
        if len == self.buf.capacity() {
            self.buf.grow_one();
        }

        unsafe {
            // infallible
            // The spot to put the new value
            let p = self.as_mut_ptr().add(index);
            {
                if index < len {
                    // Shift everything over to make space. (Duplicating the
                    // `index`th element into two consecutive places.)
                    ptr::copy(p, p.add(1), len - index);
                }
                // Write it in, overwriting the first copy of the `index`th
                // element.
                ptr::write(p, element);
            }
            self.set_len(len + 1);
            &mut *p
        }
    }

    /// Removes and returns the element at position `index` within the vector,
    /// shifting all elements after it to the left.
    ///
    /// Note: Because this shifts over the remaining elements, it has a
    /// worst-case performance of *O*(*n*). If you don't need the order of elements
    /// to be preserved, use [`swap_remove`] instead. If you'd like to remove
    /// elements from the beginning of the `VVec`, consider using
    /// [`VecDeque::pop_front`] instead.
    ///
    /// [`swap_remove`]: VVec::swap_remove
    /// [`VecDeque::pop_front`]: std::collections::VecDeque::pop_front
    ///
    /// # Panics
    ///
    /// Panics if `index` is out of bounds.
    ///
    /// # Examples
    ///

    #[track_caller]

    pub fn remove(&mut self, index: usize) -> T {
        self.restore_wf_wo_data_loss(); // lazy_loss_recovery: finish a forgotten drain before mutating
        #[cold]

        #[track_caller]

        fn assert_failed(index: usize, len: usize) -> ! {
            panic!("removal index (is {index}) should be < len (is {len})");
        }

        match self.try_remove(index) {
            Some(elem) => elem,
            None => assert_failed(index, self.len()),
        }
    }

    /// Remove and return the element at position `index` within the vector,
    /// shifting all elements after it to the left, or [`None`] if it does not
    /// exist.
    ///
    /// Note: Because this shifts over the remaining elements, it has a
    /// worst-case performance of *O*(*n*). If you'd like to remove
    /// elements from the beginning of the `VVec`, consider using
    /// [`VecDeque::pop_front`] instead.
    ///
    /// [`VecDeque::pop_front`]: std::collections::VecDeque::pop_front
    ///
    /// # Examples
    ///

    pub fn try_remove(&mut self, index: usize) -> Option<T> {
        let len = self.len();
        if index >= len {
            return None;
        }
        unsafe {
            // infallible
            let ret;
            {
                // the place we are taking from.
                let ptr = self.as_mut_ptr().add(index);
                // copy it out, unsafely having a copy of the value on
                // the stack and in the vector at the same time.
                ret = ptr::read(ptr);

                // Shift everything down to fill in that spot.
                ptr::copy(ptr.add(1), ptr, len - index - 1);
            }
            self.set_len(len - 1);
            Some(ret)
        }
    }

    /// Retains only the elements specified by the predicate.
    ///
    /// In other words, remove all elements `e` for which `f(&e)` returns `false`.
    /// This method operates in place, visiting each element exactly once in the
    /// original order, and preserves the order of the retained elements.
    ///
    /// # Examples
    ///
    ///
    /// Because the elements are visited exactly once in the original order,
    /// external state may be used to decide which elements to keep.
    ///

    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&T) -> bool,
    {
        self.retain_mut(|elem| f(elem));
    }

    /// Retains only the elements specified by the predicate, passing a mutable reference to it.
    ///
    /// In other words, remove all elements `e` such that `f(&mut e)` returns `false`.
    /// This method operates in place, visiting each element exactly once in the
    /// original order, and preserves the order of the retained elements.
    ///
    /// # Examples
    ///

    pub fn retain_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut T) -> bool,
    {
        let original_len = self.len();

        if original_len == 0 {
            // Empty case: explicit return allows better optimization, vs letting compiler infer it
            return;
        }

        // VVec: [Kept, Kept, Hole, Hole, Hole, Hole, Unchecked, Unchecked]
        //      |            ^- write                ^- read             |
        //      |<-              original_len                          ->|
        // Kept: Elements which predicate returns true on.
        // Hole: Moved or dropped element slot.
        // Unchecked: Unchecked valid elements.
        //
        // This drop guard will be invoked when predicate or `drop` of element panicked.
        // It shifts unchecked elements to cover holes and `set_len` to the correct length.
        // In cases when predicate and `drop` never panick, it will be optimized out.
        struct PanicGuard<'a, T, A: Allocator> {
            v: &'a mut VVec<T, A>,
            read: usize,
            write: usize,
            original_len: usize,
        }

        impl<T, A: Allocator> Drop for PanicGuard<'_, T, A> {
            #[cold]
            fn drop(&mut self) {
                let remaining = self.original_len - self.read;
                // SAFETY: Trailing unchecked items must be valid since we never touch them.
                unsafe {
                    ptr::copy(
                        self.v.as_ptr().add(self.read),
                        self.v.as_mut_ptr().add(self.write),
                        remaining,
                    );
                }
                // SAFETY: After filling holes, all items are in contiguous memory.
                unsafe {
                    self.v.set_len(self.write + remaining);
                }
            }
        }

        let mut read = 0;
        loop {
            // SAFETY: read < original_len
            let cur = unsafe { self.get_unchecked_mut(read) };
            if hint::unlikely(!f(cur)) {
                break;
            }
            read += 1;
            if read == original_len {
                // All elements are kept, return early.
                return;
            }
        }

        // Critical section starts here and at least one element is going to be removed.
        // Advance `g.read` early to avoid double drop if `drop_in_place` panicked.
        let mut g = PanicGuard { v: self, read: read + 1, write: read, original_len };
        // SAFETY: previous `read` is always less than original_len.
        unsafe { ptr::drop_in_place(&mut *g.v.as_mut_ptr().add(read)) };

        while g.read < g.original_len {
            // SAFETY: `read` is always less than original_len.
            let cur = unsafe { &mut *g.v.as_mut_ptr().add(g.read) };
            if !f(cur) {
                // Advance `read` early to avoid double drop if `drop_in_place` panicked.
                g.read += 1;
                // SAFETY: We never touch this element again after dropped.
                unsafe { ptr::drop_in_place(cur) };
            } else {
                // SAFETY: `read` > `write`, so the slots don't overlap.
                // We use copy for move, and never touch the source element again.
                unsafe {
                    let hole = g.v.as_mut_ptr().add(g.write);
                    ptr::copy_nonoverlapping(cur, hole, 1);
                }
                g.write += 1;
                g.read += 1;
            }
        }

        // We are leaving the critical section and no panic happened,
        // Commit the length change and forget the guard.
        // SAFETY: `write` is always less than or equal to original_len.
        unsafe { g.v.set_len(g.write) };
        mem::forget(g);
    }

    /// Removes all but the first of consecutive elements in the vector that resolve to the same
    /// key.
    ///
    /// If the vector is sorted, this removes all duplicates.
    ///
    /// # Examples
    ///

    #[inline]
    pub fn dedup_by_key<F, K>(&mut self, mut key: F)
    where
        F: FnMut(&mut T) -> K,
        K: PartialEq,
    {
        self.dedup_by(|a, b| key(a) == key(b))
    }

    /// Removes all but the first of consecutive elements in the vector satisfying a given equality
    /// relation.
    ///
    /// The `same_bucket` function is passed references to two elements from the vector and
    /// must determine if the elements compare equal. The elements are passed in opposite order
    /// from their order in the slice, so if `same_bucket(a, b)` returns `true`, `a` is removed.
    ///
    /// If the vector is sorted, this removes all duplicates.
    ///
    /// # Examples
    ///

    pub fn dedup_by<F>(&mut self, mut same_bucket: F)
    where
        F: FnMut(&mut T, &mut T) -> bool,
    {
        let len = self.len();
        if len <= 1 {
            return;
        }

        // Check if we ever want to remove anything.
        // This allows to use copy_non_overlapping in next cycle.
        // And avoids any memory writes if we don't need to remove anything.
        let mut first_duplicate_idx: usize = 1;
        let start = self.as_mut_ptr();
        while first_duplicate_idx != len {
            let found_duplicate = unsafe {
                // SAFETY: first_duplicate always in range [1..len)
                // Note that we start iteration from 1 so we never overflow.
                let prev = start.add(first_duplicate_idx.wrapping_sub(1));
                let current = start.add(first_duplicate_idx);
                // We explicitly say in docs that references are reversed.
                same_bucket(&mut *current, &mut *prev)
            };
            if found_duplicate {
                break;
            }
            first_duplicate_idx += 1;
        }
        // Don't need to remove anything.
        // We cannot get bigger than len.
        if first_duplicate_idx == len {
            return;
        }

        /* INVARIANT: vec.len() > read > write > write-1 >= 0 */
        struct FillGapOnDrop<'a, T, A: core::alloc::Allocator> {
            /* Offset of the element we want to check if it is duplicate */
            read: usize,

            /* Offset of the place where we want to place the non-duplicate
             * when we find it. */
            write: usize,

            /* The VVec that would need correction if `same_bucket` panicked */
            vec: &'a mut VVec<T, A>,
        }

        impl<'a, T, A: core::alloc::Allocator> Drop for FillGapOnDrop<'a, T, A> {
            fn drop(&mut self) {
                /* This code gets executed when `same_bucket` panics */

                /* SAFETY: invariant guarantees that `read - write`
                 * and `len - read` never overflow and that the copy is always
                 * in-bounds. */
                unsafe {
                    let ptr = self.vec.as_mut_ptr();
                    let len = self.vec.len();

                    /* How many items were left when `same_bucket` panicked.
                     * Basically vec[read..].len() */
                    let items_left = len.wrapping_sub(self.read);

                    /* Pointer to first item in vec[write..write+items_left] slice */
                    let dropped_ptr = ptr.add(self.write);
                    /* Pointer to first item in vec[read..] slice */
                    let valid_ptr = ptr.add(self.read);

                    /* Copy `vec[read..]` to `vec[write..write+items_left]`.
                     * The slices can overlap, so `copy_nonoverlapping` cannot be used */
                    ptr::copy(valid_ptr, dropped_ptr, items_left);

                    /* How many items have been already dropped
                     * Basically vec[read..write].len() */
                    let dropped = self.read.wrapping_sub(self.write);

                    self.vec.set_len(len - dropped);
                }
            }
        }

        /* Drop items while going through VVec, it should be more efficient than
         * doing slice partition_dedup + truncate */

        // Construct gap first and then drop item to avoid memory corruption if `T::drop` panics.
        let mut gap =
            FillGapOnDrop { read: first_duplicate_idx + 1, write: first_duplicate_idx, vec: self };
        unsafe {
            // SAFETY: we checked that first_duplicate_idx in bounds before.
            // If drop panics, `gap` would remove this item without drop.
            ptr::drop_in_place(start.add(first_duplicate_idx));
        }

        /* SAFETY: Because of the invariant, read_ptr, prev_ptr and write_ptr
         * are always in-bounds and read_ptr never aliases prev_ptr */
        unsafe {
            while gap.read < len {
                let read_ptr = start.add(gap.read);
                let prev_ptr = start.add(gap.write.wrapping_sub(1));

                // We explicitly say in docs that references are reversed.
                let found_duplicate = same_bucket(&mut *read_ptr, &mut *prev_ptr);
                if found_duplicate {
                    // Increase `gap.read` now since the drop may panic.
                    gap.read += 1;
                    /* We have found duplicate, drop it in-place */
                    ptr::drop_in_place(read_ptr);
                } else {
                    let write_ptr = start.add(gap.write);

                    /* read_ptr cannot be equal to write_ptr because at this point
                     * we guaranteed to skip at least one element (before loop starts).
                     */
                    ptr::copy_nonoverlapping(read_ptr, write_ptr, 1);

                    /* We have filled that place, so go further */
                    gap.write += 1;
                    gap.read += 1;
                }
            }

            /* Technically we could let `gap` clean up with its Drop, but
             * when `same_bucket` is guaranteed to not panic, this bloats a little
             * the codegen, so we just do it manually */
            gap.vec.set_len(gap.write);
            mem::forget(gap);
        }
    }

    /// Appends an element and returns a reference to it if there is sufficient spare capacity,
    /// otherwise an error is returned with the element.
    ///
    /// Unlike [`push`] this method will not reallocate when there's insufficient capacity.
    /// The caller should use [`reserve`] or [`try_reserve`] to ensure that there is enough capacity.
    ///
    /// [`push`]: VVec::push
    /// [`reserve`]: VVec::reserve
    /// [`try_reserve`]: VVec::try_reserve
    ///
    /// # Examples
    ///
    /// A manual, panic-free alternative to [`FromIterator`]:
    ///
    ///
    /// # Time complexity
    ///
    /// Takes *O*(1) time.
    #[inline]

    pub fn push_within_capacity(&mut self, value: T) -> Result<&mut T, T> {
        if self.len == self.buf.capacity() {
            return Err(value);
        }

        unsafe {
            let end = self.as_mut_ptr().add(self.len);
            ptr::write(end, value);
            self.len += 1;

            // SAFETY: We just wrote a value to the pointer that will live the lifetime of the reference.
            Ok(&mut *end)
        }
    }

    /// Removes the last element from a vector and returns it, or [`None`] if it
    /// is empty.
    ///
    /// If you'd like to pop the first element, consider using
    /// [`VecDeque::pop_front`] instead.
    ///
    /// [`VecDeque::pop_front`]: std::collections::VecDeque::pop_front
    ///
    /// # Examples
    ///
    ///
    /// # Time complexity
    ///
    /// Takes *O*(1) time.
    #[inline]

    pub fn pop(&mut self) -> Option<T> {
        self.restore_wf_wo_data_loss(); // lazy_loss_recovery: finish a forgotten drain before mutating
        if self.len == 0 {
            None
        } else {
            unsafe {
                self.len -= 1;
                core::hint::assert_unchecked(self.len < self.capacity());
                Some(ptr::read(self.as_ptr().add(self.len())))
            }
        }
    }

    /// Removes and returns the last element from a vector if the predicate
    /// returns `true`, or [`None`] if the predicate returns false or the vector
    /// is empty (the predicate will not be called in that case).
    ///
    /// # Examples
    ///

    pub fn pop_if(&mut self, predicate: impl FnOnce(&mut T) -> bool) -> Option<T> {
        let last = self.last_mut()?;
        if predicate(last) { self.pop() } else { None }
    }

    /// Returns a mutable reference to the last item in the vector, or
    /// `None` if it is empty.
    ///
    /// # Examples
    ///
    /// Basic usage:
    ///
    #[inline]

    pub fn peek_mut(&mut self) -> Option<VVecPeekMut<'_, T, A>> {
        VVecPeekMut::new(self)
    }

    /// Moves all the elements of `other` into `self`, leaving `other` empty.
    ///
    /// # Panics
    ///
    /// Panics if the new capacity exceeds `isize::MAX` _bytes_.
    ///
    /// # Examples
    ///

    #[inline]

    pub fn append(&mut self, other: &mut Self) {
        unsafe {
            self.append_elements(other.as_slice() as _);
            other.set_len(0);
        }
    }

    /// Appends elements to `self` from other buffer.

    #[inline]
    unsafe fn append_elements(&mut self, other: *const [T]) {
        let count = other.len();
        self.reserve(count);
        let len = self.len();
        if count > 0 {
            unsafe {
                ptr::copy_nonoverlapping(other as *const T, self.as_mut_ptr().add(len), count)
            };
        }
        self.len += count;
    }

    /// Removes the subslice indicated by the given range from the vector,
    /// returning a double-ended iterator over the removed subslice.
    ///
    /// If the iterator is dropped before being fully consumed,
    /// it drops the remaining removed elements.
    ///
    /// The returned iterator keeps a mutable borrow on the vector to optimize
    /// its implementation.
    ///
    /// # Panics
    ///
    /// Panics if the range has `start_bound > end_bound`, or, if the range is
    /// bounded on either end and past the length of the vector.
    ///
    /// # Leaking
    ///
    /// If the returned iterator goes out of scope without being dropped (due to
    /// [`mem::forget`], for example), the vector may have lost and leaked
    /// elements arbitrarily, including elements outside the range.
    ///
    /// # Examples
    ///

    pub fn drain<R>(&mut self, range: R) -> VVecDrain<'_, T, A>
    where
        R: RangeBounds<usize>,
    {
        // Memory safety
        //
        // When the VVecDrain is first created, it shortens the length of
        // the source vector to make sure no uninitialized or moved-from elements
        // are accessible at all if the VVecDrain's destructor never gets to run.
        //
        // VVecDrain will ptr::read out the values to remove.
        // When finished, remaining tail of the vec is copied back to cover
        // the hole, and the vector length is restored to the new length.
        //
        // lazy_loss_recovery: finish any forgotten drain/extract_if before starting a new one.
        self.restore_wf_wo_data_loss();
        let len = self.len();
        let Range { start, end } = slice::range(range, ..len);

        unsafe {
            // set self.vec length's to start, to be safe in case VVecDrain is leaked
            self.set_len(start);
            // lazy_loss_recovery: record the finish work so a forgotten VVecDrain is completed by the
            // next op instead of leaking. tail_start=end, tail_len=len-end (constant); the un-yielded
            // range starts at `start` with length `end-start` (mirrored by the iterator).
            self.set_pending_drain(end, len - end, start, end - start);
            let range_slice = slice::from_raw_parts(self.as_ptr().add(start), end - start);
            VVecDrain {
                tail_start: end,
                tail_len: len - end,
                iter: range_slice.iter(),
                vec: NonNull::from(self),
            }
        }
    }

    // lazy_loss_recovery forget-safety. Called by `drain()` (register path) to record the finish
    // work; the iterator mirrors its progress into it.
    pub(super) fn set_pending_drain(
        &mut self,
        tail_start: usize,
        tail_len: usize,
        drop_offset: usize,
        drop_len: usize,
    ) {
        self.pending = Some(Box::new(Pending::Drain { tail_start, tail_len, drop_offset, drop_len }));
    }

    pub(super) fn set_pending_extract_if(&mut self, old_len: usize, idx: usize, del: usize) {
        self.pending = Some(Box::new(Pending::ExtractIf { old_len, idx, del }));
    }

    // lazy_loss_recovery forget-safety guard. If a `drain()` iterator was forgotten, its finish work
    // is still recorded in `pending`; run it now so the vec is in its expected post-removal state
    // before this op proceeds. No-op (a single predictable branch) when nothing is pending.
    #[inline]
    fn restore_wf_wo_data_loss(&mut self) {
        if self.pending.is_some() {
            self.restore_wf_wo_data_loss_cold();
        }
    }

    #[cold]
    #[inline(never)]
    fn restore_wf_wo_data_loss_cold(&mut self) {
        // SAFETY: `pending` was set by `drain()` with `len` parked and the buffer otherwise intact;
        // the variant describes exactly the un-finished removal.
        match *self.pending.take().unwrap() {
            Pending::Drain { tail_start, tail_len, drop_offset, drop_len } => unsafe {
                VVecDrain::finish_forgotten_drain(self, tail_start, tail_len, drop_offset, drop_len)
            },
            Pending::ExtractIf { old_len, idx, del } => unsafe {
                extract_if::finish_forgotten_extract_if(self, old_len, idx, del)
            },
        }
    }

    /// Clears the vector, removing all values.
    ///
    /// Note that this method has no effect on the allocated capacity
    /// of the vector.
    ///
    /// # Examples
    ///
    #[inline]

    pub fn clear(&mut self) {
        let elems: *mut [T] = self.as_mut_slice();

        // SAFETY:
        // - `elems` comes directly from `as_mut_slice` and is therefore valid.
        // - Setting `self.len` before calling `drop_in_place` means that,
        //   if an element's `Drop` impl panics, the vector's `Drop` impl will
        //   do nothing (leaking the rest of the elements) instead of dropping
        //   some twice.
        unsafe {
            self.len = 0;
            ptr::drop_in_place(elems);
        }
    }

    /// Returns the number of elements in the vector, also referred to
    /// as its 'length'.
    ///
    /// # Examples
    ///
    #[inline]

    // NOTE: not `const fn` (unlike std) — `pending` is boxed and `Box` deref is not const-stable.
    pub fn len(&self) -> usize {
        let len = self.len;

        // SAFETY: The maximum capacity of `VVec<T>` is `isize::MAX` bytes, so the maximum value can
        // be returned is `usize::checked_div(size_of::<T>()).unwrap_or(usize::MAX)`, which
        // matches the definition of `T::MAX_SLICE_LEN`.
        unsafe { intrinsics::assume(len <= T::MAX_SLICE_LEN) };

        // lazy_loss_recovery: while a forgotten drain is pending, `self.len` is parked at the head
        // length; report the post-drain length (head + tail) without mutating, so the vec looks
        // finished from the outside even before the next op runs `restore_wf_wo_data_loss`.
        match self.pending.as_deref() {
            Some(Pending::Drain { tail_len, .. }) => len + *tail_len,
            // extract_if parks `len` at 0; post-extraction length is old_len - del.
            Some(Pending::ExtractIf { old_len, del, .. }) => *old_len - *del,
            None => len,
        }
    }

    /// Returns `true` if the vector contains no elements.
    ///
    /// # Examples
    ///

    // NOTE: not `const fn` (unlike std) — calls the (now non-const, boxed-`pending`) `len()`.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Splits the collection into two at the given index.
    ///
    /// Returns a newly allocated vector containing the elements in the range
    /// `[at, len)`. After the call, the original vector will be left containing
    /// the elements `[0, at)` with its previous capacity unchanged.
    ///
    /// - If you want to take ownership of the entire contents and capacity of
    ///   the vector, see [`mem::take`] or [`mem::replace`].
    /// - If you don't need the returned vector at all, see [`VVec::truncate`].
    /// - If you want to take ownership of an arbitrary subslice, or you don't
    ///   necessarily want to store the removed items in a vector, see [`VVec::drain`].
    ///
    /// # Panics
    ///
    /// Panics if `at > len`.
    ///
    /// # Examples
    ///

    #[inline]
    #[must_use = "use `.truncate()` if you don't need the other half"]

    #[track_caller]
    pub fn split_off(&mut self, at: usize) -> Self
    where
        A: Clone,
    {
        #[cold]

        #[track_caller]

        fn assert_failed(at: usize, len: usize) -> ! {
            panic!("`at` split index (is {at}) should be <= len (is {len})");
        }

        if at > self.len() {
            assert_failed(at, self.len());
        }

        let other_len = self.len - at;
        let mut other = VVec::with_capacity_in(other_len, self.allocator().clone());

        // Unsafely `set_len` and copy items to `other`.
        unsafe {
            self.set_len(at);
            other.set_len(other_len);

            ptr::copy_nonoverlapping(self.as_ptr().add(at), other.as_mut_ptr(), other.len());
        }
        other
    }

    /// Resizes the `VVec` in-place so that `len` is equal to `new_len`.
    ///
    /// If `new_len` is greater than `len`, the `VVec` is extended by the
    /// difference, with each additional slot filled with the result of
    /// calling the closure `f`. The return values from `f` will end up
    /// in the `VVec` in the order they have been generated.
    ///
    /// If `new_len` is less than `len`, the `VVec` is simply truncated.
    ///
    /// This method uses a closure to create new values on every push. If
    /// you'd rather [`Clone`] a given value, use [`VVec::resize`]. If you
    /// want to use the [`Default`] trait to generate values, you can
    /// pass [`Default::default`] as the second argument.
    ///
    /// # Panics
    ///
    /// Panics if the new capacity exceeds `isize::MAX` _bytes_.
    ///
    /// # Examples
    ///

    pub fn resize_with<F>(&mut self, new_len: usize, f: F)
    where
        F: FnMut() -> T,
    {
        let len = self.len();
        if new_len > len {
            self.extend_trusted(iter::repeat_with(f).take(new_len - len));
        } else {
            self.truncate(new_len);
        }
    }

    /// Consumes and leaks the `VVec`, returning a mutable reference to the contents,
    /// `&'a mut [T]`.
    ///
    /// Note that the type `T` must outlive the chosen lifetime `'a`. If the type
    /// has only static references, or none at all, then this may be chosen to be
    /// `'static`.
    ///
    /// As of Rust 1.57, this method does not reallocate or shrink the `VVec`,
    /// so the leaked allocation may include unused capacity that is not part
    /// of the returned slice.
    ///
    /// This function is mainly useful for data that lives for the remainder of
    /// the program's life. Dropping the returned reference will cause a memory
    /// leak.
    ///
    /// # Examples
    ///
    /// Simple usage:
    ///

    #[inline]
    pub fn leak<'a>(self) -> &'a mut [T]
    where
        A: 'a,
    {
        let mut me = ManuallyDrop::new(self);
        unsafe { slice::from_raw_parts_mut(me.as_mut_ptr(), me.len) }
    }

    /// Returns the remaining spare capacity of the vector as a slice of
    /// `MaybeUninit<T>`.
    ///
    /// The returned slice can be used to fill the vector with data (e.g. by
    /// reading from a file) before marking the data as initialized using the
    /// [`set_len`] method.
    ///
    /// [`set_len`]: VVec::set_len
    ///
    /// # Examples
    ///

    #[inline]
    pub fn spare_capacity_mut(&mut self) -> &mut [MaybeUninit<T>] {
        // Note:
        // This method is not implemented in terms of `split_at_spare_mut`,
        // to prevent invalidation of pointers to the buffer.
        unsafe {
            slice::from_raw_parts_mut(
                self.as_mut_ptr().add(self.len) as *mut MaybeUninit<T>,
                self.buf.capacity() - self.len,
            )
        }
    }

    /// Returns vector content as a slice of `T`, along with the remaining spare
    /// capacity of the vector as a slice of `MaybeUninit<T>`.
    ///
    /// The returned spare capacity slice can be used to fill the vector with data
    /// (e.g. by reading from a file) before marking the data as initialized using
    /// the [`set_len`] method.
    ///
    /// [`set_len`]: VVec::set_len
    ///
    /// Note that this is a low-level API, which should be used with care for
    /// optimization purposes. If you need to append data to a `VVec`
    /// you can use [`push`], [`extend`], [`extend_from_slice`],
    /// [`extend_from_within`], [`insert`], [`append`], [`resize`] or
    /// [`resize_with`], depending on your exact needs.
    ///
    /// [`push`]: VVec::push
    /// [`extend`]: VVec::extend
    /// [`extend_from_slice`]: VVec::extend_from_slice
    /// [`extend_from_within`]: VVec::extend_from_within
    /// [`insert`]: VVec::insert
    /// [`append`]: VVec::append
    /// [`resize`]: VVec::resize
    /// [`resize_with`]: VVec::resize_with
    ///
    /// # Examples
    ///

    #[inline]
    pub fn split_at_spare_mut(&mut self) -> (&mut [T], &mut [MaybeUninit<T>]) {
        // SAFETY:
        // - len is ignored and so never changed
        let (init, spare, _) = unsafe { self.split_at_spare_mut_with_len() };
        (init, spare)
    }

    /// Safety: changing returned .2 (&mut usize) is considered the same as calling `.set_len(_)`.
    ///
    /// This method provides unique access to all vec parts at once in `extend_from_within`.
    unsafe fn split_at_spare_mut_with_len(
        &mut self,
    ) -> (&mut [T], &mut [MaybeUninit<T>], &mut usize) {
        let ptr = self.as_mut_ptr();
        // SAFETY:
        // - `ptr` is guaranteed to be valid for `self.len` elements
        // - but the allocation extends out to `self.buf.capacity()` elements, possibly
        // uninitialized
        let spare_ptr = unsafe { ptr.add(self.len) };
        let spare_ptr = spare_ptr.cast_uninit();
        let spare_len = self.buf.capacity() - self.len;

        // SAFETY:
        // - `ptr` is guaranteed to be valid for `self.len` elements
        // - `spare_ptr` is pointing one element past the buffer, so it doesn't overlap with `initialized`
        unsafe {
            let initialized = slice::from_raw_parts_mut(ptr, self.len);
            let spare = slice::from_raw_parts_mut(spare_ptr, spare_len);

            (initialized, spare, &mut self.len)
        }
    }

    /// Groups every `N` elements in the `VVec<T>` into chunks to produce a `VVec<[T; N]>`, dropping
    /// elements in the remainder. `N` must be greater than zero.
    ///
    /// If the capacity is not a multiple of the chunk size, the buffer will shrink down to the
    /// nearest multiple with a reallocation or deallocation.
    ///
    /// This function can be used to reverse [`VVec::into_flattened`].
    ///
    /// # Examples
    ///

    pub fn into_chunks<const N: usize>(mut self) -> VVec<[T; N], A> {
        const {
            assert!(N != 0, "chunk size must be greater than zero");
        }

        let (len, cap) = (self.len(), self.capacity());

        let len_remainder = len % N;
        if len_remainder != 0 {
            self.truncate(len - len_remainder);
        }

        let cap_remainder = cap % N;
        if !T::IS_ZST && cap_remainder != 0 {
            self.buf.shrink_to_fit(cap - cap_remainder);
        }

        let (ptr, _, _, alloc) = self.into_raw_parts_with_alloc();

        // SAFETY:
        // - `ptr` and `alloc` were just returned from `self.into_raw_parts_with_alloc()`
        // - `[T; N]` has the same alignment as `T`
        // - `size_of::<[T; N]>() * cap / N == size_of::<T>() * cap`
        // - `len / N <= cap / N` because `len <= cap`
        // - the allocated memory consists of `len / N` valid values of type `[T; N]`
        // - `cap / N` fits the size of the allocated memory after shrinking
        unsafe { VVec::from_raw_parts_in(ptr.cast(), len / N, cap / N, alloc) }
    }

    /// This clears out this `VVec` and recycles the allocation into a new `VVec`.
    /// The item type of the resulting `VVec` needs to have the same size and
    /// alignment as the item type of the original `VVec`.
    ///
    /// # Examples
    ///
    ///
    /// The `Recyclable` bound prevents this method from being called when `T` and `U` have different sizes; e.g.:
    ///
    /// ...or different alignments:
    ///
    ///
    /// However, due to temporary implementation limitations of `Recyclable`,
    /// this method is not yet callable when `T` or `U` are slices, trait objects,
    /// or other exotic types; e.g.:
    ///

    #[expect(private_bounds)]
    pub fn recycle<U>(mut self) -> VVec<U, A>
    where
        U: Recyclable<T>,
    {
        self.clear();
        const {
            // FIXME(const-hack, 146097): compare `Layout`s
            assert!(size_of::<T>() == size_of::<U>());
            assert!(align_of::<T>() == align_of::<U>());
        };
        let (ptr, length, capacity, alloc) = self.into_parts_with_alloc();
        debug_assert_eq!(length, 0);
        // SAFETY:
        // - `ptr` and `alloc` were just returned from `self.into_raw_parts_with_alloc()`
        // - `T` & `U` have the same layout, so `capacity` does not need to be changed and we can safely use `alloc.dealloc` later
        // - the original vector was cleared, so there is no problem with "transmuting" the stored values
        unsafe { VVec::from_parts_in(ptr.cast::<U>(), length, capacity, alloc) }
    }
}

/// Denotes that an allocation of `From` can be recycled into an allocation of `Self`.
///
/// # Safety
///
/// `Self` is `Recyclable<From>` if `Layout::new::<Self>() == Layout::new::<From>()`.
unsafe trait Recyclable<From: Sized>: Sized {}

// SAFETY: enforced by `TransmuteFrom`
unsafe impl<From, To> Recyclable<From> for To
where
    for<'a> &'a MaybeUninit<To>: TransmuteFrom<&'a MaybeUninit<From>, { Assume::SAFETY }>,
    for<'a> &'a MaybeUninit<From>: TransmuteFrom<&'a MaybeUninit<To>, { Assume::SAFETY }>,
{
}

impl<T: Clone, A: Allocator> VVec<T, A> {
    /// Resizes the `VVec` in-place so that `len` is equal to `new_len`.
    ///
    /// If `new_len` is greater than `len`, the `VVec` is extended by the
    /// difference, with each additional slot filled with `value`.
    /// If `new_len` is less than `len`, the `VVec` is simply truncated.
    ///
    /// This method requires `T` to implement [`Clone`],
    /// in order to be able to clone the passed value.
    /// If you need more flexibility (or want to rely on [`Default`] instead of
    /// [`Clone`]), use [`VVec::resize_with`].
    /// If you only need to resize to a smaller size, use [`VVec::truncate`].
    ///
    /// # Panics
    ///
    /// Panics if the new capacity exceeds `isize::MAX` _bytes_.
    ///
    /// # Examples
    ///

    pub fn resize(&mut self, new_len: usize, value: T) {
        let len = self.len();

        if new_len > len {
            self.extend_with(new_len - len, value)
        } else {
            self.truncate(new_len);
        }
    }

    /// Clones and appends all elements in a slice to the `VVec`.
    ///
    /// Iterates over the slice `other`, clones each element, and then appends
    /// it to this `VVec`. The `other` slice is traversed in-order.
    ///
    /// Note that this function is the same as [`extend`],
    /// except that it also works with slice elements that are Clone but not Copy.
    /// If Rust gets specialization this function may be deprecated.
    ///
    /// # Panics
    ///
    /// Panics if the new capacity exceeds `isize::MAX` _bytes_.
    ///
    /// # Examples
    ///
    ///
    /// [`extend`]: VVec::extend

    pub fn extend_from_slice(&mut self, other: &[T]) {
        self.spec_extend(other.iter())
    }

    /// Given a range `src`, clones a slice of elements in that range and appends it to the end.
    ///
    /// `src` must be a range that can form a valid subslice of the `VVec`.
    ///
    /// # Panics
    ///
    /// Panics if starting index is greater than the end index, if the index is
    /// greater than the length of the vector, or if the new capacity exceeds
    /// `isize::MAX` _bytes_.
    ///
    /// # Examples
    ///

    pub fn extend_from_within<R>(&mut self, src: R)
    where
        R: RangeBounds<usize>,
    {
        let range = slice::range(src, ..self.len());
        self.reserve(range.len());

        // SAFETY:
        // - `slice::range` guarantees that the given range is valid for indexing self
        unsafe {
            self.spec_extend_from_within(range);
        }
    }
}

impl<T, A: Allocator, const N: usize> VVec<[T; N], A> {
    /// Takes a `VVec<[T; N]>` and flattens it into a `VVec<T>`.
    ///
    /// # Panics
    ///
    /// Panics if the length of the resulting vector would overflow a `usize`.
    ///
    /// This is only possible when flattening a vector of arrays of zero-sized
    /// types, and thus tends to be irrelevant in practice. If
    /// `size_of::<T>() > 0`, this will never panic.
    ///
    /// # Examples
    ///

    pub fn into_flattened(self) -> VVec<T, A> {
        let (ptr, len, cap, alloc) = self.into_raw_parts_with_alloc();
        let (new_len, new_cap) = if T::IS_ZST {
            (len.checked_mul(N).expect("vec len overflow"), usize::MAX)
        } else {
            // SAFETY:
            // - `cap * N` cannot overflow because the allocation is already in
            // the address space.
            // - Each `[T; N]` has `N` valid elements, so there are `len * N`
            // valid elements in the allocation.
            unsafe { (len.unchecked_mul(N), cap.unchecked_mul(N)) }
        };
        // SAFETY:
        // - `ptr` was allocated by `self`
        // - `ptr` is well-aligned because `[T; N]` has the same alignment as `T`.
        // - `new_cap` refers to the same sized allocation as `cap` because
        // `new_cap * size_of::<T>()` == `cap * size_of::<[T; N]>()`
        // - `len` <= `cap`, so `len * N` <= `cap * N`.
        unsafe { VVec::<T, A>::from_raw_parts_in(ptr.cast(), new_len, new_cap, alloc) }
    }
}

impl<T: Clone, A: Allocator> VVec<T, A> {

    /// Extend the vector by `n` clones of value.
    fn extend_with(&mut self, n: usize, value: T) {
        self.reserve(n);

        unsafe {
            let mut ptr = self.as_mut_ptr().add(self.len());
            // Use VVecSetLenOnDrop to work around bug where compiler
            // might not realize the store through `ptr` through self.set_len()
            // don't alias.
            let mut local_len = VVecSetLenOnDrop::new(&mut self.len);

            // Write all elements except the last one
            for _ in 1..n {
                ptr::write(ptr, value.clone());
                ptr = ptr.add(1);
                // Increment the length in every step in case clone() panics
                local_len.increment_len(1);
            }

            if n > 0 {
                // We can write the last element directly without cloning needlessly
                ptr::write(ptr, value);
                local_len.increment_len(1);
            }

            // len set by scope guard
        }
    }
}

impl<T: PartialEq, A: Allocator> VVec<T, A> {
    /// Removes consecutive repeated elements in the vector according to the
    /// [`PartialEq`] trait implementation.
    ///
    /// If the vector is sorted, this removes all duplicates.
    ///
    /// # Examples
    ///

    #[inline]
    pub fn dedup(&mut self) {
        self.dedup_by(|a, b| a == b)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Internal methods and functions
////////////////////////////////////////////////////////////////////////////////

#[doc(hidden)]

pub fn from_elem<T: Clone>(elem: T, n: usize) -> VVec<T> {
    <T as SpecFromElem>::from_elem(elem, n, Global)
}

#[doc(hidden)]

pub fn from_elem_in<T: Clone, A: Allocator>(elem: T, n: usize, alloc: A) -> VVec<T, A> {
    <T as SpecFromElem>::from_elem(elem, n, alloc)
}

trait ExtendFromWithinSpec {
    /// # Safety
    ///
    /// - `src` needs to be valid index
    /// - `self.capacity() - self.len()` must be `>= src.len()`
    unsafe fn spec_extend_from_within(&mut self, src: Range<usize>);
}

impl<T: Clone, A: Allocator> ExtendFromWithinSpec for VVec<T, A> {
    default unsafe fn spec_extend_from_within(&mut self, src: Range<usize>) {
        // SAFETY:
        // - len is increased only after initializing elements
        let (this, spare, len) = unsafe { self.split_at_spare_mut_with_len() };

        // SAFETY:
        // - caller guarantees that src is a valid index
        let to_clone = unsafe { this.get_unchecked(src) };

        iter::zip(to_clone, spare)
            .map(|(src, dst)| dst.write(src.clone()))
            // Note:
            // - Element was just initialized with `MaybeUninit::write`, so it's ok to increase len
            // - len is increased after each element to prevent leaks (see issue #82533)
            .for_each(|_| *len += 1);
    }
}

impl<T: TrivialClone, A: Allocator> ExtendFromWithinSpec for VVec<T, A> {
    unsafe fn spec_extend_from_within(&mut self, src: Range<usize>) {
        let count = src.len();
        {
            let (init, spare) = self.split_at_spare_mut();

            // SAFETY:
            // - caller guarantees that `src` is a valid index
            let source = unsafe { init.get_unchecked(src) };

            // SAFETY:
            // - Both pointers are created from unique slice references (`&mut [_]`)
            //   so they are valid and do not overlap.
            // - Elements implement `TrivialClone` so this is equivalent to calling
            //   `clone` on every one of them.
            // - `count` is equal to the len of `source`, so source is valid for
            //   `count` reads
            // - `.reserve(count)` guarantees that `spare.len() >= count` so spare
            //   is valid for `count` writes
            unsafe { ptr::copy_nonoverlapping(source.as_ptr(), spare.as_mut_ptr() as _, count) };
        }

        // SAFETY:
        // - The elements were just initialized by `copy_nonoverlapping`
        self.len += count;
    }
}

////////////////////////////////////////////////////////////////////////////////
// Common trait implementations for VVec
////////////////////////////////////////////////////////////////////////////////

impl<T, A: Allocator> ops::Deref for VVec<T, A> {
    type Target = [T];

    #[inline]
    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T, A: Allocator> ops::DerefMut for VVec<T, A> {
    #[inline]
    fn deref_mut(&mut self) -> &mut [T] {
        self.as_mut_slice()
    }
}

unsafe impl<T, A: Allocator> ops::DerefPure for VVec<T, A> {}

impl<T: Clone, A: Allocator + Clone> Clone for VVec<T, A> {
    fn clone(&self) -> Self {
        // Adapted: upstream uses the alloc-internal `<[T]>::to_vec_in`. Over the public
        // surface, clone by allocating and cloning each element (same result).
        let alloc = self.allocator().clone();
        let mut v = VVec::with_capacity_in(self.len(), alloc);
        v.extend(self.iter().cloned());
        v
    }

    /// Overwrites the contents of `self` with a clone of the contents of `source`.
    ///
    /// This method is preferred over simply assigning `source.clone()` to `self`,
    /// as it avoids reallocation if possible. Additionally, if the element type
    /// `T` overrides `clone_from()`, this will reuse the resources of `self`'s
    /// elements as well.
    ///
    /// # Examples
    ///
    fn clone_from(&mut self, source: &Self) {
        // Adapted: upstream delegates to the alloc-private `slice::SpecCloneIntoVec`. Over
        // the public surface, reuse `self`'s allocation by clearing and re-extending (same
        // result, same reuse-if-capacity-suffices behavior as VVecDeque's clone_from).
        self.clear();
        self.extend(source.iter().cloned());
    }
}

/// The hash of a vector is the same as that of the corresponding slice,
/// as required by the `core::borrow::Borrow` implementation.
///

impl<T: Hash, A: Allocator> Hash for VVec<T, A> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        Hash::hash(&**self, state)
    }
}

impl<T, I: SliceIndex<[T]>, A: Allocator> Index<I> for VVec<T, A> {
    type Output = I::Output;

    #[inline]
    fn index(&self, index: I) -> &Self::Output {
        Index::index(&**self, index)
    }
}

impl<T, I: SliceIndex<[T]>, A: Allocator> IndexMut<I> for VVec<T, A> {
    #[inline]
    fn index_mut(&mut self, index: I) -> &mut Self::Output {
        IndexMut::index_mut(&mut **self, index)
    }
}

/// Collects an iterator into a VVec, commonly called via [`Iterator::collect()`]
///
/// # Allocation behavior
///
/// In general `VVec` does not guarantee any particular growth or allocation strategy.
/// That also applies to this trait impl.
///
/// **Note:** This section covers implementation details and is therefore exempt from
/// stability guarantees.
///
/// VVec may use any or none of the following strategies,
/// depending on the supplied iterator:
///
/// * preallocate based on [`Iterator::size_hint()`]
///   * and panic if the number of items is outside the provided lower/upper bounds
/// * use an amortized growth strategy similar to `pushing` one item at a time
/// * perform the iteration in-place on the original allocation backing the iterator
///
/// The last case warrants some attention. It is an optimization that in many cases reduces peak memory
/// consumption and improves cache locality. But when big, short-lived allocations are created,
/// only a small fraction of their items get collected, no further use is made of the spare capacity
/// and the resulting `VVec` is moved into a longer-lived structure, then this can lead to the large
/// allocations having their lifetimes unnecessarily extended which can result in increased memory
/// footprint.
///
/// In cases where this is an issue, the excess capacity can be discarded with [`VVec::shrink_to()`],
/// [`VVec::shrink_to_fit()`] or by collecting into [`Box<[T]>`][owned slice] instead, which additionally reduces
/// the size of the long-lived struct.
///
/// [owned slice]: Box
///

impl<T> FromIterator<T> for VVec<T> {
    #[inline]
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> VVec<T> {
        // Routed to SpecFromIterNested (the naive build-then-extend path): the
        // top-level SpecFromIter is a corpse because its VVecIntoIter specialization
        // delegates to the in-place machinery (in_place_collect), which the shim
        // cannot support. SpecFromIterNested is what SpecFromIter's default impl
        // delegated to anyway.
        <Self as SpecFromIterNested<T, I::IntoIter>>::from_iter(iter.into_iter())
    }
}

impl<T, A: Allocator> IntoIterator for VVec<T, A> {
    type Item = T;
    type IntoIter = VVecIntoIter<T, A>;

    /// Creates a consuming iterator, that is, one that moves each value out of
    /// the vector (from start to end). The vector cannot be used after calling
    /// this.
    ///
    /// # Examples
    ///
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        unsafe {
            let me = ManuallyDrop::new(self);
            let alloc = ManuallyDrop::new(ptr::read(me.allocator()));
            let buf = me.buf.non_null();
            let begin = buf.as_ptr();
            let end = if T::IS_ZST {
                begin.wrapping_byte_add(me.len())
            } else {
                begin.add(me.len()) as *const T
            };
            let cap = me.buf.capacity();
            VVecIntoIter { buf, phantom: PhantomData, cap, alloc, ptr: buf, end }
        }
    }
}

impl<'a, T, A: Allocator> IntoIterator for &'a VVec<T, A> {
    type Item = &'a T;
    type IntoIter = slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, T, A: Allocator> IntoIterator for &'a mut VVec<T, A> {
    type Item = &'a mut T;
    type IntoIter = slice::IterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<T, A: Allocator> Extend<T> for VVec<T, A> {
    #[inline]
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        // lazy_loss_recovery: finish a forgotten drain before extending (spec_extend reads len
        // directly). The extend runs in a separate never-inlined fn so the (never-taken) guard call
        // cannot merge into and poison the specialized bulk-copy path (the deque `extend` lesson).
        self.restore_wf_wo_data_loss();
        #[inline(never)]
        fn extend_after_wf<T, A: Allocator, I: Iterator<Item = T>>(this: &mut VVec<T, A>, iter: I) {
            <VVec<T, A> as SpecExtend<T, I>>::spec_extend(this, iter);
        }
        extend_after_wf(self, iter.into_iter())
    }

    #[inline]
    fn extend_one(&mut self, item: T) {
        self.push(item);
    }

    #[inline]
    fn extend_reserve(&mut self, additional: usize) {
        self.reserve(additional);
    }

    #[inline]
    unsafe fn extend_one_unchecked(&mut self, item: T) {
        // SAFETY: Our preconditions ensure the space has been reserved, and `extend_reserve` is implemented correctly.
        unsafe {
            let len = self.len();
            ptr::write(self.as_mut_ptr().add(len), item);
            self.set_len(len + 1);
        }
    }
}

impl<T, A: Allocator> VVec<T, A> {
    // leaf method to which various SpecFrom/SpecExtend implementations delegate when
    // they have no further optimizations to apply

    fn extend_desugared<I: Iterator<Item = T>>(&mut self, mut iterator: I) {
        // This is the case for a general iterator.
        //
        // This function should be the moral equivalent of:
        //
        //      for item in iterator {
        //          self.push(item);
        //      }
        while let Some(element) = iterator.next() {
            let len = self.len();
            if len == self.capacity() {
                let (lower, _) = iterator.size_hint();
                self.reserve(lower.saturating_add(1));
            }
            unsafe {
                ptr::write(self.as_mut_ptr().add(len), element);
                // Since next() executes user code which can panic we have to bump the length
                // after each step.
                // NB can't overflow since we would have had to alloc the address space
                self.set_len(len + 1);
            }
        }
    }

    // specific extend for `TrustedLen` iterators, called both by the specializations
    // and internal places where resolving specialization makes compilation slower

    fn extend_trusted(&mut self, iterator: impl iter::TrustedLen<Item = T>) {
        let (low, high) = iterator.size_hint();
        if let Some(additional) = high {
            debug_assert_eq!(
                low,
                additional,
                "TrustedLen iterator's size hint is not exact: {:?}",
                (low, high)
            );
            self.reserve(additional);
            unsafe {
                let ptr = self.as_mut_ptr();
                let mut local_len = VVecSetLenOnDrop::new(&mut self.len);
                iterator.for_each(move |element| {
                    ptr::write(ptr.add(local_len.current_len()), element);
                    // Since the loop executes user code which can panic we have to update
                    // the length every step to correctly drop what we've written.
                    // NB can't overflow since we would have had to alloc the address space
                    local_len.increment_len(1);
                });
            }
        } else {
            // Per TrustedLen contract a `None` upper bound means that the iterator length
            // truly exceeds usize::MAX, which would eventually lead to a capacity overflow anyway.
            // Since the other branch already panics eagerly (via `reserve()`) we do the same here.
            // This avoids additional codegen for a fallback code path which would eventually
            // panic anyway.
            panic!("capacity overflow");
        }
    }

    /// Creates a splicing iterator that replaces the specified range in the vector
    /// with the given `replace_with` iterator and yields the removed items.
    /// `replace_with` does not need to be the same length as `range`.
    ///
    /// `range` is removed even if the `VVecSplice` iterator is not consumed before it is dropped.
    ///
    /// lazy_loss_recovery forget-safety (differs from std): if the `VVecSplice` is **leaked**
    /// (`mem::forget`), the vec's own elements are **not** lost — the next op lazily repairs the vec
    /// to well-formed (the `range` is removed and dropped, the tail is kept). Only the un-inserted
    /// `replace_with` iterator leaks (it is consumed only on drop, and a leaked Splice never drops
    /// it); that is unavoidable without an `I: 'static` bound, and the signature is kept identical to
    /// std. So: the vec is repaired; only the abandoned replacement input leaks.
    ///
    /// This is optimal if:
    ///
    /// * The tail (elements in the vector after `range`) is empty,
    /// * or `replace_with` yields fewer or equal elements than `range`'s length
    /// * or the lower bound of its `size_hint()` is exact.
    ///
    /// Otherwise, a temporary vector is allocated and the tail is moved twice.
    ///
    /// # Panics
    ///
    /// Panics if the range has `start_bound > end_bound`, or, if the range is
    /// bounded on either end and past the length of the vector.
    ///
    /// # Examples
    ///
    ///
    /// Using `splice` to insert new items into a vector efficiently at a specific position
    /// indicated by an empty range:
    ///

    #[inline]

    pub fn splice<R, I>(&mut self, range: R, replace_with: I) -> VVecSplice<'_, I::IntoIter, A>
    where
        R: RangeBounds<usize>,
        I: IntoIterator<Item = T>,
    {
        VVecSplice { drain: self.drain(range), replace_with: replace_with.into_iter() }
    }

    /// Creates an iterator which uses a closure to determine if an element in the range should be removed.
    ///
    /// If the closure returns `true`, the element is removed from the vector
    /// and yielded. If the closure returns `false`, or panics, the element
    /// remains in the vector and will not be yielded.
    ///
    /// Only elements that fall in the provided range are considered for extraction, but any elements
    /// after the range will still have to be moved if any element has been extracted.
    ///
    /// If the returned `VVecExtractIf` is not exhausted, e.g. because it is dropped without iterating
    /// or the iteration short-circuits, then the remaining elements will be retained.
    /// Use `extract_if().for_each(drop)` if you do not need the returned iterator,
    /// or [`retain_mut`] with a negated predicate if you also do not need to restrict the range.
    ///
    /// [`retain_mut`]: VVec::retain_mut
    ///
    /// Using this method is equivalent to the following code:
    ///
    ///
    /// But `extract_if` is easier to use. `extract_if` is also more efficient,
    /// because it can backshift the elements of the array in bulk.
    ///
    /// The iterator also lets you mutate the value of each element in the
    /// closure, regardless of whether you choose to keep or remove it.
    ///
    /// # Panics
    ///
    /// If `range` is out of bounds.
    ///
    /// # Examples
    ///
    /// Splitting a vector into even and odd values, reusing the original vector:
    ///
    ///
    /// Using the range argument to only process a part of the vector:
    ///

    pub fn extract_if<F, R>(&mut self, range: R, filter: F) -> VVecExtractIf<'_, T, F, A>
    where
        F: FnMut(&mut T) -> bool,
        R: RangeBounds<usize>,
    {
        self.restore_wf_wo_data_loss(); // lazy_loss_recovery: finish a forgotten drain/extract_if first
        VVecExtractIf::new(self, filter, range)
    }
}

/// Extend implementation that copies elements out of references before pushing them onto the VVec.
///
/// This implementation is specialized for slice iterators, where it uses [`copy_from_slice`] to
/// append the entire slice at once.
///
/// [`copy_from_slice`]: slice::copy_from_slice

impl<'a, T: Copy + 'a, A: Allocator> Extend<&'a T> for VVec<T, A> {
    fn extend<I: IntoIterator<Item = &'a T>>(&mut self, iter: I) {
        self.spec_extend(iter.into_iter())
    }

    #[inline]
    fn extend_one(&mut self, &item: &'a T) {
        self.push(item);
    }

    #[inline]
    fn extend_reserve(&mut self, additional: usize) {
        self.reserve(additional);
    }

    #[inline]
    unsafe fn extend_one_unchecked(&mut self, &item: &'a T) {
        // SAFETY: Our preconditions ensure the space has been reserved, and `extend_reserve` is implemented correctly.
        unsafe {
            let len = self.len();
            ptr::write(self.as_mut_ptr().add(len), item);
            self.set_len(len + 1);
        }
    }
}

/// Implements comparison of vectors, [lexicographically](Ord#lexicographical-comparison).

impl<T, A1, A2> PartialOrd<VVec<T, A2>> for VVec<T, A1>
where
    T: PartialOrd,
    A1: Allocator,
    A2: Allocator,
{
    #[inline]
    fn partial_cmp(&self, other: &VVec<T, A2>) -> Option<Ordering> {
        PartialOrd::partial_cmp(&**self, &**other)
    }
}

impl<T: Eq, A: Allocator> Eq for VVec<T, A> {}

/// Implements ordering of vectors, [lexicographically](Ord#lexicographical-comparison).

impl<T: Ord, A: Allocator> Ord for VVec<T, A> {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        Ord::cmp(&**self, &**other)
    }
}

unsafe impl<#[may_dangle] T, A: Allocator> Drop for VVec<T, A> {
    fn drop(&mut self) {
        // lazy_loss_recovery: if a `drain()` iterator was forgotten, finish it first so the
        // un-yielded drained elements are dropped (not leaked) and `len` covers the full survivor
        // set before we drop it.
        self.restore_wf_wo_data_loss();
        unsafe {
            // use drop for [T]
            // use a raw slice to refer to the elements of the vector as weakest necessary type;
            // could avoid questions of validity in certain cases
            ptr::drop_in_place(ptr::slice_from_raw_parts_mut(self.as_mut_ptr(), self.len))
        }
        // VRawVec handles deallocation
    }
}

impl<T> Default for VVec<T> {
    /// Creates an empty `VVec<T>`.
    ///
    /// The vector will not allocate until elements are pushed onto it.
    fn default() -> VVec<T> {
        VVec::new()
    }
}

impl<T: fmt::Debug, A: Allocator> fmt::Debug for VVec<T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T, A: Allocator> AsRef<VVec<T, A>> for VVec<T, A> {
    fn as_ref(&self) -> &VVec<T, A> {
        self
    }
}

impl<T, A: Allocator> AsMut<VVec<T, A>> for VVec<T, A> {
    fn as_mut(&mut self) -> &mut VVec<T, A> {
        self
    }
}

impl<T, A: Allocator> AsRef<[T]> for VVec<T, A> {
    fn as_ref(&self) -> &[T] {
        self
    }
}

impl<T, A: Allocator> AsMut<[T]> for VVec<T, A> {
    fn as_mut(&mut self) -> &mut [T] {
        self
    }
}

impl<T: Clone> From<&[T]> for VVec<T> {
    /// Allocates a `VVec<T>` and fills it by cloning `s`'s items.
    ///
    /// # Examples
    ///
    fn from(s: &[T]) -> VVec<T> {
        // Adapted: upstream `s.to_vec()` returns a std `Vec`; clone into a `VVec` instead.
        let mut v = VVec::with_capacity(s.len());
        v.extend(s.iter().cloned());
        v
    }
}

impl<T: Clone> From<&mut [T]> for VVec<T> {
    /// Allocates a `VVec<T>` and fills it by cloning `s`'s items.
    ///
    /// # Examples
    ///
    fn from(s: &mut [T]) -> VVec<T> {
        // Adapted: upstream `s.to_vec()` returns a std `Vec`; clone into a `VVec` instead.
        let mut v = VVec::with_capacity(s.len());
        v.extend(s.iter().cloned());
        v
    }
}

impl<T: Clone, const N: usize> From<&[T; N]> for VVec<T> {
    /// Allocates a `VVec<T>` and fills it by cloning `s`'s items.
    ///
    /// # Examples
    ///
    fn from(s: &[T; N]) -> VVec<T> {
        Self::from(s.as_slice())
    }
}

impl<T: Clone, const N: usize> From<&mut [T; N]> for VVec<T> {
    /// Allocates a `VVec<T>` and fills it by cloning `s`'s items.
    ///
    /// # Examples
    ///
    fn from(s: &mut [T; N]) -> VVec<T> {
        Self::from(s.as_mut_slice())
    }
}

impl<T, const N: usize> From<[T; N]> for VVec<T> {
    /// Allocates a `VVec<T>` and moves `s`'s items into it.
    ///
    /// # Examples
    ///
    fn from(s: [T; N]) -> VVec<T> {
        // Adapted: upstream `<[T]>::into_vec(Box::new(s))` reuses the box allocation and
        // returns a std `Vec`. Move the array's elements into a fresh `VVec`.
        let mut v = VVec::with_capacity(N);
        v.extend(s);
        v
    }
}

impl<'a, T> From<Cow<'a, [T]>> for VVec<T>
where
    [T]: ToOwned<Owned = VVec<T>>,
{
    /// Converts a clone-on-write slice into a vector.
    ///
    /// If `s` already owns a `VVec<T>`, it will be returned directly.
    /// If `s` is borrowing a slice, a new `VVec<T>` will be allocated and
    /// filled by cloning `s`'s items into it.
    ///
    /// # Examples
    ///
    fn from(s: Cow<'a, [T]>) -> VVec<T> {
        s.into_owned()
    }
}

// note: test pulls in std, which causes errors here

impl<T, A: Allocator> From<Box<[T], A>> for VVec<T, A> {
    /// Converts a boxed slice into a vector by transferring ownership of
    /// the existing heap allocation.
    ///
    /// # Examples
    ///
    fn from(s: Box<[T], A>) -> Self {
        // Adapted: upstream `s.into_vec()` is the alloc-internal bridge that transfers the
        // box's heap allocation into a `Vec`. Do the same transfer into a `VVec` by raw
        // parts (no copy, no realloc — ownership of the existing allocation moves).
        let len = s.len();
        let (ptr, alloc) = Box::into_raw_with_allocator(s);
        // SAFETY: `ptr` is a `len`-element allocation from `alloc` with all `len` elements
        // initialized; a boxed slice has length == capacity.
        unsafe { VVec::from_raw_parts_in(ptr as *mut T, len, len, alloc) }
    }
}

// note: test pulls in std, which causes errors here

// CORPSE (ProcessCommentingStandard): `From<VVec<T, A>> for Box<[T], A>` implements the
// FOREIGN `From` trait for the FOREIGN `Box` type, with the type parameter `A` uncovered
// before the local `VVec` — an orphan-rule violation here (E0210; legal in `alloc` where
// `Box` is local). Use `VVec::into_boxed_slice` for the same conversion.
/*
impl<T, A: Allocator> From<VVec<T, A>> for Box<[T], A> {
    /// Converts a vector into a boxed slice.
    ///
    /// Before doing the conversion, this method discards excess capacity like [`VVec::shrink_to_fit`].
    ///
    /// [owned slice]: Box
    /// [`VVec::shrink_to_fit`]: VVec::shrink_to_fit
    ///
    /// # Examples
    ///
    ///
    /// Any excess capacity is removed:
    fn from(v: VVec<T, A>) -> Self {
        v.into_boxed_slice()
    }
}
*/

impl From<&str> for VVec<u8> {
    /// Allocates a `VVec<u8>` and fills it with a UTF-8 string.
    ///
    /// # Examples
    ///
    fn from(s: &str) -> VVec<u8> {
        From::from(s.as_bytes())
    }
}

impl<T, A: Allocator, const N: usize> TryFrom<VVec<T, A>> for [T; N] {
    type Error = VVec<T, A>;

    /// Gets the entire contents of the `VVec<T>` as an array,
    /// if its size exactly matches that of the requested array.
    ///
    /// # Examples
    ///
    ///
    /// If the length doesn't match, the input comes back in `Err`:
    ///
    /// If you're fine with just getting a prefix of the `VVec<T>`,
    /// you can call [`.truncate(N)`](VVec::truncate) first.
    fn try_from(mut vec: VVec<T, A>) -> Result<[T; N], VVec<T, A>> {
        if vec.len() != N {
            return Err(vec);
        }

        // SAFETY: `.set_len(0)` is always sound.
        unsafe { vec.set_len(0) };

        // SAFETY: A `VVec`'s pointer is always aligned properly, and
        // the alignment the array needs is the same as the items.
        // We checked earlier that we have sufficient items.
        // The items will not double-drop as the `set_len`
        // tells the `VVec` not to also drop them.
        let array = unsafe { ptr::read(vec.as_ptr() as *const [T; N]) };
        Ok(array)
    }
}
