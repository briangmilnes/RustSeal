<style>
body, main, .markdown-body { max-width: 95vw !important; width: 95vw !important; }
table { width: 100% !important; border-collapse: collapse; }
td, th { padding: 3px 7px; vertical-align: top; }
</style>

# Existing Vec Benchmarks (faithful port)

Faithful 1:1 Divan port of the real Rust suite `library/alloctests/benches/vec.rs`
(Rust 1.97.0) — `benches/vec/real_vec.rs`, run 2026-07-04. All 101 real `#[bench]`; the
`in_place` type-variant family expands to **118 Divan functions** across u8/u32/u128, exactly as
upstream. Real element types preserved (u8/u32/u128/usize/i32 and a custom `Droppable`).
`std::VVec` vs `lazy_loss_recovery::VVec`; `ratio = lazy / std`, `fastest` of 100 samples.

Aggregates over the **105 real** benches (13 bogus excluded):
**median 1.05 · average 1.18 · time-weighted 1.01.** By wall-clock the whole suite is ~1% slower;
the large-payload ops are parity and the cost is a small fixed per-op tax at small sizes.
(Per-bench rows: see `benches/vec/real_vec.rs`.)

## By family

| # | family (count) | elem | verdict |
|--:|----------------|------|---------|
| 1 | `new` (1) | u32 | **bogus** — empty ctor, timer floor |
| 2 | `with_capacity_{0000,0010,0100,1000}` (4) | u32 | real; `_0000` bogus (no alloc) |
| 3 | `from_fn_*` / `from_elem_*` / `from_slice_*` / `from_iter_*` (4×4) | usize | real; each `_0000` bogus (empty) |
| 4 | `extend_*` (7) / `extend_from_slice_*` (7) | usize | real; `_0000_0000` bogus |
| 5 | `extend_recycle` (1) | i32 | real |
| 6 | `clone_{0000,0010,0100,1000}` (4) | usize | real; `_0000` bogus |
| 7 | `clone_from_{01,10}_*_*` (24) | usize | real; the 4 with src-payload `_0000` bogus |
| 8 | `in_place_{xxu8,xu32,u128}_*` (18) | u8/u32/u128 | real |
| 9 | `in_place_recycle` / `_zip_recycle` / `_zip_iter_mut` / `transmute` / `_collect_droppable` (5) | usize/u8/u32/Droppable | real |
| 10 | `chain_collect` / `chain_chain_collect` / `nest_chain_chain_collect` (3) | i32 | real |
| 11 | `range_map_collect` / `chain_extend_ref` / `chain_extend_value` / `rev_1` / `rev_2` / `map_regular` / `map_fast` (7) | u32 | real |
| 12 | `dedup_{slice_truncate,random,none,all}_{100,1000,10000,100000}` (16) | u32 | real |
| 13 | `flat_map_collect` / `retain_iter_100000` / `retain_100000` / `retain_whole_100000` / `next_chunk` (5) | u8/i32/u32 | real |

## Bogus (13)

All size-0 / empty-payload family members — their ratios are timer/allocator floor, not signal:
`new`, `with_capacity_0000`, `from_fn_0000`, `from_elem_0000`, `from_slice_0000`, `from_iter_0000`,
`clone_0000`, `extend_0000_0000`, `extend_from_slice_0000_0000`, and the 4 `clone_from_*_*_0000`
(clone_from with a zero-element source). Their non-zero siblings are all real.

## Top real costs

| bench | lazy/std | note |
|-------|---------:|------|
| `from_slice_0010` | 2.28 | 10-element construct — the lazy `pending` field's fixed cost on ~10 ns of work (absolute +13 ns) |
| `from_iter_0010` | 1.99 | same, 10-element collect |
| `clone_from_10_*_0010` | ~1.9 | 10× clone_from into ~10 elements — per-call lazy overhead |
| `in_place_u128_1000_i0` | 1.41 | u128 in-place collect (larger element, more copy) |

The large-payload ops are parity: `dedup_random_100000` 1.00, `flat_map_collect` 1.00,
`retain_100000` ~1.04, `clone_1000` ~1.02, `extend_1000_1000` ~1.02, `map_fast`/`map_regular` 1.00.
The lazy variant's cost is the fixed boxed-`pending` field, visible only where the payload is tiny;
it vanishes by ~1000 elements — which is why median is 1.05 but time-weighted is 1.01.
