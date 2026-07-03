#!/usr/bin/env python3
"""compare_safe_unsafe.py — parse rustseal's committed GRASE bench/test logs and emit an
N-way comparison (bench-ratio table + test-parity table) across the implementation
*variants* of each base data structure.

Data-driven: no base/module name is hardcoded. A source-file stem is split into
(variant, base) by stripping a known variant prefix (longest match wins), e.g.

    unsafe_binary_heap    -> variant `unsafe`,   base `binary_heap`
    safe_binary_heap      -> variant `safe`,     base `binary_heap`
    safe_opt_binary_heap  -> variant `safe_opt`, base `binary_heap`

All variants of a base are then shown side by side, with each non-reference variant's
ratio to the reference variant (`unsafe`). Adding a new base needs zero edits; adding a
new *variant* needs one line in VARIANT_PREFIXES (+ optionally PREFERRED_ORDER).

Pure stdlib. Invoked by scripts/compare-safe-unsafe; can also be run directly.

Log line formats (stable, see r0437 recon):
  header  :  `     Running benches/<file>.rs (target/.../deps/<bin>)`
             `     Running tests/<file>.rs   (target/.../deps/<bin>)`
  bench   :  `test <name> ... bench:     <N> ns/iter (+/- <D>)`  (N, D have commas)
  test sum:  `test result: ok. P passed; F failed; K ignored; M measured; ... `
"""
import argparse
import math
import re
import sys

# ---- line patterns -------------------------------------------------------------
RE_RUNNING = re.compile(r"^\s*Running\s+(\S+?\.rs)\b")
RE_BENCH = re.compile(
    r"^\s*test\s+(\S+)\s+\.\.\.\s+bench:\s+([\d,]+(?:\.\d+)?)\s+ns/iter"
    r"(?:\s+\(\+/-\s+([\d,]+(?:\.\d+)?)\))?"
)
RE_RESULT = re.compile(
    r"^\s*test result:\s+\w+\.\s+(\d+)\s+passed;\s+(\d+)\s+failed;\s+(\d+)\s+ignored"
)

# Variant prefixes, matched longest-first (so `safe_opt_` wins over `safe_`). Add a new
# implementation variant here and it shows up as its own column automatically.
VARIANT_PREFIXES = ("unsafe_nopanic_", "unsafe_", "safe_but_for_index_", "safe_opt_", "safe_")

# Display/column order for variants; the reference (first) is the ratio denominator.
PREFERRED_ORDER = ("unsafe", "unsafe_nopanic", "safe", "safe_opt", "safe_but_for_index")
REF = "unsafe"


def split_variant(stem):
    """Split a file stem into (base, variant) by longest matching variant prefix.
    Returns (None, None) if no known variant prefix matches."""
    for pref in sorted(VARIANT_PREFIXES, key=len, reverse=True):
        if stem.startswith(pref):
            return stem[len(pref):], pref[:-1]  # drop trailing '_'
    return None, None


def order_variants(variants):
    """Order a set of variant names: PREFERRED_ORDER first, then the rest alphabetically."""
    pos = {v: i for i, v in enumerate(PREFERRED_ORDER)}
    return sorted(variants, key=lambda v: (pos.get(v, len(PREFERRED_ORDER)), v))


def num(s):
    """Parse a possibly-comma-grouped number into a float."""
    return float(s.replace(",", ""))


def fmt_ns(v):
    if v is None:
        return "—"
    return "{:,.2f}".format(v)


def fmt_ratio(v):
    if v is None:
        return "—"
    return "{:.2f}".format(v)


def geomean(values):
    """Geometric mean of a list of positive ratios; None if empty."""
    vals = [v for v in values if v is not None and v > 0]
    if not vals:
        return None
    return math.exp(sum(math.log(v) for v in vals) / len(vals))


