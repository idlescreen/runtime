#!/usr/bin/env python3
"""Compare a criterion run against a recorded baseline.

Usage:
    ./scripts/compare-bench.py <baseline.json> [criterion-dir]

`criterion-dir` defaults to `target/criterion`.

Numbers come from criterion's own machine-readable output —
`<criterion-dir>/<group>/<id>/new/estimates.json` — never from scraped
stdout. The human-readable `time: [a b c]` line is a rendering whose
format has changed between criterion releases, and the previous regex
(`^([\\w/]+)$` to find the bench name) silently matched nothing on some
runs, which is how a compare could report "0 entries" and still exit 0.
Reading the JSON makes an empty result a hard error instead.

The baseline is a JSON file with the shape:

    {
      "version": 2,
      "captured_at": "2026-09-27",
      "commit": "<git sha>",
      "benches": {
        "<group>/<id>": {
          "median_ns": <float>,
          "median_abs_dev_ns": <float>
        },
        ...
      }
    }

`median_ns` is criterion's median point estimate in nanoseconds.
`median_abs_dev_ns` is the median absolute deviation — a cheap, robust
read on how noisy that one measurement was, which is the difference
between "this got 6% slower" and "this runner was busy".

Exit codes:
    0 — no T1 bench past the GATE_PCT catastrophe line. Deltas past the
        5% advisory line are printed and counted, but do not fail.
    1 — a T1 bench regressed past GATE_PCT (65%). This is the only
        verdict that fails the build, and it is deliberately coarse:
        two `ubuntu-latest` runs of identical code differ by a median of
        9.8% and a p90 of 63%, so a 5% gate would false-fail 42 of 73
        benches every run. See GATE_PCT.
    2 — baseline missing, criterion dir missing, or zero entries found
"""

import json
import sys
from pathlib import Path

# The advisory line: a delta past this is worth a human looking, and is
# printed in bold terms, but does not fail the build on its own.
ADVISORY_PCT = 5.0

# The only line that fails the build: a T1 bench that got twice as slow.
#
# A doubling is not a performance target. It is roughly the ceiling of
# what GitHub-hosted runners can distinguish, and it is here because
# the alternative is a gate that lies.
#
# Two `ubuntu-latest` runs of byte-identical code, captured back to back
# with no commit in between, differed by a median of 9.8% across the 73
# T1 benches, with a p90 of 63.0% and a worst case of +94.5%. 42 of
# them moved more than 5%. A 5% threshold therefore false-fails the
# majority of the suite on every run, which is not a gate, it is a coin
# flip with extra steps.
#
# The tell is uniformity rather than size: `letterbox/nearest_*` moved
# +63.0% to +63.4% across nine different resolutions. No code change
# produces identical percentages at every size; a different host CPU
# does. `letterbox/linear_*`, on the same runner, agreed to within 1%.
#
# So the split is deliberate:
#   - under ADVISORY_PCT       -> `ok`
#   - over ADVISORY_PCT        -> `ADVISORY` (loud, non-blocking)
#   - at or over GATE_PCT      -> `REGRESSION` (build fails)
#
# What this still catches is the class of change that matters at this
# stage of the project: a T1 path accidentally made quadratic, a cache
# that stopped hitting, a SIMD fast path that silently fell back to
# scalar. Those are 2x-and-up changes, and a doubling threshold will
# not hide them.
#
# Caveat, stated because it bounds how much this is worth: this is
# calibrated on one pair of runs. The number is the right order of
# magnitude, not a measured distribution. Tightening it needs a
# self-hosted, pinned-hardware runner, and until one exists a tighter
# figure here would be a claim the hardware cannot support.
GATE_PCT = 100.0

# Criterion group names that are T1 — the only ones the gate may fail
#
# Everything else in the baseline (T2 groups such as `stretch_cache`,
# `consume_events`, `power_watcher`) is measured and reported but never
# gated: a T1 target may run T2 benches as a side effect of being one
# crate's `[[bench]]` target, and §5 promises T2 is "benched on demand,
# not gated". Gating them anyway would make the rule a lie in the
# direction that costs the most CI time.
T1_GROUPS = (
    "stretch",
    "letterbox",
    "stretch_u32_rows",
    "stretch_byte_rows",
    "bilinear_row",
    "bilinear_avx2",
    "render_content_viewport",
    "apply_fade_in",
)

