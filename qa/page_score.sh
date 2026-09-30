#!/bin/bash
# page_score.sh — per-page scorecard: lines, binary bytes, heat, reach.
#
# Usage: qa/page_score.sh [--bin] [--heat [--heat-runs N]] [--out FILE]
#   default: lines + reach (fast, no build)
#   --bin:   + shipped-bytes attribution via nm (builds the workspace)
#   --heat:  + CPU share sampled while test binaries run (slow, bounded);
#            --heat-runs N repeats the suite N times for fast suites
# Writes page-score.json. Best-effort fields are null with notes when
# their tools are missing. Only stock Linux tools are used.
set -u
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="page-score.json"
WANT_BIN=0
WANT_HEAT=0
HEAT_RUNS=1
while [ $# -gt 0 ]; do
  case "$1" in
    --bin) WANT_BIN=1 ;;
    --heat) WANT_HEAT=1 ;;
    --heat-runs) HEAT_RUNS="$2"; shift ;;
    --out) OUT="$2"; shift ;;
  esac
  shift
done
cd "$ROOT" || exit 1
TMP="$(mktemp -d /tmp/page-score.XXXXXX)"
trap 'rm -rf "$TMP"' EXIT

# ---- pages + lines (exact) ----
find . -name '*.rs' -not -path '*/target/*' -not -path '*/.git/*' -not -path './runtime/*' -not -path './downstream/*' | sort > "$TMP/pages.txt"
: > "$TMP/lines.tsv"
while IFS= read -r f; do
  printf '%s\t%s\n' "$f" "$(wc -l < "$f")" >> "$TMP/lines.tsv"
done < "$TMP/pages.txt"

# ---- reach: files referencing each page's module stem (approx) ----
# A bare stem over-counts when several pages share a file name (every
# error.rs claims every error:: reference). Shared stems must match
# qualified by an ancestor directory (bus::error, or dbus::error via a
# re-export); relative references from the same directory
# (super::/self::/bare stem) still count, since those resolve to that
# directory's page. Unique stems match bare.
awk -F/ '{b=$NF; sub(/\.rs$/,"",b); print b}' "$TMP/pages.txt" | sort | uniq -c > "$TMP/stemcounts.txt"
declare -A STEMCOUNT
while read -r c s; do STEMCOUNT[$s]=$c; done < "$TMP/stemcounts.txt"
: > "$TMP/reach.tsv"
while IFS= read -r f; do
  stem="$(basename "$f" .rs)"
  dir="$(dirname "$f")"
  if [ "${STEMCOUNT[$stem]:-1}" -gt 1 ] && [ "$dir" != "." ]; then
    anc="$(echo "$dir" | tr '/' '\n' | grep -v '^\.$' | grep -v '^$' | paste -sd'|')"
    n=$({ grep -rl --include='*.rs' -E "(^|[^A-Za-z0-9_])(${anc})::${stem}(::|[ ;])|use [^;]*(${anc})::${stem}[ ;]" . 2>/dev/null | grep -v target;
          grep -rl --include='*.rs' -E "(^|[^A-Za-z0-9_])${stem}::|use [^;]*${stem}[ ;]|(super|self)::${stem}([^A-Za-z0-9_]|$)" "$dir" 2>/dev/null; } | sort -u | grep -v "^${f}$" | wc -l)
  else
    n=$(grep -rl --include='*.rs' -E "(^|[^A-Za-z0-9_])${stem}::|use [^;]*${stem}[ ;]" . 2>/dev/null | grep -v target | grep -v "^${f}$" | wc -l)
  fi
  printf '%s\t%s\n' "$f" "$n" >> "$TMP/reach.tsv"
done < "$TMP/pages.txt"

NOTES="reach counts referencing files (shared file names qualified by ancestor dir);"
# ---- bin_bytes: .text sizes per source file via nm (approx) ----
: > "$TMP/bin.tsv"
if [ "$WANT_BIN" = 1 ]; then
  if ! command -v nm >/dev/null; then
    NOTES="$NOTES bin_bytes unavailable (no nm);"
  elif ! cargo build --workspace --offline >/dev/null 2>&1; then
    NOTES="$NOTES bin_bytes unavailable (build failed);"
  else
    : > "$TMP/nm.tsv"
    # Workspace-member rlibs only: dependency rlibs carry foreign code that
    # must never land on repo pages. Falls back to all rlibs if metadata
    # is unavailable.
    MEMBERS="$(cargo metadata --format-version 1 --offline --no-deps 2>/dev/null | python3 -c "
import json,sys,os
tmp = sys.argv[1]
try: m = json.load(sys.stdin)
except Exception: raise SystemExit
names = set()
pkgdirs = open(tmp + '/pkgdirs.tsv', 'w')
root = os.getcwd()
for p in m.get('packages', []):
    d = os.path.relpath(os.path.dirname(p['manifest_path']), root)
    pkgdirs.write(p['name'] + '\t' + ('./' if d == '.' else './' + d) + '\n')
    for t in p.get('targets', []):
        # 'lib' is cargo's word for the default rlib output.
        if set(t.get('crate_types', [])) & {'lib', 'rlib', 'dylib', 'staticlib'}:
            names.add(t['name'].replace('-', '_'))