# ---- parsers -------------------------------------------------------------------
def parse_bench_log(path):
    """Return { base: { variant: { bench_name: (ns, dev) } } } for bench source files."""
    data = {}
    cur_base = cur_var = None
    with open(path, "r", encoding="utf-8", errors="replace") as fh:
        for line in fh:
            m = RE_RUNNING.match(line)
            if m:
                rel = m.group(1)
                if not rel.startswith("benches/"):
                    cur_base = cur_var = None
                    continue
                stem = rel[len("benches/"):-len(".rs")]
                cur_base, cur_var = split_variant(stem)
                continue
            if cur_base is None:
                continue
            mb = RE_BENCH.match(line)
            if mb:
                name = mb.group(1)
                ns = num(mb.group(2))
                dev = num(mb.group(3)) if mb.group(3) else None
                data.setdefault(cur_base, {}).setdefault(cur_var, {})[name] = (ns, dev)
    return data


def parse_test_log(path):
    """Return { base: { variant: (passed, failed, ignored) } } for test source files."""
    data = {}
    cur_base = cur_var = None
    in_test_file = False
    with open(path, "r", encoding="utf-8", errors="replace") as fh:
        for line in fh:
            m = RE_RUNNING.match(line)
            if m:
                rel = m.group(1)
                if not rel.startswith("tests/"):
                    cur_base = cur_var = None
                    in_test_file = False
                    continue
                stem = rel[len("tests/"):-len(".rs")]
                cur_base, cur_var = split_variant(stem)
                in_test_file = cur_base is not None
                continue
            if not in_test_file or cur_base is None:
                continue
            mr = RE_RESULT.match(line)
            if mr:
                p, f, k = int(mr.group(1)), int(mr.group(2)), int(mr.group(3))
                data.setdefault(cur_base, {})[cur_var] = (p, f, k)
                in_test_file = False  # one summary per test file
    return data


# ---- table builders ------------------------------------------------------------
def build_bench_table(bench, bench_log):
    out = []
    out.append("## Bench comparison (variants vs `{}`)".format(REF))
    out.append("")
    out.append("Source: `{}`".format(bench_log))
    out.append("")
    if not bench:
        out.append("(no bench source variants found in log)")
        out.append("")
        return out

    variants = order_variants({v for sides in bench.values() for v in sides})
    non_ref = [v for v in variants if v != REF]

    out.append("Columns:")
    out.append("- `#` — row id.")
    out.append("- `base` — data-structure name (source stem minus the variant prefix).")
    out.append("- `bench` — benchmark function name.")
    for v in variants:
        out.append("- `{0} ns/iter` — {0} variant's median ns/iter (`—` = no such bench).".format(v))
    for v in non_ref:
        out.append("- `{0}/{1}` — ratio {0}_ns / {1}_ns (>1 = {0} slower); "
                   "`—` = ref or this variant missing for the row.".format(v, REF))
    out.append("- `note` — `unpaired` when fewer than two variants have the bench; else blank.")
    out.append("")
    out.append("Subtotal/TOTALS cells are the GEOMETRIC mean of that variant's ratios in the "
               "group (geomean, not arithmetic, because these are ratios).")
    out.append("")

    cols = (["#", "base", "bench"]
            + ["{} ns/iter".format(v) for v in variants]
            + ["{}/{}".format(v, REF) for v in non_ref] + ["note"])
    aligns = (["---", "---", "---"]
              + ["---:" for _ in variants] + ["---:" for _ in non_ref] + ["---"])
    out.append("| " + " | ".join(cols) + " |")
    out.append("| " + " | ".join(aligns) + " |")

    idx = 0
    total_ratios = {v: [] for v in non_ref}
    for base in sorted(bench):
        sides = bench[base]
        names = sorted(set().union(*(set(sides.get(v, {})) for v in variants)))
        group_ratios = {v: [] for v in non_ref}
        for name in names:
            idx += 1
            ns = {v: (sides.get(v, {}).get(name) or (None,))[0] for v in variants}
            ref_ns = ns.get(REF)
            present = sum(1 for v in variants if ns[v] is not None)
            ratios = {}
            for v in non_ref:
                if ns[v] is not None and ref_ns is not None and ref_ns > 0:
                    r = ns[v] / ref_ns
                    ratios[v] = r
                    group_ratios[v].append(r)
                    total_ratios[v].append(r)
                else:
                    ratios[v] = None
            note = "" if present >= 2 else "unpaired"
            row = ([str(idx), base, name]
                   + [fmt_ns(ns[v]) for v in variants]
                   + [fmt_ratio(ratios[v]) for v in non_ref] + [note])
            out.append("| " + " | ".join(row) + " |")
        sub = (["", "**{} subtotal**".format(base), "_geomean_"]
               + ["" for _ in variants]
               + ["**{}**".format(fmt_ratio(geomean(group_ratios[v]))) for v in non_ref] + [""])
        out.append("| " + " | ".join(sub) + " |")
    tot = (["", "**TOTALS**", "_geomean_"]
           + ["" for _ in variants]
           + ["**{}**".format(fmt_ratio(geomean(total_ratios[v]))) for v in non_ref] + [""])
    out.append("| " + " | ".join(tot) + " |")
    out.append("")
    return out


