#!/usr/bin/env bash
# chaos_test_dbus.sh - D-Bus method-call flooding test (sourced by chaos_test.sh)
test_dbus_flooding() {
    echo ""
    echo "[CHAOS TEST 2/3] Simulating D-Bus Method Call Bursting & Flooding..."
    cleanup
    sleep 0.2

    echo "[CHAOS TEST] Spawning dedicated test daemon instance..."
    spawn_test_daemon

    if ! wait_for_daemon_dbus_ready "$TEST_DAEMON_PID"; then
        echo "[CHAOS ERROR] Test daemon failed to launch for D-Bus flood (Wayland/XDG env or crash — not TRUST_ALL)."
        tail -n 20 "$DAEMON_LOG" 2>/dev/null || true
        cleanup; return 1
    fi

    echo "[PROOF] Bursting 300 D-Bus method calls across concurrent processes..."
    CHAOS_FLOOD_PIDS=()
    for _ in $(seq 1 5); do
        (
            for _ in $(seq 1 60); do
                busctl --auto-start=false --user call io.github.idlescreen.Idle /io/github/idlescreen/Idle io.github.idlescreen.Idle GetStatus >/dev/null 2>&1 || true
                busctl --auto-start=false --user call io.github.idlescreen.Idle /io/github/idlescreen/Idle io.github.idlescreen.Idle GetStatus >/dev/null 2>&1 || true
                sleep 0.002
            done
        ) &
        CHAOS_FLOOD_PIDS+=($!)
    done

    for fpid in "${CHAOS_FLOOD_PIDS[@]}"; do
        wait "$fpid" 2>/dev/null || true
    done
    CHAOS_FLOOD_PIDS=()

    echo "[PROOF] Verifying D-Bus responsiveness after message flood..."
    sleep 0.3
    local status="" call_res=1 out=""
    for _ in $(seq 1 60); do
        if kill -0 "$TEST_DAEMON_PID" 2>/dev/null; then
            if out=$(busctl --auto-start=false --user call io.github.idlescreen.Idle /io/github/idlescreen/Idle io.github.idlescreen.Idle GetStatus 2>&1); then
                status="$out"; call_res=0; break
            else
                [ -n "$out" ] && status="$out"
            fi
        fi
        sleep 0.1
    done

    if [ "$call_res" -ne 0 ] || ! echo "$status" | grep -q "running"; then
        echo "[CHAOS ERROR] Daemon failed D-Bus response post-flood: $status"
        tail -n 20 "$DAEMON_LOG" 2>/dev/null || true
        cleanup; return 1
    fi

    cleanup
    echo "✅ [PASS] D-Bus Flooding test passed (daemon survived and responded)."
    return 0
}
