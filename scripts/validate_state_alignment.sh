#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Validation script for Requirement R2 / Acceptance Criterion 4: State Alignment

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$SCRIPT_DIR/.."

echo "=== Running State Alignment Integration Tests ==="
cd "$PROJECT_ROOT/idle-daemon"
cargo test --test state_sync_tests

echo "=== Running clippy (-D warnings, gating) ==="
cd "$PROJECT_ROOT"
cargo clippy --workspace --all-targets -- -D warnings
CLIPPY_RC=$?
if [ "$CLIPPY_RC" -ne 0 ]; then
    echo "  clippy gate FAILED (exit=$CLIPPY_RC)"
    exit "$CLIPPY_RC"
fi
echo "  clippy gate PASSED"

echo "=== Poison fail-open gate (no production into_inner recovery) ==="
# Ban silent poison recovery outside tests/docs.
if grep -RIn --include='*.rs' 'into_inner()' \
    "$PROJECT_ROOT/idle-daemon/src" \
    "$PROJECT_ROOT/idle-runner/src" \
    2>/dev/null \
    | grep -v '/tests' \
    | grep -v '_tests\.rs' \
    | grep -v 'mod tests' \
    | grep -v 'locks\.rs' \
    | grep -v '#\[cfg(test)\]' \
    | grep -v 'poison_or_exit' \
    | grep -q .; then
    echo "  FAIL: production into_inner() poison recovery found:"
    grep -RIn --include='*.rs' 'into_inner()' \
        "$PROJECT_ROOT/idle-daemon/src" \
        "$PROJECT_ROOT/idle-runner/src" \
        2>/dev/null \
        | grep -v '/tests' | grep -v '_tests\.rs' | grep -v 'locks\.rs' || true
    exit 1
fi
echo "  poison gate PASSED"

echo "=== All Milestone 2 State Alignment Checks Passed Successfully ==="
