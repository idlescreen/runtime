#!/usr/bin/env bash
# scripts/chaos_test.sh - E2E Chaos & Fault Injection Suite entry point
# Sources chaos_common.sh and the per-test modules, runs them sequentially.
#
# CHAOS_ALLOW_NO_WAYLAND=1 permits run without Wayland (explicit residual; tests may fail).
set -euo pipefail
set +m

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck source=./chaos_common.sh
source "$SCRIPT_DIR/chaos_common.sh"

if [ ! -f "$IDLE_DAEMON" ]; then
    echo "[CHAOS SETUP] Building workspace binaries..."
    (cd "$ROOT_DIR/idle" && cargo build --bin idle-daemon)
fi

echo "============================================================"
echo " Starting idlescreen E2E Chaos & Fault Injection Test Suite "
echo "============================================================"
echo " Auth: no IDLE_DBUS_TRUST_ALL (GetStatus product path only)"
echo " Root: $ROOT_DIR"

require_chaos_env

trap 'cleanup' EXIT SIGINT SIGTERM
cleanup

# shellcheck source=./chaos_test_fd.sh
source "$SCRIPT_DIR/chaos_test_fd.sh"
# shellcheck source=./chaos_test_dbus.sh
source "$SCRIPT_DIR/chaos_test_dbus.sh"
# shellcheck source=./chaos_test_fs.sh
source "$SCRIPT_DIR/chaos_test_fs.sh"

if test_fd_limit_exhaustion; then
    PASSED_TESTS=$((PASSED_TESTS + 1))
fi
if test_dbus_flooding; then
    PASSED_TESTS=$((PASSED_TESTS + 1))
fi
if test_readonly_runtime; then
    PASSED_TESTS=$((PASSED_TESTS + 1))
fi

echo ""
echo "============================================================"
echo " Chaos Test Results: $PASSED_TESTS / $TOTAL_TESTS passed"
echo "============================================================"

if [ "$PASSED_TESTS" -eq "$TOTAL_TESTS" ]; then
    echo "SUCCESS: All chaos and fault injection tests PASSED!"
    exit 0
else
    echo "FAILURE: $TOTAL_TESTS chaos tests did not all pass."
    exit 1
fi
