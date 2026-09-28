#!/usr/bin/env bash
# Regenerate `perf-baseline.json` from a fresh `cargo bench` run.
#
# Usage:
#   scripts/refresh-baseline.sh                # capture every T1 target
#   scripts/refresh-baseline.sh stretch        # capture one target
#   scripts/refresh-baseline.sh --comment [stretch|draw_frame]
#                                           # refresh + open a PR via gh CLI
#
# Numbers are read from criterion's own JSON — `target/criterion/*/new/
# estimates.json` — never from scraped stdout. Only files written after
# this script started are collected, so a stale directory left behind by
# a bench that was deleted or renamed cannot leak into the baseline.
#
# T1 targets are the ones perf.yml runs and the gate compares; see the
# tier table in .github/RULES.md §4. T2 benches are deliberately not
# baselined — they are on-demand, not gated.
set -euo pipefail

cd "$(dirname "$0")/.."

OPEN_PR=0
GROUP=""
while [ $# -gt 0 ]; do
    case "$1" in
        --comment) OPEN_PR=1; shift ;;
        --help|-h) sed -n '2,19p' "$0"; exit 0 ;;
        stretch|draw_frame|"") GROUP="$1"; shift ;;
        *)
            echo "Usage: $0 [--comment] [stretch|draw_frame]" >&2
            exit 2
            ;;
    esac
done

# T1 `[[bench]]` targets: label `bench:` value -> cargo invocation.
# T1 targets are the ones perf.yml runs and the gate compares; see the
# tier table in .github/RULES.md §4. T2 benches are deliberately not
# baselined — they are on-demand, not gated.
target_args() {
    case "$1" in
        "") printf '%s\n' "stretch" "draw_frame" ;;
        *)  printf '%s\n' "$1" ;;
    esac
}

case "$GROUP" in
    stretch|draw_frame|"") ;;
    *) echo "Usage: $0 [--comment] [stretch|draw_frame]" >&2; exit 2 ;;
esac

CRITERION_DIR="target/criterion"
# Stamped before the run; anything criterion wrote after this instant
# belongs to this run and nothing older does.
MARKER=$(mktemp)
OUT=$(mktemp)
trap 'rm -f "$MARKER" "$OUT"' EXIT

COMMIT=$(git rev-parse --short=7 HEAD)
CAPTURED_AT=$(date -u +%Y-%m-%d)

for target in $(target_args "$GROUP"); do
    case "$target" in
        stretch)    args="-p idle-upscaler --bench stretch" ;;
        draw_frame) args="-p idle-daemon --bench draw_frame" ;;
        *) echo "unknown T1 bench target '$target'" >&2; exit 2 ;;
    esac
    echo "Running T1 bench target: $target"
    # criterion exits non-zero when a benchmark records a change; the
    # JSON is still written, so the failure is logged, not fatal.
    #
    # No `-q` here: criterion 0.5 dropped the flag, and passing it made
    # every run die with "unexpected argument found". The previous
    # version of this script carried that flag and hid the failure
    # behind `|| true`, which is how an empty baseline could be
    # committed as if it were a real one.
    if ! cargo bench $args -- --warm-up-time 1 --measurement-time 3 >"$OUT" 2>&1; then
        echo "WARNING: cargo bench reported a failure for $target; reading JSON anyway" >&2
        tail -20 "$OUT" >&2
    fi
done

python3 - "$CRITERION_DIR" "$MARKER" "$COMMIT" "$CAPTURED_AT" <<'PY'
import json
import os
import sys
from pathlib import Path

crit_dir, marker, commit, captured_at = sys.argv[1:5]
mark = os.path.getmtime(marker)

benches = {}
for path in sorted(Path(crit_dir).rglob("estimates.json")):
    if path.parent.name != "new":
        continue  # `base/` is the prior run, `change/` is the diff
    try:
        if os.path.getmtime(path) < mark - 1.0:
            continue  # left over from an earlier run of some deleted bench
        data = json.loads(path.read_text())
        median = data["median"]["point_estimate"]
        mad = data.get("median_abs_dev", {}).get("point_estimate")
    except (OSError, ValueError, KeyError) as e:
        print(f"WARNING: skipping {path}: {e}", file=sys.stderr)
        continue
    name = str(path.parent.parent.relative_to(crit_dir))
    benches[name] = {"median_ns": median, "median_abs_dev_ns": mad}

if not benches:
    print(f"ERROR: no fresh estimates.json under {crit_dir} after the run", file=sys.stderr)
    print("       refusing to write an empty baseline; check that cargo bench ran",
          file=sys.stderr)
    sys.exit(2)

out = {
    "version": 2,
    "captured_at": captured_at,
    "commit": commit,
    "note": "Captured by scripts/refresh-baseline.sh from criterion estimates.json",
    "benches": benches,
}
Path("perf-baseline.json.tmp").write_text(json.dumps(out, indent=2, sort_keys=True) + "\n")
print(f"Wrote {len(benches)} bench entries to perf-baseline.json.tmp")
PY

# Show the delta against the existing baseline before overwriting it.
if [ -f perf-baseline.json ]; then
    echo
    echo "Delta vs current perf-baseline.json (informational, not enforced):"
    python3 scripts/compare-bench.py perf-baseline.json "$CRITERION_DIR" || true
fi

mv perf-baseline.json.tmp perf-baseline.json

if [ "$OPEN_PR" = "0" ]; then
    echo
    echo "Wrote perf-baseline.json. Review the diff, then:"
    echo "  git add perf-baseline.json && git commit -m 'runtime: refresh perf baseline'"
    exit 0
fi

# --comment mode: branch + push + open a PR with gh CLI.
command -v gh >/dev/null 2>&1 || { echo "ERROR: --comment requires the gh CLI" >&2; exit 1; }
gh auth status >/dev/null 2>&1 || { echo "ERROR: --comment requires an authenticated gh session" >&2; exit 1; }

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

Bench numbers captured against \`$COMMIT\`, read from criterion's
\`new/estimates.json\`. Deltas vs the prior baseline are in the job log.")
echo
echo "Opened PR: $PR_URL"
