#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright 2026 IdleScreen
#
# Every page declares what it costs and how a machine would notice if
# that cost changed. This script enforces both halves.
#
# Label grammar (fields separated by U+00B7 MIDDLE DOT):
#
#   // perf: T1 · bench: stretch · gate: perf-baseline.json · check: bench
#   // perf: T2 · bench: hot_path · on-demand only · check: bench
#   // perf: T3 · metric: no syscalls on the steady path · check: test
#   // perf: T3 · metric: process-exit, never on a hot path · check: review
#
#   perf:   T1 | T2 | T3        required
#   check:  bench | test | review   required -- the detection mechanism
#   bench:  [[bench]] target name, or `none`   required for T1/T2
#   gate:   baseline file at the repo root     required for T1 only
#   sym:    symbol to find in the bench source; defaults to the filename
#   metric: what this page costs, in words     required for T3
#
# `check:` is the field that answers "how would we know if this got
# slower?". Three answers, and a page must pick one that is true:
#
#   bench   a criterion target measures it and CI compares the median
#           against a baseline
#   test    a property test asserts it -- call counts, allocation
#           counts, no-syscall invariants. This is how a page that is
#           too fast to time gets real machine checking.
#   review  neither is possible, so a human checks the `metric:` claim
#           when the page changes. This is the honest floor, not a
#           pass.
#
# Every `.rs` page must carry a label. An unlabelled page is a page
# nobody has thought about, and that is the failure this replaces.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT"

errors=0

fail() {
    printf 'perf-label: %s\n' "$1" >&2
    errors=$((errors + 1))
}

# Labels separate fields with U+00B7 MIDDLE DOT. Rewrite to `|` so the
# field parsing below stays pure ASCII and needs no locale games.
DOT=$(printf '\xc2\xb7')

# Pull `key: value` out of a rewritten label. awk exits on the first
# match, so the whole field is drained and no stage of this pipeline can
# die on SIGPIPE under `set -o pipefail`.
field() {
    printf '%s' "$1" | tr '|' '\n' |
        awk -v key="$2:" 'index($0, key) {
            sub("^.*" key, "")
            gsub(/^[ \t]+|[ \t]+$/, "")
            print
            exit
        }'
}

# Map bench target name -> source file. `render_content_viewport_into`
# is defined in idle-runner but benched from idle-daemon, so the lookup
# is workspace-wide by design; a per-crate search would false-fail.
declare -A BENCH
while IFS= read -r f; do
    name="$(basename "$f" .rs)"
    [ -n "${BENCH[$name]:-}" ] || BENCH[$name]="$f"
done < <(find . -type f -path '*/benches/*.rs' -not -path '*/target/*' -not -path '*/runtime/*')

# Every page that counts. `lib.rs` and `main.rs` are crate roots that
# only re-export or hold `fn main`; they are pages too and are held to
# the same rule, because a crate root is exactly where a reader looks
# first.
page_list() {
    find . -name '*.rs' \
        -not -path './target/*' -not -path './dist/*' -not -path './.git/*' \
        -not -path './.agents/*' -not -path './node_modules/*' \
        -not -path './.local/*' -not -path './containers/*' \
        -not -path './.cache/*' -not -path './runtime/*' \
        | sed 's|^\./||' | sort
}

checked=0
declare -A SEEN