# A percentage delta only carries information when the number it is
# computed from is larger than the noise floor of the clock that measured
# it. `apply_fade_in/past_500ms_noop` baselines at 0.5ns -- a fraction of
# a single tick -- and the first real CI run reported it as
# `0.00 -> 0.00 us (+26.7%)` and failed the build on it. That is not a
# slowdown; it is a different rounding of zero, and it can only ever
# produce false failures.
#
# The floor is deliberately low, not 1us. Plenty of T1 benches are
# genuinely fast -- savers' `chaos_update` is 72ns and the bilinear SIMD
# paths are 38ns -- and those are exactly the pages most worth watching.
# A 1us floor would have silently un-gated all of them. 10ns separates
# the degenerate no-ops (0.5ns, 0.7ns, 2.1ns, 6.2ns) from the merely
# quick without exempting a single real measurement.
#
# Applied to the *current* median only: a bench that was 0.5ns and is
# now 5us really did start doing work, and that must stay gated.
MIN_GATED_MEDIAN_NS = 10.0

# Per-group advisory thresholds, matched left-to-right on the bench name
# prefix. This is the "worth a look" line, not the build-failing one —
# see GATE_PCT above. All T1 groups under the tier table in
BENCH_THRESHOLDS = (
    ("letterbox/", 5.0),
    ("stretch/", 5.0),
    ("render_content_viewport/", 5.0),
    ("apply_fade_in/", 5.0),
    ("stretch_u32_rows/", 5.0),
    ("stretch_byte_rows/", 5.0),
    ("bilinear_row/", 5.0),
    ("bilinear_avx2/", 5.0),
)


def is_gated(name: str) -> bool:
    """True when this bench is T1 and may therefore fail the build."""
    return name.split("/", 1)[0] in T1_GROUPS


def threshold_for(name: str) -> float:
    for prefix, pct in BENCH_THRESHOLDS:
        if name.startswith(prefix):
            return pct
    return ADVISORY_PCT


def read_criterion(criterion_dir: Path) -> dict:
    """Collect `new/estimates.json` into `{name: {median_ns, ...}}`.

    A bench's name is its path under the criterion dir with the trailing
    `new/estimates.json` removed, so `stretch/nearest_320x180_to_1920x1080`
    stays identical between baseline and run.
    """
    if not criterion_dir.is_dir():
        print(f"ERROR: criterion dir {criterion_dir} does not exist", file=sys.stderr)
        print("       did `cargo bench` run, and did it use the default target dir?",
              file=sys.stderr)
        return None

    benches = {}
    for path in sorted(criterion_dir.rglob("estimates.json")):
        # `base/` is the previous run and `change/` is the diff; only
        # `new/` describes the run that just happened.
        if path.parent.name != "new":
            continue
        try:
            data = json.loads(path.read_text())
            median = data["median"]["point_estimate"]
            mad = data.get("median_abs_dev", {}).get("point_estimate")
        except (OSError, ValueError, KeyError) as e:
            print(f"WARNING: skipping unreadable {path}: {e}", file=sys.stderr)
            continue
        name = str(path.parent.parent.relative_to(criterion_dir))
        benches[name] = {"median_ns": median, "median_abs_dev_ns": mad}

    return benches


