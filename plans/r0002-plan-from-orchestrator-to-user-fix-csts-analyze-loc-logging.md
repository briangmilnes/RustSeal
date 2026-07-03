---
round: r0002
from: orchestrator
to: user
subject: fix-csts-analyze-loc-logging
date: 2026-0703-14:05
status: pending
---

# Fix `csts-analyze-loc` logging to conform to `LoggingStandard.md`

**For:** the CSTs orchestrator (this work is in `~/projects/CSTs`, outside RustSeal;
the user is the courier — GRASE rule 1.4).

**Authored by:** RustSeal orchestrator, after auditing the tool against
`~/projects/GRASE/standards/LoggingStandard.md` and `BinaryStandard.md`. Directives
below reflect the user's rulings on the audit.

## Governing principle

**The rust program owns its logging; the wrapper does not control it.** The binary
self-generates ONE second-resolution stamp (`chrono::Local::now().format("%Y%m%d-%H%M%S")`)
at startup and always writes its timing log with that stamp. The wrapper must not
inject a timestamp or gate the program's logging. The wrapper's only logging
responsibility is teeing its own build/exec output.

## Files

- `datastructs/vst-lib/src/bin/csts-analyze-loc.rs` — the binary (fixes B1, B2).
- `scripts/products/generate-analysis-loc` — the CSTs wrapper (fix W2).

The two **destinations** are already correct (timing telemetry → `logs/`, the
markdown table → `analyses/`). The fixes are filename format + removing the
wrapper's control of the program's logging.

## Fixes

### B1 — timing-log filename separator `.` → `-`

`timing::init` (line ~107) builds `format!("logs/csts-analyze-loc.{ts}.{}.log", role())`.
The `LoggingStandard.md` filename form is `<program>-YYYYMMDD-HHMMSS.log` — a `-`
before the stamp, not a `.`. Change to
`format!("logs/csts-analyze-loc-{ts}.{}.log", role())`.

→ verify: a run emits `logs/csts-analyze-loc-<YYYYMMDD-HHMMSS>.<role>.log`.

### B2 — the binary must ALWAYS write its timing log, self-stamped

Currently `timing::init` (line ~106) only opens a file when the `CSTS_ANALYSIS_TS`
env var is set (`std::env::var("CSTS_ANALYSIS_TS").ok().and_then(...)`), so a raw /
direct run persists NO `logs/` telemetry — stderr only. This env-var gating is a
design smell (it lets the wrapper control the program's logging — the opposite of
the governing principle above; its origin is unclear and it should go).

Fix: the binary self-generates one second-resolution stamp at startup and ALWAYS
opens and writes `logs/csts-analyze-loc-<ts>.<role>.log`, per the `LoggingStandard.md`
Rust implementation pattern (`init_execution_log`). Remove the `CSTS_ANALYSIS_TS`
dependency entirely — do not honour it as an override; the program owns the stamp.
The same self-stamp fills the "Generated `<ts>`" line in the markdown body (which
is currently blank when the env var is unset).

→ verify: `./datastructs/target/release/csts-analyze-loc -d some/dir` with NO env
var set writes `logs/csts-analyze-loc-<ts>.<role>.log` AND the markdown carries a
real "Generated `<ts>`" line.

### W2 — the wrapper stops controlling logging; one stamp per run

`scripts/products/generate-analysis-loc` currently injects
`CSTS_ANALYSIS_TS="$(date +%Y%m%d-%H%M%S)"` (line 37) and makes three independent
`date` calls, so one run's artifacts can carry three different stamps. Fix:

- Delete the `CSTS_ANALYSIS_TS=...` injection — the binary now self-stamps (B2).
- The wrapper keeps teeing its OWN build/exec output to
  `logs/generate-analysis-loc-<ts>.<role>.log`. **Minute resolution on this
  wrapper exec log is acceptable** (user ruling — the exec log is the wrapper's own
  telemetry, not the program's logging). The former W1 "bump to seconds" item is
  withdrawn.

→ verify: the wrapper sets no `CSTS_ANALYSIS_TS`; the program's timing log and its
analytical output share the program's single self-stamp.

## Open decisions (for the CSTs orchestrator)

1. **Who names / writes the analytical output?** Today the wrapper passes
   `-o analyses/csts-analyze-loc-<ts>.md`, so the wrapper stamps and places the
   data product. Under "the program owns its logging," the program would self-name
   it (one stamp shared with the timing log). But `LoggingStandard.md`'s Rust
   pattern has the tool write `analyses/` itself, which collides with GRASE rule 8.4
   ("agents do not write `analyses/`"). Reconcile: either (a) the program self-names
   and writes `analyses/…` (drop `-o` from the wrapper; supersede rule 8.4 with the
   LoggingStandard), or (b) keep rule 8.4 and accept that the wrapper-placed output
   and the program-stamped timing log won't share a stamp.
2. **Analyses extension `.md` vs `.log`.** The output is markdown (embeds a `<style>`
   block + table). GRASE rule 8.1 permits `.md`; `LoggingStandard.md` §"Two
   destinations" writes analyses as `.log`. The two GRASE docs disagree — pick one.

## RustSeal follow-ups (after the CSTs report returns)

- RustSeal orchestrator rewrites `RustSeal/scripts/count-loc.sh` (today it writes a
  deprecated static filename and persists no timing/exec log).
- RustSeal's `scripts/generate-analysis-loc.sh` drops its interim
  `CSTS_ANALYSIS_TS` bridge once the binary self-logs (B2).

## Reference

`RustSeal/scripts/generate-analysis-loc.sh` is a working wrapper. It currently sets
`CSTS_ANALYSIS_TS` only as an INTERIM bridge (so today's env-gated binary still
writes a correlated timing log); that line is removed once B2 lands.
