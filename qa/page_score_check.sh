#!/bin/bash
# page_score_check.sh — validate page-score.json against known-good rules.
#
# Usage: qa/page_score_check.sh [SCOREFILE]
# Exit 0 when every check passes, 1 otherwise. Run after regenerating the
# scorecard (qa/page_score.sh) and before shipping a release snapshot.
# Guards the fixed bugs: foreign-code attribution onto same-named pages
# (2026-09) and shared reach counts across same-named pages (2026-09).
set -u
ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
SCORE="${1:-page-score.json}"
cd "$ROOT" || exit 1
[ -f "$SCORE" ] || { echo "FAIL: $SCORE not found (run qa/page_score.sh first)"; exit 1; }

find . -name '*.rs' -not -path '*/target/*' -not -path '*/.git/*' | sort > /tmp/pages.check.$$
trap 'rm -f /tmp/pages.check.$$' EXIT

python3 - "$SCORE" /tmp/pages.check.$$ <<'EOF'
import json, os, sys
from collections import Counter, defaultdict

score_file, pages_file = sys.argv[1], sys.argv[2]
fails = []
def check(name, ok, detail=""):
    print(("PASS " if ok else "FAIL ") + name + (f" ({detail})" if detail and not ok else ""))
    if not ok:
        fails.append(name)

d = json.load(open(score_file))
pages = d.get("pages", [])
by_path = {p["path"]: p for p in pages}
disk = [l.rstrip("\n") for l in open(pages_file)]

# 1. Every file on disk is listed exactly once (no stale/missing entries).
counts = Counter(p["path"] for p in pages)
missing = [f for f in disk if f not in by_path]
stale = [p for p in by_path if p not in set(disk)]
dupes = [p for p, c in counts.items() if c > 1]
check("coverage", not missing and not stale and not dupes,
      f"missing={missing[:3]} stale={stale[:3]} dupes={dupes[:3]}")

# 2. Shims (re-export-only pages) never carry binary bytes: they declare
# no code, so any attribution is measurement noise. Same mechanical rule
# as PAGE_RULE.md. Skipped when the snapshot has no bin data at all.
def is_shim(path):
    try:
        with open(path) as f:
            for raw in f:
                s = raw.strip()
                if not s or s.startswith("//") or s.startswith("#["):
                    continue
                if not (s.startswith("mod ") or s.startswith("use ")
                        or s.startswith("pub")):
                    return False
        return True
    except OSError:
        return False
has_bin = any(p.get("bin_bytes") is not None for p in pages)
bad_shims = [p["path"] for p in pages
             if p.get("bin_bytes") is not None and is_shim(p["path"])]
if has_bin:
    check("shim-bin-null", not bad_shims, f"{len(bad_shims)} shims with bin_bytes, e.g. {bad_shims[:3]}")
else:
    print("SKIP shim-bin-null (no bin data in snapshot)")

# 3. Same-named pages must not share one big reach count: that is the
# signature of the unqualified-stem bug (e.g. four error.rs each at 75).
by_base = defaultdict(list)
for p in pages:
    by_base[os.path.basename(p["path"])].append(p)
shared_bad = []
for base, group in by_base.items():
    reaches = {p["reach"] for p in group}
    if len(group) >= 3 and len(reaches) == 1 and next(iter(reaches)) >= 10:
        shared_bad.append(f"{base} x{len(group)} all reach={next(iter(reaches))}")
check("shared-reach", not shared_bad, "; ".join(shared_bad[:3]))

# 4. No page exceeds 50 KB of binary per source line: real debug codegen
# (even derive-heavy pages) peaks in the single digits; anything past
# this line is foreign code mis-attributed onto the page.
bloated = [f"{p['path']} {p['bin_bytes']/p['lines']:,.0f} B/line"
           for p in pages
           if p.get("bin_bytes") and p.get("lines") and p["bin_bytes"]/p["lines"] > 50000]
check("bin-plausible", not bloated, "; ".join(bloated[:3]))

# 5. Schema sanity: required keys with sane types on every page.
schema_bad = [p.get("path", "?") for p in pages
              if not isinstance(p.get("lines"), int) or p.get("lines", 0) <= 0
              or not isinstance(p.get("reach"), int) or p.get("reach", -1) < 0
              or ("bin_bytes" in p and p["bin_bytes"] is not None and not isinstance(p["bin_bytes"], int))
              or ("heat_pct" in p and p["heat_pct"] is not None and not isinstance(p["heat_pct"], (int, float)))]
check("schema", not schema_bad, f"{len(schema_bad)} bad entries, e.g. {schema_bad[:3]}")

sys.exit(1 if fails else 0)
EOF
