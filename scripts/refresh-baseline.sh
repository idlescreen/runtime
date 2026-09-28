#!/usr/bin/env bash
# Regenerate `perf-baseline.json` from a fresh `cargo bench` run.
#
# Usage:
#   scripts/refresh-baseline.sh                # capture all benches
#   scripts/refresh-baseline.sh stretch        # capture stretch only
#   scripts/refresh-baseline.sh draw_frame     # capture draw_frame only
#   scripts/refresh-baseline.sh --comment [stretch|draw_frame]
#                                           # refresh + open a PR via gh CLI
#
# The script writes the new baseline to `perf-baseline.json.tmp` and
# (after a sanity check on JSON validity + delta vs the current
# baseline) renames it over `perf-baseline.json`. The git commit is
# left to the maintainer so the change is attributed in git history
# (except in --comment mode, where the PR branch is pushed directly).
#
# Reads the git HEAD sha automatically so the baseline.json records
# which commit the numbers came from.
set -euo pipefail

cd "$(dirname "$0")/.."

OPEN_PR=0
GROUP=""
while [ $# -gt 0 ]; do
    case "$1" in
        --comment) OPEN_PR=1; shift ;;
        --help|-h)
            sed -n '2,12p' "$0"
            exit 0
            ;;
        stretch|draw_frame|"") GROUP="$1"; shift ;;
        *)
            echo "Usage: $0 [--comment] [stretch|draw_frame]" >&2
            exit 2
            ;;
    esac
done

BENCH_ARGS=""
case "$GROUP" in
    "")          BENCH_ARGS="-p idle-upscaler --bench stretch -- -q --warm-up-time 1 --measurement-time 3" ;;
    stretch)     BENCH_ARGS="-p idle-upscaler --bench stretch -- -q --warm-up-time 1 --measurement-time 3" ;;
    draw_frame)  BENCH_ARGS="-p idle-daemon --bench draw_frame -- -q --warm-up-time 1 --measurement-time 3" ;;
    *)           echo "Usage: $0 [--comment] [stretch|draw_frame]" >&2; exit 2 ;;
esac

OUT=$(mktemp)
trap "rm -f $OUT" EXIT

echo "Capturing bench output (this can take a few minutes)…"
cargo bench $BENCH_ARGS > "$OUT" 2>&1 || true

# Run compare-bench.py in dry-run mode (we don't have a baseline yet)
# — instead, parse the output and emit fresh JSON.
COMMIT=$(git rev-parse --short=7 HEAD)
CAPTURED_AT=$(date -u +%Y-%m-%d)

python3 - "$OUT" "$COMMIT" "$CAPTURED_AT" <<'PY'
import json, re, sys
from pathlib import Path

text_path, commit, captured_at = sys.argv[1], sys.argv[2], sys.argv[3]
text = Path(text_path).read_text()

benches = {}
current = None
scale_unit = {"ns": 1, "µs": 1_000, "ms": 1_000_000, "s": 1_000_000_000}
scale_thrpt = {"KiB": 1024, "MiB": 1024**2, "GiB": 1024**3, "TiB": 1024**4}

for line in text.splitlines():
    m = re.match(r"^([\w/]+)$", line.strip())
    if m:
        current = m.group(1)
        benches.setdefault(current, {"p50_ns": None, "throughput_bytes": None})
        continue
    if current is None:
        continue
    m_time = re.search(
        r"time:\s+\[\s*([\d.]+)\s*(µs|ns|ms|s)\s+([\d.]+)\s*(?:µs|ns|ms|s)\s+([\d.]+)\s*(?:µs|ns|ms|s)\s*\]",
        line,
    )
    if m_time:
        value = float(m_time.group(1))
        benches[current]["p50_ns"] = value * scale_unit[m_time.group(2)]
    m_thrpt = re.search(r"thrpt:\s+\[\s*([\d.]+)\s*(KiB|MiB|GiB|TiB)/s", line)
    if m_thrpt and benches[current]["throughput_bytes"] is None:
        value = float(m_thrpt.group(1))
        benches[current]["throughput_bytes"] = value * scale_thrpt[m_thrpt.group(2)]

# Drop benches with no p50 (parse failures)
benches = {k: v for k, v in benches.items() if v["p50_ns"] is not None}

out = {
    "version": 1,
    "captured_at": captured_at,
    "commit": commit,
    "note": "Captured locally via scripts/refresh-baseline.sh",
    "benches": benches,
}
Path("perf-baseline.json.tmp").write_text(json.dumps(out, indent=2, sort_keys=True) + "\n")
print(f"Wrote {len(benches)} bench entries to perf-baseline.json.tmp")
PY

# Sanity check: if there's an existing baseline, show the delta.
if [ -f perf-baseline.json ]; then
    echo
    echo "Delta vs current perf-baseline.json (informational, not enforced):"
    python3 scripts/compare-bench.py perf-baseline.json "$OUT" \
        || true  # regressions are reported but don't block the refresh
fi

mv perf-baseline.json.tmp perf-baseline.json

if [ "$OPEN_PR" = "0" ]; then
    echo
    echo "Wrote perf-baseline.json. Review the diff, then:"
    echo "  git add perf-baseline.json && git commit -m 'runtime: refresh perf baseline'"
    exit 0
fi

# --comment mode: branch + push + open a PR with gh CLI.
if ! command -v gh >/dev/null 2>&1; then
    echo "ERROR: --comment requires the gh CLI on PATH" >&2
    exit 1
fi
if ! gh auth status >/dev/null 2>&1; then
    echo "ERROR: --comment requires an authenticated gh session" >&2
    exit 1
fi

BRANCH="perf-baseline-${CAPTURED_AT}"
git checkout -b "$BRANCH" >/dev/null
git add perf-baseline.json
if git diff --cached --quiet; then
    echo "No baseline change to commit — exiting."
    git checkout - >/dev/null
    git branch -D "$BRANCH" >/dev/null
    exit 0
fi
git -c user.name=jeryd -c user.email=jeryd@users.noreply.github.com \
    commit -m "runtime: refresh perf baseline ($CAPTURED_AT)" \
            -m "Captured by scripts/refresh-baseline.sh --comment on commit $COMMIT." >/dev/null
git -c user.name=jeryd -c user.email=jeryd@users.noreply.github.com \
    push --set-upstream origin "$BRANCH" 2>&1 | tail -3
PR_URL=$(gh pr create \
    --base master \
    --head "$BRANCH" \
    --title "runtime: refresh perf baseline ($CAPTURED_AT)" \
    --body "Automated baseline update via \`scripts/refresh-baseline.sh --comment\`.

Bench numbers captured against \`$COMMIT\`. See \`scripts/compare-bench.py perf-baseline.json <current-bench-output>\` for deltas vs the prior baseline.")
echo
echo "Opened PR: $PR_URL"