def compare(baseline_path: Path, criterion_dir: Path) -> int:
    if not baseline_path.exists():
        print(f"ERROR: baseline file {baseline_path} does not exist", file=sys.stderr)
        return 2

    current = read_criterion(criterion_dir)
    if current is None:
        return 2
    if not current:
        # The failure class this script exists to make loud: a compare
        # that finds nothing must never look like a pass.
        print(f"ERROR: no `new/estimates.json` under {criterion_dir}", file=sys.stderr)
        print("       a bench run produced no parseable results; refusing to",
              file=sys.stderr)
        print("       report this as 'no regression'.", file=sys.stderr)
        return 2

    try:
        baseline = json.loads(baseline_path.read_text())
    except (OSError, ValueError) as e:
        print(f"ERROR: baseline {baseline_path} is not valid JSON: {e}", file=sys.stderr)
        return 2

    baseline_benches = baseline.get("benches", {})
    if not baseline_benches:
        print(f"ERROR: baseline {baseline_path} has no `benches` entries", file=sys.stderr)
        return 2

    regressions = []
    advisory = []
    compared = 0
    ungated = 0
    inconclusive = []
    below_floor = []
    print(f"Baseline commit {baseline.get('commit', '?')}, "
          f"captured {baseline.get('captured_at', '?')}")
    print(f"Current run: {len(current)} bench(es) under {criterion_dir}")
    print()

    for name, base_data in sorted(baseline_benches.items()):
        base_p50 = base_data.get("median_ns")
        if base_p50 is None:
            print(f"  {name}: no median in baseline (skipped)")
            continue
        curr = current.get(name)
        if curr is None:
            print(f"  {name}: missing from this run (skipped)")
            continue
        curr_p50 = curr["median_ns"]
        delta_pct = (curr_p50 - base_p50) / base_p50 * 100
        threshold = threshold_for(name)
        mad = curr.get("median_abs_dev_ns")
        noise_pct = (mad / curr_p50 * 100) if mad else None
        noise = f", noise ±{noise_pct:.1f}%" if noise_pct is not None else ""
        gated = is_gated(name)
        compared += 1
        if not gated:
            # Reported for information; deliberately cannot fail the build.
            ungated += 1
            marker = "not gated (T2/T3)"
        elif curr_p50 < MIN_GATED_MEDIAN_NS:
            # The measurement itself is too small for a percentage to mean
            # anything. Still printed, with its real numbers, so a human can
            # see it -- just never allowed to fail the build on its own.
            below_floor.append(name)
            marker = f"not gated (median {curr_p50:.1f}ns below {MIN_GATED_MEDIAN_NS:.0f}ns floor)"
        elif noise_pct is not None and noise_pct > threshold:
            # The measurement is too noisy to support any verdict. A
            # ±54% median absolute deviation measured against a 5%
            # threshold is a coin flip, and calling it "ok" is a claim
            # this run cannot support — §6 asks for the noise, not just
            # the delta. Inconclusive is not a pass and not a failure:
            # it says the bench needs a quieter machine before it can
            # gate anything.
            inconclusive.append((name, noise_pct, threshold))
            marker = f"INCONCLUSIVE (noise ±{noise_pct:.1f}% > {threshold}%)"
        elif delta_pct > GATE_PCT:
            marker = f"REGRESSION (> {GATE_PCT:.0f}%)"
            regressions.append((name, base_p50, curr_p50, delta_pct, GATE_PCT))
        elif delta_pct > threshold:
            # Past the advisory line but inside the noise this hardware
            # produces on its own. Reported loudly, never fatal: two runs
            # of identical code differ by a median of 9.8% and a p90 of
            # 63% here, so a sub-65% delta is not yet evidence of a
            # code change. See GATE_PCT.
            advisory.append((name, base_p50, curr_p50, delta_pct, threshold))
            marker = f"ADVISORY (> {threshold}%, under the {GATE_PCT:.0f}% gate)"
        elif delta_pct < -threshold:
            marker = "IMPROVED"
        else:
            marker = "ok"
        print(f"  {name}: {base_p50/1000:.2f} -> {curr_p50/1000:.2f} us "
              f"({delta_pct:+.1f}%{noise}) [{marker}]")

    print()
    if compared == 0:
        print("ERROR: baseline and current run share no bench names", file=sys.stderr)
        print(f"       baseline has {len(baseline_benches)}, run has {len(current)}",
              file=sys.stderr)
        return 2

    if regressions:
        print(f"REGRESSION: {len(regressions)} of {compared} bench(es) past the "
              f"{GATE_PCT:.0f}% gate:")
        for name, base, curr, pct, threshold in regressions:
            print(f"  {name}: {base/1000:.2f} -> {curr/1000:.2f} us "
                  f"({pct:+.1f}%, gate {threshold:.0f}%)")
        return 1

    gated_n = compared - ungated - len(inconclusive) - len(below_floor)
    parts = [f"{gated_n} gated T1 bench(es) compared, "
             f"none past the {GATE_PCT:.0f}% gate"]
    if advisory:
        parts.append(
            f"{len(advisory)} ADVISORY past the 5% line but inside the "
            f"{GATE_PCT:.0f}% gate — reported, not blocking, because two "
            f"runs of identical code on these runners differ by a median of "
            f"9.8%: " + ", ".join(n for n, _, _, _, _ in advisory)
        )
    if inconclusive:
        parts.append(
            f"{len(inconclusive)} T1 bench(es) INCONCLUSIVE (run-to-run noise "
            f"exceeds the threshold, so they cannot gate anything on this "
            f"hardware): " + ", ".join(n for n, _, _ in inconclusive)
        )
    if below_floor:
        parts.append(
            f"{len(below_floor)} T1 bench(es) below the "
            f"{MIN_GATED_MEDIAN_NS:.0f}ns floor (a percentage on them is "
            f"rounding noise): " + ", ".join(below_floor)
        )
    if ungated:
        parts.append(f"{ungated} T2 bench(es) reported but not gated")
    print("OK: " + "; ".join(parts))
    return 0


if __name__ == "__main__":
    if len(sys.argv) not in (2, 3):
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    crit = Path(sys.argv[2]) if len(sys.argv) == 3 else Path("target/criterion")
    sys.exit(compare(Path(sys.argv[1]), crit))
