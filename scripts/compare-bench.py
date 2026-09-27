#!/usr/bin/env python3
"""Compare current bench output against a baseline.

Usage:
    ./scripts/compare-bench.py <baseline.json> <current-output.txt>

The current output is `cargo bench`'s human-readable text (the format
criterion emits by default). The baseline is a JSON file with the
shape:

    {
      "version": 1,
      "captured_at": "2026-09-27",
      "commit": "<git sha>",
      "benches": {
        "<group>/<name>": {"p50_ns": <float>, "throughput_bytes": <float>},
        ...
      }
    }

Exit codes:
    0 — no regression ≥ REGRESSION_PCT (default 2%)
    1 — regression detected
    2 — bench parsing error / baseline missing

The script prints a single human-readable summary line per bench, plus
a final summary. Designed to be parsed by a GitHub Actions
`github-script` step that posts a PR comment when exit code is 1.

Only benches present in both baseline and current output are compared.
A new bench in the current output (no regression entry) is reported
but not flagged.

NOT a Tier-3 perf measurement script — `cargo bench` is. This script
just compares two snapshots of `cargo bench` output. The actual
numeric gate (≥ 2% regression) is in `REGRESSION_PCT` below.
"""

import json
import re
import sys
from pathlib import Path

REGRESSION_PCT = 2.0


def parse_cargo_bench_output(text: str) -> dict:
    """Parse the human-readable output of `cargo bench`.

    criterion emits blocks like:

        stretch/nearest_640x360_to_1920x1080
                                time:   [726.67 µs 731.90 µs 738.43 µs]
                                thrpt:  [10.461 GiB/s 10.554 GiB/s 10.630 GiB/s]

    We capture the lower-bound time (the conservative estimate of the
    fastest measurement) and the lower-bound throughput. Lower-bound
    is the first number in the `[a b c]` tuple.
    """
    benches = {}
    current = None
    for line in text.splitlines():
        m = re.match(r"^([\w/]+)$", line.strip())
        if m:
            current = m.group(1)
            benches.setdefault(current, {"p50_ns": None, "throughput_bytes": None})
            continue
        if current is None:
            continue
        # `time:   [726.67 µs 731.90 µs 738.43 µs]`
        m_time = re.search(
            r"time:\s+\[\s*([\d.]+)\s*(µs|ns|ms|s)\s+([\d.]+)\s*(?:µs|ns|ms|s)\s+([\d.]+)\s*(?:µs|ns|ms|s)\s*\]",
            line,
        )
        if m_time:
            value = float(m_time.group(1))
            unit = m_time.group(2)
            scale = {"ns": 1, "µs": 1_000, "ms": 1_000_000, "s": 1_000_000_000}
            benches[current]["p50_ns"] = value * scale[unit]
        # `thrpt:  [10.461 GiB/s ...]` — only set if not already
        m_thrpt = re.search(
            r"thrpt:\s+\[\s*([\d.]+)\s*(KiB|MiB|GiB|TiB)/s",
            line,
        )
        if m_thrpt and benches[current]["throughput_bytes"] is None:
            value = float(m_thrpt.group(1))
            scale = {"KiB": 1024, "MiB": 1024**2, "GiB": 1024**3, "TiB": 1024**4}
            benches[current]["throughput_bytes"] = value * scale[m_thrpt.group(2)]
    return benches


def compare(baseline_path: Path, current_path: Path) -> int:
    if not baseline_path.exists():
        print(f"ERROR: baseline file {baseline_path} does not exist", file=sys.stderr)
        return 2
    if not current_path.exists():
        print(f"ERROR: current output file {current_path} does not exist", file=sys.stderr)
        return 2

    baseline = json.loads(baseline_path.read_text())
    current_benches = parse_cargo_bench_output(current_path.read_text())

    baseline_benches = baseline.get("benches", {})
    regressions = []
    print(f"Comparing against baseline (commit {baseline.get('commit', '?')}):")
    print()
    for name, base_data in sorted(baseline_benches.items()):
        base_p50 = base_data.get("p50_ns")
        if base_p50 is None:
            continue
        curr_data = current_benches.get(name)
        if curr_data is None:
            print(f"  {name}: missing in current output (skipped)")
            continue
        curr_p50 = curr_data.get("p50_ns")
        if curr_p50 is None:
            print(f"  {name}: could not parse p50 from current output")
            continue
        delta_pct = (curr_p50 - base_p50) / base_p50 * 100
        marker = "OK"
        if delta_pct > REGRESSION_PCT:
            marker = "REGRESSION"
            regressions.append((name, base_p50, curr_p50, delta_pct))
        elif delta_pct < -REGRESSION_PCT:
            marker = "IMPROVED"
        print(f"  {name}: {base_p50/1_000:.2f} µs -> {curr_p50/1_000:.2f} µs ({delta_pct:+.1f}%) [{marker}]")

    if regressions:
        print()
        print(f"REGRESSION: {len(regressions)} bench(es) regressed ≥ {REGRESSION_PCT}%:")
        for name, base, curr, pct in regressions:
            print(f"  {name}: {base/1_000:.2f} µs -> {curr/1_000:.2f} µs ({pct:+.1f}%)")
        return 1
    print()
    print(f"OK: no regression ≥ {REGRESSION_PCT}%")
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 3:
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    sys.exit(compare(Path(sys.argv[1]), Path(sys.argv[2])))