# Rust Leakage

 The rust language has a very badly designed concept of "leak-amplification" in which core
 standard library data structures are specified to lose data when mutable iterators are
 not dropped due to 'safe' forgets and panics that may unwind the stack and allow the
 program to continue.

 This project documents and investigates the cost of this intentional destruction of
program state with alternative implementations.
 
  ## Contents

  - [Leaking in Rust: `mem::forget`, the "Leakpocalypse", and RFC 1066](#leaking-in-rust-memforget-the-leakpocalypse-and-rfc-1066)
  - [Where the Rust std docs specify leaking](#where-the-rust-std-docs-specify-leaking)
    - [Undefined behaviors and leaking](#undefined-behaviors-and-leaking)
    - [Explicit leak/forget APIs in std](#explicit-leakforget-apis-in-std)
  - [The online arguments ("leakpocalypse" / "leak apocalypse")](#the-online-arguments-leakpocalypse--leak-apocalypse)
  - [Leak notes on collection APIs](#leak-notes-on-collection-apis)
  - [Benchmark results](#benchmark-results)

  ## Leaking in Rust: `mem::forget`, the "Leakpocalypse", and RFC 1066

  ## Where the Rust std docs specify leaking

  Rust doesn't specify leakage in one canonical place. The policy statement
  is "destructors are not guaranteed to run; leaking is safe, not `unsafe`". 

  ### Undefined behaviors and leaking

  - **1.** [`std::mem::forget`](https://doc.rust-lang.org/std/mem/fn.forget.html) — "`forget` is not marked as `unsafe`, because Rust's safety guarantees do not include a guarantee that destructors will always run." Notes that `Rc` cycles and `process::exit` already make leaks reachable in safe code.
  - **2.** [The Rustonomicon — "Leaking"](https://doc.rust-lang.org/nomicon/leaking.html) — Long-form explanation (leak amplification, `Vec::drain`, `Rc`, `thread::scoped`). Official docs, though not the API reference.

  ### Explicit leak/forget APIs in std

  Rust allows leaking in 'safe' code and recoverable panics using:

  - **1.** [`std::mem::forget`](https://doc.rust-lang.org/std/mem/fn.forget.html) — Runs no destructor for the value; the canonical safe-code leak primitive.
  - **2.** [`Box::leak`](https://doc.rust-lang.org/std/boxed/struct.Box.html#method.leak) — Leaks the box, returns `&'static mut T`.
  - **3.** [`Vec::leak`](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.leak) — Leaks the buffer, returns `&'static mut [T]`.
  - **4.** [`String::leak`](https://doc.rust-lang.org/std/string/struct.String.html#method.leak) — Leaks the buffer, returns `&'static mut str`.
  - **5.** [`std::mem::ManuallyDrop`](https://doc.rust-lang.org/std/mem/struct.ManuallyDrop.html) — Suppresses the destructor (the type-level counterpart to [`forget`](https://doc.rust-lang.org/std/mem/fn.forget.html)).
  - **6.** [`CString::into_raw`](https://doc.rust-lang.org/std/ffi/struct.CString.html#method.into_raw) — Gives up ownership as a raw pointer; leaks unless reclaimed with `from_raw`.
  - **7.** [`Box::into_raw`](https://doc.rust-lang.org/std/boxed/struct.Box.html#method.into_raw) — Gives up ownership as a raw pointer; leaks unless reclaimed with `from_raw`.
  - **8.** [`Rc::into_raw`](https://doc.rust-lang.org/std/rc/struct.Rc.html#method.into_raw) — Gives up ownership as a raw pointer without decrementing the strong count; leaks unless reclaimed.
  - **9.** [`Arc::into_raw`](https://doc.rust-lang.org/std/sync/struct.Arc.html#method.into_raw) — Gives up ownership as a raw pointer without decrementing the strong count; leaks unless reclaimed.
  - **10.** [`Vec::into_raw_parts`](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.into_raw_parts) — Decomposes the vec into raw parts; leaks unless reclaimed with `from_raw_parts` (unstable).

  ## The online arguments ("leakpocalypse" / "leak apocalypse")

  The debate erupted before Rust 1.0, in April 2015, when [@arielb1](https://github.com/arielb1)
  showed that `thread::scoped` was unsound
  ([rust-lang/rust#24292](https://github.com/rust-lang/rust/issues/24292)). It returned a
  `JoinGuard` that joined the child thread when dropped, and the child borrowed the parent's stack.
  Because any value can be leaked in safe code — via [`mem::forget`](https://doc.rust-lang.org/std/mem/fn.forget.html)
  or an `Rc` reference cycle — that destructor could be skipped, letting the child keep running
  after the borrowed stack frame was freed: a use-after-free reachable from safe code.

  The resolution was [RFC 1066](https://rust-lang.github.io/rfcs/1066-safe-mem-forget.html), which
  fixed the language rule that leaking is safe: a destructor is not guaranteed to run, so no API may
  rely on `Drop` for memory safety. `thread::scoped` was removed, and scoped threads returned years
  later as the closure-based [`std::thread::scope`](https://doc.rust-lang.org/std/thread/fn.scope.html)
  (stable in 1.63). This is the rule that licenses the leak-amplification in `Vec::drain` and the
  others below: losing data on a forgotten iterator is defined to be safe, and it is the type's
  responsibility, not `forget`'s.


  ## Leak notes on collection APIs

  - **1.** `Vec` — `drain` → **§ Leaking** ("…the vector may have lost and leaked elements arbitrarily, including elements outside the range"). <https://doc.rust-lang.org/std/vec/struct.Vec.html#method.drain>
  - **2.** `VecDeque` — `drain` → **§ Leaking** ("…the deque may have lost and leaked elements arbitrarily, including elements outside the range"). <https://doc.rust-lang.org/std/collections/vec_deque/struct.VecDeque.html#method.drain>
  - **3.** `BinaryHeap` — `peek_mut` → **Leaking note** ("If the `PeekMut` value is leaked, some heap elements might get leaked along with it, but the remaining elements will remain a valid heap"). <https://doc.rust-lang.org/std/collections/struct.BinaryHeap.html#method.peek_mut>

  ## Benchmark results

  We rebuilt each `std` collection's leaking baseline plus a forget-safe variant and benchmarked them.
  The methodology, full per-collection tables, and the cross-collection summary are in
  [`docs/Leakage.md`](docs/Leakage.md). This shows very small cost to get real data well-formedness without leakage.


