#!/usr/bin/env bash
# chaos_common.sh - shared helpers for chaos_test_*.sh
# Sourced; not executable standalone.
#
# Prereqs (product path):
#   - XDG_RUNTIME_DIR writable
#   - WAYLAND_DISPLAY (or wayland-* socket under XDG_RUNTIME_DIR)
#   - debug idle-daemon binary (built if missing)
#
# Auth honesty: does NOT set IDLE_DBUS_TRUST_ALL. Chaos exercises GetStatus
#
# Env flags:
#   CHAOS_ALLOW_NO_WAYLAND=1  — allow running without Wayland; C1/C2 will fail
#                               closed with an explicit ENV residual (not silent green)

set -euo pipefail
set +m

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# Prefer IDLE_DAEMON env override (e.g. release binary for product-path probes).
IDLE_DAEMON="${IDLE_DAEMON:-$ROOT_DIR/idle/target/debug/idle-daemon}"
export IDLE_TEST_DISABLE_EXTERNAL=1
# Intentionally NOT exporting IDLE_DBUS_TRUST_ALL (F-004).

PASSED_TESTS=0
TOTAL_TESTS=3
TEST_DAEMON_PID=""
DAEMON_LOG=""
CHAOS_TMP_DIRS=()
CHAOS_FLOOD_PIDS=()

cleanup() {
    set +e
    local uid
    uid=$(id -u 2>/dev/null || echo 1000)
    local pid_files=("/run/user/$uid/idle-daemon.pid" "${XDG_RUNTIME_DIR:-/tmp}/idle-daemon.pid" "/tmp/idle-daemon.pid")

    rm -f "${pid_files[@]}" 2>/dev/null || true
    rm -f "${XDG_RUNTIME_DIR:-/tmp}"/idle-uds-*.sock /tmp/idle-uds-*.sock 2>/dev/null || true

    if [ "${#CHAOS_FLOOD_PIDS[@]}" -gt 0 ]; then
        for fpid in "${CHAOS_FLOOD_PIDS[@]}"; do
            kill -9 "$fpid" 2>/dev/null || true
        done
        CHAOS_FLOOD_PIDS=()
    fi

    systemctl --user stop idle-daemon.service 2>/dev/null || true
    systemctl --user reset-failed idle-daemon.service 2>/dev/null || true

    if [ -n "${TEST_DAEMON_PID:-}" ]; then
        kill -TERM "$TEST_DAEMON_PID" 2>/dev/null || true
        for _ in $(seq 1 10); do
            kill -0 "$TEST_DAEMON_PID" 2>/dev/null || break
            sleep 0.05
        done
        kill -9 "$TEST_DAEMON_PID" 2>/dev/null || true
        TEST_DAEMON_PID=""
    fi

    pkill -TERM -u "$USER" -x "idle-daemon" 2>/dev/null || true
    pkill -TERM -u "$USER" -x "idlescreen-daemon" 2>/dev/null || true
    for _ in $(seq 1 20); do
        pgrep -u "$USER" -x "idle-daemon" >/dev/null 2>&1 || break
        sleep 0.05
    done
    pkill -9 -u "$USER" -x "idle-daemon" 2>/dev/null || true
    pkill -9 -u "$USER" -x "idlescreen-daemon" 2>/dev/null || true
    pkill -9 -u "$USER" -x "busctl" 2>/dev/null || true
    wait 2>/dev/null || true

    rm -f "${pid_files[@]}" 2>/dev/null || true
    rm -f "${XDG_RUNTIME_DIR:-/tmp}"/idle-uds-*.sock /tmp/idle-uds-*.sock 2>/dev/null || true

    for tmpdir in "${CHAOS_TMP_DIRS[@]:-}"; do
        if [ -n "$tmpdir" ] && [ -d "$tmpdir" ]; then
            chmod -R 755 "$tmpdir" 2>/dev/null || true
            rm -rf "$tmpdir" 2>/dev/null || true
        fi
    done
    CHAOS_TMP_DIRS=()

    for _ in $(seq 1 30); do
        busctl --user list 2>/dev/null | grep -q "io.github.idlescreen.Idle" || break
        sleep 0.05
    done

    rm -f "${pid_files[@]}" 2>/dev/null || true
    sleep 0.1
    if [ -n "${DAEMON_LOG:-}" ]; then
        rm -f "$DAEMON_LOG" 2>/dev/null || true
        DAEMON_LOG=""
    fi
    return 0
}

wait_for_daemon_dbus_ready() {
    local pid="${1:-}"
    for _ in $(seq 1 60); do
        if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
            if busctl --user list 2>/dev/null | grep -E -q "^io\.github\.idlescreen\.Idle[[:space:]]+$pid\b"; then
                return 0
            fi
        fi
        if busctl --auto-start=false --user call io.github.idlescreen.Idle /io/github/idlescreen/Idle io.github.idlescreen.Idle GetStatus >/dev/null 2>&1; then
            return 0
        fi
        sleep 0.1
    done
    return 1
}

# Returns 0 if env is sufficient for full chaos; 1 if residual skip mode; exits 2 if hard fail.
require_chaos_env() {
    local missing=0
    local msg=""

    if [ -z "${XDG_RUNTIME_DIR:-}" ] || [ ! -d "${XDG_RUNTIME_DIR}" ]; then
        missing=1
        msg="${msg}XDG_RUNTIME_DIR missing or not a directory. "
    fi

    local has_wl=0
    if [ -n "${WAYLAND_DISPLAY:-}" ]; then
        if [ -S "${XDG_RUNTIME_DIR:-/dev/null}/${WAYLAND_DISPLAY}" ] \
            || [ -S "/run/user/$(id -u)/${WAYLAND_DISPLAY}" ]; then
            has_wl=1
        fi
        # Some compositors export WAYLAND_DISPLAY without a plain socket path check.
        if [ "$has_wl" -eq 0 ] && [ -n "${WAYLAND_DISPLAY}" ]; then
            has_wl=1
        fi
    fi
    if [ "$has_wl" -eq 0 ] && [ -n "${XDG_RUNTIME_DIR:-}" ]; then
        if ls "${XDG_RUNTIME_DIR}"/wayland-* >/dev/null 2>&1; then
            has_wl=1
        fi
    fi
    if [ "$has_wl" -eq 0 ]; then
        missing=1
        msg="${msg}Wayland display/socket not available (set WAYLAND_DISPLAY). "
    fi

    if [ "$missing" -eq 0 ]; then
        return 0
    fi

    if [ "${CHAOS_ALLOW_NO_WAYLAND:-}" = "1" ]; then
        echo "[CHAOS ENV] RESIDUAL: ${msg}"
        echo "[CHAOS ENV] CHAOS_ALLOW_NO_WAYLAND=1 — C1/C2 may fail closed (not a silent pass)."
        return 1
    fi

    echo "[CHAOS ENV] ERROR: ${msg}"
    echo "[CHAOS ENV] Required: XDG_RUNTIME_DIR + Wayland session for full suite."
    echo "[CHAOS ENV] Or set CHAOS_ALLOW_NO_WAYLAND=1 to run with explicit residual (exit non-zero if tests fail)."
    exit 2
}

spawn_test_daemon() {
    DAEMON_LOG=$(mktemp)
    # No IDLE_DBUS_TRUST_ALL — GetStatus is unauthenticated product path.
    IDLE_TEST_DISABLE_EXTERNAL=1 "$IDLE_DAEMON" >"$DAEMON_LOG" 2>&1 &
    TEST_DAEMON_PID=$!
    disown "$TEST_DAEMON_PID" 2>/dev/null || true
}
