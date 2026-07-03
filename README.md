# Rust Leakage

 The rust language has a very badly designed concept of "leak-amplification". 
 If 
 
  ## Leaking in Rust: `mem::forget`, the "Leakpocalypse", and RFC 1066

  ## Where the Rust std docs specify leaking

  Rust doesn't specify leakage in one canonical place. The policy statement
  ("destructors are not guaranteed to run; leaking is safe, not `unsafe`") lives
  in two spots, and there are several explicit leak/forget APIs documented
  elsewhere.

  ### Policy statements (the "leaking is safe" doctrine)

  | # | Location | What it says |
  |---|----------|--------------|
  | 1 | [`std::mem::forget`](https://doc.rust-lang.org/std/mem/fn.forget.html) | "`forget` is not marked as `unsafe`, because Rust's safety guarantees do not
  include a guarantee that destructors will always run." Notes that `Rc` cycles and `process::exit` already make leaks reachable in safe code. |
  | 2 | [The Rustonomicon — "Leaking"](https://doc.rust-lang.org/nomicon/leaking.html) | Long-form explanation (leak amplification, `Vec::drain`, `Rc`,
  `thread::scoped`). Official docs, though not the API reference. |

  ### Explicit leak/forget APIs in std

  | # | API | Behavior |
  |---|-----|----------|
  | 3 | `Box::leak` | Leaks the box, returns `&'static mut T`. |
  | 4 | `Vec::leak` | Leaks the buffer, returns `&'static mut [T]`. |
  | 5 | `String::leak` | Leaks the buffer, returns `&'static mut str`. |
  | 6 | `std::mem::ManuallyDrop` | Suppresses the destructor (the type-level counterpart to `forget`). |
  | 7 | `CString::into_raw`, `Box::into_raw`, `Rc/Arc::into_raw`, `Vec::into_raw_parts` | Give up ownership without dropping (leak unless reclaimed). |

  So: two doctrinal statements (`mem::forget` + Nomicon) and roughly five
  families of leak-by-design APIs.

  ## The online arguments ("leakpocalypse" / "leak apocalypse")

  The debate erupted pre-1.0, in April 2015, when @arielb1 showed that
  `thread::scoped` (which returned a `JoinGuard` RAII join-on-drop 1 new message 


  ## Leak notes on collection APIs

  | # | Type | Section | URL |
  |---|------|---------|-----|
  | 1 | `Vec` | `drain` → **§ Leaking** ("…the vector may have lost and leaked elements arbitrarily, including elements outside the range") |
  https://doc.rust-lang.org/std/vec/struct.Vec.html#method.drain |
  | 2 | `VecDeque` | `drain` → **§ Leaking** ("…the deque may have lost and leaked elements arbitrarily, including elements outside the range") |
  https://doc.rust-lang.org/std/collections/vec_deque/struct.VecDeque.html#method.drain |
  | 3 | `BinaryHeap` | `peek_mut` → **Leaking note** ("If the `PeekMut` value is leaked, some heap elements might get leaked along with it, but the remaining
  elements will remain a valid heap") | https://doc.rust-lang.org/std/collections/struct.BinaryHeap.html#method.peek_mut |


