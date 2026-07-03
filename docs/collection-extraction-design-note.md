

  So the decision is not "we'd rather go fast than not drop your data." It's three layered claims, in priority order:

  1. Soundness (primary). The eager len = 0 is what keeps the API memory-safe if the iterator is leaked. The naive "keep the Vec consistent after every
  element" alternative isn't just slow — skipping it is outright UB in safe code. That's the real driver.
  2. Cost (secondary, and asserted, not measured). "enormous cost… would negate any benefits of the API." There's no benchmark behind that sentence — it's a
  design assertion.
  3. Leak license (the foundation). This all rests on the pre-1.0 "leakpocalypse" decision — RFC 1066, safe mem::forget — which declared that leaking is safe
  and that unsafe code may not rely on Drop running. Once leaking is blessed by the language, leak-amplification becomes a legitimate technique, and "data on
  the floor via forget" is something you explicitly opted into by calling forget.


https://rust-lang.github.io/rfcs/1066-safe-mem-forget.html

