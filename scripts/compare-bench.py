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
    0 — no regression past the per-group threshold
    1 — regression detected
    2 — baseline missing, criterion dir missing, or zero entries found
"""

import json
import sys
from pathlib import Path

# Default threshold for any bench with no specific rule below.
#
# 5% is deliberately not tight. GitHub-hosted runners are shared and
# noisy; a 2% gate produces red builds that mean nothing. A tighter
# threshold belongs on a self-hosted, pinned-hardware runner, not here.
REGRESSION_PCT = 5.0

# Criterion group names that are T1 — the only ones the gate may fail
# the build on. See the tier table in .github/RULES.md §5.
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

# Per-group thresholds, matched left-to-right on the bench name prefix.
# All T1 groups under the tier table in .github/RULES.md §4.
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
    return REGRESSION_PCT


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
    compared = 0
    ungated = 0
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
        noise = f", noise ±{(mad / curr_p50 * 100):.1f}%" if mad else ""
        gated = is_gated(name)
        compared += 1
        if not gated:
            # Reported for information; deliberately cannot fail the build.
            ungated += 1
            marker = "not gated (T2/T3)"
        elif delta_pct > threshold:
            marker = f"REGRESSION (> {threshold}%)"
            regressions.append((name, base_p50, curr_p50, delta_pct, threshold))
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
        print(f"REGRESSION: {len(regressions)} of {compared} bench(es) past threshold:")
        for name, base, curr, pct, threshold in regressions:
            print(f"  {name}: {base/1000:.2f} -> {curr/1000:.2f} us "
                  f"({pct:+.1f}%, threshold {threshold}%)")
        return 1

    gated_n = compared - ungated
    print(f"OK: {gated_n} gated T1 bench(es) compared, no regression past threshold"
          + (f"; {ungated} T2 bench(es) reported but not gated" if ungated else ""))
    return 0


if __name__ == "__main__":
    if len(sys.argv) not in (2, 3):
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    crit = Path(sys.argv[2]) if len(sys.argv) == 3 else Path("target/criterion")
    sys.exit(compare(Path(sys.argv[1]), crit))
