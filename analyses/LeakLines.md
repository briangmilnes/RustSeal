# LeakLines — forget/leak call census over the RustProjects corpus

*Generated 2026-07-03 16:05 PDT. Gross `rg` census of `forget(` and `leak(` calls across every
`.rs` file in `RustProjects/` (1001 cloned crates, source-only). `target/` excluded;
`into_raw`/`into_raw_parts` deliberately skipped this pass. Counts include comments,
strings, and user-defined `forget`/`leak` methods — a magnitude estimate, not precise.*

## Call families

| # | family | ripgrep pattern | matches | files |
|--:|--------|-----------------|--------:|------:|
| 1 | forget | `\bforget\s*\(` | 1436 | 484 |
| 2 | leak | `\bleak\s*\(` | 326 | 169 |
| — | **TOTAL** | | **1762** | |

Skipped this pass: `into_raw`, `into_raw_parts`.

## Top 40 projects by forget+leak calls

| # | project | forget | leak | total |
|--:|---------|-------:|-----:|------:|
| 1 | `microsoft__windows-rs` | 598 | 6 | 604 |
| 2 | `napi-rs__napi-rs` | 26 | 41 | 67 |
| 3 | `wasm-bindgen__wasm-bindgen` | 42 | 17 | 59 |
| 4 | `google__zerocopy` | 45 | 2 | 47 |
| 5 | `mvdnes__spin-rs` | 31 | 15 | 46 |
| 6 | `Amanieu__parking_lot` | 42 | 1 | 43 |
| 7 | `rodrigocfd__winsafe` | 0 | 41 | 41 |
| 8 | `bytecodealliance__wasi-rs` | 40 | 0 | 40 |
| 9 | `rust-openssl__rust-openssl` | 38 | 0 | 38 |
| 10 | `psychon__x11rb` | 28 | 0 | 28 |
| 11 | `tokio-rs__tokio` | 22 | 2 | 24 |
| 12 | `servo__stylo` | 22 | 2 | 24 |
| 13 | `rust-lang__rust-analyzer` | 10 | 12 | 22 |
| 14 | `rust-lang__futures-rs` | 18 | 2 | 20 |
| 15 | `bytecodealliance__wit-bindgen` | 13 | 6 | 19 |
| 16 | `rustwasm__gloo` | 16 | 0 | 16 |
| 17 | `bytecodealliance__wasmtime` | 16 | 0 | 16 |
| 18 | `GitoxideLabs__gitoxide` | 12 | 4 | 16 |
| 19 | `jni-rs__jni-rs` | 11 | 5 | 16 |
| 20 | `tokio-rs__loom` | 3 | 12 | 15 |
| 21 | `smol-rs__async-executor` | 3 | 12 | 15 |
| 22 | `swc-project__swc` | 13 | 2 | 15 |
| 23 | `briansmith__ring` | 0 | 15 | 15 |
| 24 | `fitzgen__bumpalo` | 9 | 5 | 14 |
| 25 | `gfx-rs__wgpu` | 13 | 0 | 13 |
| 26 | `crossbeam-rs__crossbeam` | 12 | 1 | 13 |
| 27 | `awesomized__crc-fast-rust` | 0 | 12 | 12 |
| 28 | `ferrilab__ferrilab` | 3 | 7 | 10 |
| 29 | `zakarumych__allocator-api2` | 2 | 8 | 10 |
| 30 | `smol-rs__async-lock` | 10 | 0 | 10 |
| 31 | `rayon-rs__rayon` | 10 | 0 | 10 |
| 32 | `contain-rs__linked-hash-map` | 10 | 0 | 10 |
| 33 | `servo__rust-smallvec` | 8 | 1 | 9 |
| 34 | `servo__core-foundation-rs` | 8 | 1 | 9 |
| 35 | `rust-lang__hashbrown` | 7 | 2 | 9 |
| 36 | `smithy-lang__smithy-rs` | 5 | 4 | 9 |
| 37 | `awslabs__smithy-rs` | 5 | 4 | 9 |
| 38 | `bitvecto-rs__bitvec` | 3 | 6 | 9 |
| 39 | `launchbadge__sqlx` | 2 | 7 | 9 |
| 40 | `tokio-rs__mio` | 8 | 0 | 8 |

## Method

```
rg --no-ignore -g '*.rs' -g '!target' --count-matches -e '\bforget\s*\(' RustProjects
rg --no-ignore -g '*.rs' -g '!target' --count-matches -e '\bleak\s*\('   RustProjects
```