while IFS= read -r file; do
    [ -n "$file" ] || continue
    # A vendored copy of another repo is not this repo's page.
    case "$file" in runtime/*) continue ;; esac

    label="$(grep -m1 -E '^[[:space:]]*//[[:space:]]*perf:[[:space:]]' "$file" || true)"
    checked=$((checked + 1))
    where="$file"

    if [ -z "$label" ]; then
        fail "$where: no // perf: label -- every page declares what it costs and how it is checked"
        continue
    fi
    SEEN["$file"]=1

    lineno="$(printf '%s' "$label" | grep -o '^[0-9]*' || true)"
    [ -n "$lineno" ] && where="$file:$lineno"

    fields="${label//$DOT/|}"
    tier="$(field "$fields" perf)"
    bench="$(field "$fields" bench)"
    sym="$(field "$fields" sym)"
    gate="$(field "$fields" gate)"
    check="$(field "$fields" check)"
    metric="$(field "$fields" metric)"

    if [ -z "$sym" ]; then
        sym="$(basename "$file" .rs)"
    fi

    if [ -z "$tier" ]; then
        fail "$where: label has no tier (expected T1, T2 or T3)"
        continue
    fi

    if [ -z "$check" ]; then
        fail "$where: $tier label has no check: field -- say how a machine would notice (bench, test or review)"
        continue
    fi

    case "$check" in
        bench|test|review) ;;
        *) fail "$where: unknown check '$check' (expected bench, test or review)"; continue ;;
    esac

    if [ -n "$bench" ] && [ "$bench" != "none" ] && [ -z "${BENCH[$bench]:-}" ]; then
        fail "$where: $tier names bench '$bench', which is not a bench target in this repo"
        continue
    fi

    case "$tier" in
        T1)
            if [ -z "$bench" ] || [ "$bench" = "none" ]; then
                fail "$where: T1 must name a bench target; '$sym' is never gated if it has none"
                continue
            fi
            if [ "$check" != "bench" ]; then
                fail "$where: T1 is measured by criterion, so check: must be 'bench', not '$check'"
                continue
            fi
            if ! grep -q -- "$sym" "${BENCH[$bench]}"; then
                fail "$where: T1 claims bench '$bench', but that source never references '$sym'"
                continue
            fi
            if [ -z "$gate" ]; then
                fail "$where: T1 must name a gate: (regressions are checked against a baseline)"
                continue
            fi
            if [ ! -f "$ROOT/$gate" ]; then
                fail "$where: T1 gate '$gate' does not exist at the repo root"
                continue
            fi
            ;;
        T2)
            if [ -z "$bench" ]; then
                fail "$where: T2 must name a bench target or say bench: none with a reason"
                continue
            fi
            if [ "$bench" = "none" ]; then
                # Un-bencheable on this runner -- an aarch64-only path, say.
                # It is not measured, so it cannot claim check: bench, and
                # it needs a stated reason for being unmeasured.
                if [ "$check" = "bench" ]; then
                    fail "$where: T2 says bench: none, so check: cannot be 'bench' -- use test or review"
                    continue
                fi
                if [ -z "$metric" ]; then
                    fail "$where: T2 with bench: none must state a metric: explaining why it is unmeasured"
                    continue
                fi
            elif [ "$check" != "bench" ]; then
                fail "$where: T2 is benched, so check: must be 'bench', not '$check'"
                continue
            fi
            ;;
        T3)
            if [ -n "$bench" ] && [ "$bench" != "none" ]; then
                fail "$where: T3 is QA-only and must not claim a bench"
                continue
            fi
            if [ "$check" = "bench" ]; then
                fail "$where: T3 must not claim check: bench; that is a T1/T2 measurement"
                continue
            fi
            if [ -z "$metric" ]; then
                fail "$where: T3 must state a metric: -- what does this page cost?"
                continue
            fi
            if [ "$check" = "test" ] && ! grep -q '#\[test\]' "$file"; then
                fail "$where: check: test claims a property test, but the page has no #[test]"
                continue
            fi
            ;;
        *)
            fail "$where: unknown tier '$tier' (expected T1, T2 or T3)"
            ;;
    esac
done < <(page_list)

if [ "$checked" -eq 0 ]; then
    echo "perf-label: no pages found -- is the find pattern stale?" >&2
    exit 1
fi

if [ "$errors" -gt 0 ]; then
    echo "perf-label: $errors bad page(s) across $checked checked" >&2
    exit 1
fi

echo "perf-label: $checked page(s) OK"