def build_parity_table(tests, test_log):
    out = []
    out.append("## Test parity (variants vs `{}`)".format(REF))
    out.append("")
    out.append("Source: `{}`".format(test_log))
    out.append("")
    if not tests:
        out.append("(no test source variants found in log)")
        out.append("")
        return out

    variants = order_variants({v for sides in tests.values() for v in sides})

    out.append("Columns:")
    out.append("- `#` — row id.")
    out.append("- `base` — data-structure name.")
    for v in variants:
        out.append("- `{0} P/F/K` — {0} variant's passed/failed/ignored (`—` = no such test file).".format(v))
    out.append("- `parity` — `Y` iff no present variant has a test FAILURE and all run the "
               "same total number of tests (so any difference is only which tests are "
               "`#[ignore]`d, e.g. a variant that drops a guarantee); `N` on any failure or "
               "differing suite size; `—` if `{0}` missing or only one variant.".format(REF))
    out.append("- `note` — `unpaired` (<2 variants); `has failures` / `suite size differs` "
               "explain an `N`; `ignored differ` when parity is `Y` but the `#[ignore]` sets "
               "differ; else blank.")
    out.append("")

    cols = ["#", "base"] + ["{} P/F/K".format(v) for v in variants] + ["parity", "note"]
    aligns = ["---", "---"] + ["---" for _ in variants] + ["---", "---"]
    out.append("| " + " | ".join(cols) + " |")
    out.append("| " + " | ".join(aligns) + " |")

    def pfk(t):
        return "—" if t is None else "{}/{}/{}".format(*t)

    idx = 0
    groups = 0
    parity_yes = 0
    for base in sorted(tests):
        idx += 1
        sides = tests[base]
        ref = sides.get(REF)
        others = [v for v in variants if v != REF and sides.get(v) is not None]
        if ref is not None and others:
            groups += 1
            present = [REF] + others
            any_fail = any(sides[v][1] > 0 for v in present)
            ref_total = sum(ref)
            same_total = all(sum(sides[v]) == ref_total for v in others)
            if any_fail:
                parity, note = "N", "has failures"
            elif not same_total:
                parity, note = "N", "suite size differs"
            else:
                parity = "Y"
                parity_yes += 1
                note = "" if all(sides[v][2] == ref[2] for v in others) else "ignored differ"
        else:
            parity = "—"
            note = "unpaired"
        row = [str(idx), base] + [pfk(sides.get(v)) for v in variants] + [parity, note]
        out.append("| " + " | ".join(row) + " |")
    out.append("| " + " | ".join(
        ["", "**TOTALS**"] + ["" for _ in variants]
        + ["**{}/{} Y**".format(parity_yes, groups), ""]) + " |")
    out.append("")
    return out


def main():
    ap = argparse.ArgumentParser(
        description="N-way compare rustseal heap variants (unsafe/safe/safe_opt/…) from "
                    "committed GRASE logs.")
    ap.add_argument("--bench-log", required=True, help="path to a bench-rustseal-*.log")
    ap.add_argument("--test-log", required=True, help="path to a test-rustseal-*.log")
    args = ap.parse_args()

    bench = parse_bench_log(args.bench_log)
    tests = parse_test_log(args.test_log)

    lines = []
    lines.append("# Variant comparison — rustseal")
    lines.append("")
    lines.append("Generated by `scripts/compare-safe-unsafe` from committed GRASE logs "
                 "(no benches/tests re-run).")
    lines.append("")
    lines += build_bench_table(bench, args.bench_log)
    lines += build_parity_table(tests, args.test_log)

    sys.stdout.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
