# labs

Isolated experiment scratch areas, in the style of `~/projects/CSTs/labs`.
Experiments may fail to compile or verify, so the labs are kept out of the main
RustSeal workspace build. Each experiment file carries a `RESULT:` status marker
(CLAUDE.md rule 16.2); a failed experiment is left in place as documentation, not
edited to pass (rule 16.3).

| # | dir | role |
|---|-----|------|
| 1 | `verus-lab` | single-file verus experiments, verified directly by the verus binary (no cargo build). |

See each lab's own `README.md` for layout.