print(' '.join(sorted(names)))" "$TMP")"
    if [ -n "$MEMBERS" ]; then
      MEMBER_RE="/lib($(echo "$MEMBERS" | tr ' ' '|'))-[0-9a-f]+\.rlib$"
      RLIBS="$(ls -t target/debug/deps/*.rlib 2>/dev/null | grep -E "$MEMBER_RE")"
    else
      NOTES="$NOTES bin rlib filter off (no cargo metadata);"
      RLIBS="$(ls -t target/debug/deps/*.rlib 2>/dev/null)"
    fi
    # One rlib per crate: newest first that was actually built from this
    # repo's sources (deps/ and registry-built same-name rlibs exist from
    # release debris; recency alone picks the wrong file).
    declare -A SEEN_CRATE
    for rlib in $RLIBS; do
      [ -f "$rlib" ] || continue
      key="$(echo "$rlib" | awk -F'[-.]' '{k=""; for(i=1;i<NF-1;i++) k=k $i "-"; print k}')"
      [ -n "${SEEN_CRATE[$key]:-}" ] && continue
      if nm --print-size --line-numbers "$rlib" 2>/dev/null | grep -q -E -e "$ROOT" -e '/target/package/'; then
        SEEN_CRATE[$key]=1
        nm --print-size --line-numbers "$rlib" 2>/dev/null | awk '
          / [TtWw] / {
            size = strtonum("0x" $2)
            loc = $NF
            sub(/:[0-9]+$/, "", loc)
            if (loc != "" && loc != "??") print loc "\t" size
          }' >> "$TMP/nm.tsv"
      fi
    done
    # Map nm paths onto repo pages. Packaged-copy paths (target/package)
    # resolve through the cargo package->dir map, since member dirs don't
    # match package names. Absolute paths outside the repo (dependency
    # sources, std) are NEVER basename-matched: that used to dump foreign
    # lib.rs/mod.rs code onto same-named repo pages. Only relative strays
    # match by unique basename.
    [ -f "$TMP/pkgdirs.tsv" ] || : > "$TMP/pkgdirs.tsv"
    awk -v root="$ROOT" '
      FILENAME == ARGV[1] { pages[$1] = 1; base[$1] = $1; sub(/.*\//, "", base[$1]); count[base[$1]]++; next }
      FILENAME == ARGV[2] { pkgdir[$1] = $2; next }
      {
        loc = $1; size = $2
        if (match(loc, /\/target\/package\/[^\/]+\//)) {
          seg = substr(loc, RSTART, RLENGTH)
          sub(/^\/target\/package\//, "", seg); sub(/\/$/, "", seg)
          sub(/-[0-9][0-9A-Za-z.+-]*$/, "", seg)
          rest = substr(loc, RSTART + RLENGTH)
          if (seg in pkgdir) loc = pkgdir[seg] "/" rest
          else loc = "./" rest
          gsub(/\/+/, "/", loc)
        }
        else if (index(loc, root) == 1) { loc = substr(loc, length(root) + 2); if (loc !~ /^\.\//) loc = "./" loc }
        else if (loc ~ /^\// || loc ~ /\.\./) next
        else { b = loc; sub(/.*\//, "", b); if (count[b] == 1) { for (p in pages) if (base[p] == b) { loc = p; break } } else next }
        if (loc in pages) bytes[loc] += size
      }
      END { for (p in bytes) print p "\t" bytes[p] }
    ' "$TMP/pages.txt" "$TMP/pkgdirs.tsv" "$TMP/nm.tsv" > "$TMP/bin.tsv"
    NOTES="$NOTES bin_bytes from newest workspace dev rlib per crate via nm (inlining approx, bins re-link the same code; shims excluded, foreign paths never attributed);"
  fi
fi

# ---- heat: CPU share per source file, sampled under test runs (approx) ----
: > "$TMP/heat.tsv"
if [ "$WANT_HEAT" = 1 ]; then
  if ! command -v eu-stack >/dev/null; then
    NOTES="$NOTES heat unavailable (no eu-stack);"
  elif ! cargo test --workspace --no-run --offline >/dev/null 2>&1; then
    NOTES="$NOTES heat unavailable (test build failed);"
  else
    : > "$TMP/hits.tsv"
    SAMPLED=0
    TOTAL=0
    DEADLINE=$((SECONDS + 1200))
    for RUN in $(seq 1 "$HEAT_RUNS"); do
      for bin in target/debug/deps/*; do
        [ -f "$bin" ] && [ -x "$bin" ] || continue
        case "$bin" in *.d|*.rlib|*.rmeta|*.so|*.dylib) continue ;; esac
        TOTAL=$((TOTAL + 1))
        [ $SECONDS -gt $DEADLINE ] && break 2
        timeout -s KILL 300 "$bin" </dev/null >>"$TMP/run.log" 2>&1 &
        PID=$!
        sleep 0.05
        TPID=$(pgrep -P $PID 2>/dev/null | head -1)
        [ -z "$TPID" ] && { wait $PID 2>/dev/null; continue; }
        SAMPLED=$((SAMPLED + 1))
        EXE=$(readlink -f /proc/$TPID/exe 2>/dev/null)
        TICKS=0
        while kill -0 $TPID 2>/dev/null && [ $TICKS -lt 2400 ]; do
          MAPS=$(awk -v exe="$EXE" '$6 == exe && index($2, "r-x") == 1 {print $1; exit}' /proc/$TPID/maps 2>/dev/null)
          BASE=${MAPS%%-*}; END=${MAPS##*-}
          for P in $TPID $(pgrep -P $TPID 2>/dev/null); do
            eu-stack -p $P 2>/dev/null | grep -oE "#[0-9]+ +0x[0-9a-f]+" | awk '{print $2}' | head -16 >> "$TMP/frames.$SAMPLED"
          done
          echo "$EXE $BASE $END" > "$TMP/base.$SAMPLED"
          TICKS=$((TICKS + 1))
          sleep 0.1
        done
        wait $PID 2>/dev/null
      done
    done
    # Attribute one innermost repo frame per tick batch is overkill; count
    # every frame resolving into the repo (documented in notes).
    : > "$TMP/heatraw.tsv"
    for f in "$TMP"/frames.*; do
      [ -f "$f" ] || continue
      tag=${f##*/frames.}; read -r EXE BASE END < "$TMP/base.$tag" 2>/dev/null || continue
      [ -n "$BASE" ] || continue
      awk -v b="0x$BASE" -v e="0x$END" '{a=strtonum($1); if (a>=strtonum(b) && a<strtonum(e)) printf "%x\n", a-strtonum(b)}' "$f" | sort -u > "$TMP/addrs.$tag"
      [ -s "$TMP/addrs.$tag" ] || continue
      # shellcheck disable=SC2046
      addr2line -e "$EXE" -f -C $(cat "$TMP/addrs.$tag" | tr '\n' ' ') 2>/dev/null | grep -E "^/" | sed "s|^$ROOT/||" >> "$TMP/heatraw.tsv"
    done
    TOTALHITS=$(wc -l < "$TMP/heatraw.tsv")
    if [ "$TOTALHITS" -ge 50 ]; then
      cut -d: -f1 "$TMP/heatraw.tsv" | sort | uniq -c | awk -v t="$TOTALHITS" -v r="$ROOT/" '{p=$2; sub("^"r, "", p); printf "./%s\t%.2f\n", p, 100*$1/t}' > "$TMP/heat.tsv"
      NOTES="$NOTES heat from $TOTALHITS attributed samples over $SAMPLED/$TOTAL binaries at 10Hz incl. one child level; every repo-resolving frame counts;"
    else
      NOTES="$NOTES heat unavailable: only $TOTALHITS attributed samples (need 50; suite too fast — use --heat-runs N to amplify);"
    fi
  fi
fi

# ---- emit ----
python3 - "$TMP" "$OUT" "$NOTES" <<'EOF'
import json, sys
tmp, out, notes = sys.argv[1], sys.argv[2], sys.argv[3]
def tsv(name):
    d = {}
    try:
        with open(f"{tmp}/{name}") as f:
            for line in f:
                line = line.rstrip("\n")
                if "\t" in line:
                    k, v = line.split("\t", 1)
                    d[k] = v
    except FileNotFoundError:
        pass
    return d
lines, reach, bins, heat = tsv("lines.tsv"), tsv("reach.tsv"), tsv("bin.tsv"), tsv("heat.tsv")
def is_shim(path):
    # Same mechanical rule as PAGE_RULE.md: only mod/use/pub lines remain
    # after dropping blanks, comments, and attributes. Shims declare no
    # code, so any binary attribution to them is noise.
    try:
        has_code = False
        with open(path) as f:
            for raw in f:
                s = raw.strip()
                if not s or s.startswith("//") or s.startswith("#[") or s.startswith("#!["):
                    continue
                if not (s.startswith("mod ") or s.startswith("use ")
                        or s.startswith("pub")):
                    return False
                has_code = True
        return has_code
    except OSError:
        return False
import subprocess
rev = subprocess.run(["git", "rev-parse", "--short", "HEAD"], capture_output=True, text=True).stdout.strip()
dirty = bool(subprocess.run(["git", "status", "--short"], capture_output=True, text=True).stdout.strip())
repo = subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True).stdout.strip().rstrip("/").rsplit("/", 1)[-1]
pages = []
with open(f"{tmp}/pages.txt") as f:
    for line in f:
        p = line.rstrip("\n")
        pages.append({
            "path": p,
            "lines": int(lines.get(p, 0)),
            "bin_bytes": None if is_shim(p) else (int(bins[p]) if p in bins else None),
            "heat_pct": float(heat[p]) if p in heat else None,
            "reach": int(reach.get(p, 0)),
        })
with open(out, "w") as f:
    json.dump({"repo": repo, "rev": rev, "dirty": dirty, "pages": pages, "notes": notes.strip()}, f, indent=1)
print(f"wrote {out}: {len(pages)} pages")
EOF
