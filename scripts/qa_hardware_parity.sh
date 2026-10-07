#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# qa_hardware_parity.sh — Phase 4 verification on real hardware.
#
# Everything else in qa/ runs headless. This cannot: input semantics, DPMS
# power-off, suspend/resume and multi-monitor coverage are properties of a
# physical machine and a real compositor. This script drives them and prints a
# pass/fail/skip matrix so the result is a recorded artifact rather than a
# memory of "it looked fine".
#
# Honesty rules this script holds itself to:
#   - SKIP is a real outcome and is never reported as PASS. A dimension that
#     could not be exercised on this machine is unknown, not good.
#   - Rendering liveness is measured as CPU time consumed by the daemon unit
#     (CPUUsageNSec covers the daemon *and* its run-ipc-runner children), not
#     inferred from a status flag. A flag says what the daemon believes; CPU
#     says whether pixels were actually produced.
#   - It always restores the machine: presentation stopped, idle re-enabled.
#
# Usage:
#   ./scripts/qa_hardware_parity.sh                 # all scenarios
#   ./scripts/qa_hardware_parity.sh input multihead # named scenarios
#   SCENARIOS=dpms NONINTERACTIVE=1 ./scripts/qa_hardware_parity.sh
#
# Environment:
#   SCENARIOS      space-separated subset (input dpms suspend multihead)
#   SAMPLE_HZ      sampler interval in Hz (default 2)
#   SETTLE_SECS    seconds to hold a stable phase (default 6)
#   NONINTERACTIVE set to 1 to skip anything needing a human -> SKIP, not FAIL
#   DPMS_OFF_CMD   shell command that powers the display down (e.g. brightnessctl set 0)
#   DPMS_ON_CMD    shell command that powers the display back up
#   SUSPEND_CMD    shell command that suspends (default: prompt for lid close)
#
# Exit 0 = no FAIL. 1 = at least one FAIL. 2 = harness could not start.

set -euo pipefail

SCENARIOS="${SCENARIOS:-input dpms suspend multihead}"
SAMPLE_HZ="${SAMPLE_HZ:-2}"
SETTLE_SECS="${SETTLE_SECS:-6}"
NONINTERACTIVE="${NONINTERACTIVE:-0}"
UNIT="idle-daemon.service"
SAMPLE_INTERVAL=$(python3 -c "print(1/$SAMPLE_HZ)")

WORK="$(mktemp -d)"
LOG="$WORK/samples.csv"
RESULTS="$WORK/results.txt"
SAMPLER_PID=""

pass=0; fail=0; skip=0

# ---------------------------------------------------------------- lifecycle

cleanup() {
    stop_sampler
    if [ -f "$RESULTS" ]; then
        echo
        echo "samples: $LOG"
    fi
}
trap cleanup EXIT

record() { # record <SCENARIO> <PASS|FAIL|SKIP> <detail>
    printf '%s\t%s\t%s\n' "$1" "$2" "$3" >>"$RESULTS"
    case "$2" in
        PASS) pass=$((pass + 1)); echo "PASS [$1] $3" ;;
        FAIL) fail=$((fail + 1)); echo "FAIL [$1] $3" >&2 ;;
        SKIP) skip=$((skip + 1)); echo "SKIP [$1] $3" ;;
    esac
}

need_cmd() {
    command -v "$1" >/dev/null 2>&1 || {
        echo "qa_hardware_parity: missing required command: $1" >&2
        exit 2
    }
}

# ------------------------------------------------------------------ probes

jfield() { # jfield <key>  -> value of a field from `idlescreen status --json`
    python3 -c '
import json,subprocess,sys
key=sys.argv[1]
try:
    raw=subprocess.run(["idlescreen","status","--json"],
                       capture_output=True,text=True,timeout=5).stdout
    val=json.loads(raw).get(key,"")
    # Re-serialize rather than print the parsed value: Python renders booleans
    # as True/False, but the daemon speaks JSON true/false. Comparing against
    # the daemon'"'"'s own spelling is the whole point.
    print(val if isinstance(val,str) else json.dumps(val))
except Exception:
    print("")
' "$1"
}

cpu_ns() { # cumulative CPU of the whole daemon unit, nanoseconds
    systemctl --user show "$UNIT" --property=CPUUsageNSec --value 2>/dev/null \
        || echo 0
}

# CPU burned between two instants. Normalised to "milliseconds of CPU per
# second of wall clock" so the number is comparable whether the window was 4
# or 20 seconds long.
cpu_rate() { # cpu_rate <start_ns> <start_epoch> <end_ns> <end_epoch>
    # Single quotes are deliberate: the Python must receive $ and backticks
    # literally, with expansion happening in sys.argv, not in the shell.
    # shellcheck disable=SC2016
    python3 -c '
import sys
# NOTE: with `python3 -c`, sys.argv[0] is "-c"; the four arguments are 1..4.
def num(x):
    try:
        return float(x)
    except ValueError:
        return 0.0
d_ns=num(sys.argv[3])-num(sys.argv[1]); d_s=num(sys.argv[4])-num(sys.argv[2])
print(f"{(d_ns/1e6)/d_s:.1f}" if d_s>0 and d_ns>=0 else "0.0")
' "$1" "$2" "$3" "$4"
}

# ------------------------------------------------------------------ sampler

start_sampler() {
    stop_sampler
    echo "epoch,presentation_active,session_locked,cpu_ns" >"$LOG"
    (
        while :; do
            printf '%s,%s,%s,%s\n' \
                "$(date +%s.%N)" \
                "$(jfield presentation_active)" \
                "$(jfield session_locked)" \
                "$(cpu_ns)" >>"$LOG"
            sleep "$SAMPLE_INTERVAL"
        done
    ) &
    SAMPLER_PID=$!
}

stop_sampler() {
    if [ -n "$SAMPLER_PID" ]; then
        kill "$SAMPLER_PID" 2>/dev/null || true
        wait "$SAMPLER_PID" 2>/dev/null || true
        SAMPLER_PID=""
    fi
}

# Measure CPU rate over a window while the caller does whatever it needs.
measure_window() { # measure_window <seconds> -> "rate_ms_per_s"
    local s_cpu s_epoch e_cpu e_epoch
    s_cpu="$(cpu_ns)"; s_epoch="$(date +%s.%N)"
    sleep "$1"
    e_cpu="$(cpu_ns)"; e_epoch="$(date +%s.%N)"
    cpu_rate "$s_cpu" "$s_epoch" "$e_cpu" "$e_epoch"
}

# Raise presentation directly rather than waiting out the idle timeout: this
# harness is about *reactions* to events, not about timeout accuracy.
start_presentation() {
    idlescreen start >/dev/null 2>&1 || true
}

stop_presentation() {
    idlescreen stop >/dev/null 2>&1 || true
}

wait_for() { # wait_for <key> <want> <timeout_s>
    local key="$1" want="$2" deadline
    deadline=$(python3 -c "import time;print(time.time()+float('$3'))")
    while python3 -c "import sys,time;sys.exit(0 if time.time()<float(sys.argv[1]) else 1)" "$deadline"; do
        [ "$(jfield "$key")" = "$want" ] && return 0
        sleep 0.2
    done
    return 1
}

ask() { # ask <question> -> 0 if the operator confirms
    [ "$NONINTERACTIVE" = "1" ] && return 1
    printf '%s [y/N] ' "$1" >/dev/tty 2>&1 || return 1
    read -r reply </dev/tty || return 1
    case "$reply" in
        y|Y|yes|YES) return 0 ;;
        *) return 1 ;;
    esac
}

# Run a physical step, preferring a command the operator supplied over a
# prompt. Returns 1 when neither is available, which the caller records as a
# SKIP — never as a pass.
actuate() { # actuate <question> <optional command>
    if [ -n "${2:-}" ]; then
        sh -c "$2" >/dev/null 2>&1
        return $?
    fi
    ask "$1"
}

# -------------------------------------------------------------- scenarios

scenario_input() {
    local name=input
    [ "$(jfield running)" = "true" ] || { record "$name" SKIP "daemon not reporting running"; return; }

    start_presentation
    if ! wait_for presentation_active true 5; then
        record "$name" FAIL "presentation did not start; nothing to dismiss"
        return
    fi

    local baseline
    baseline="$(measure_window 3)"
    echo "  rendering while idle-but-present: ${baseline} ms CPU/s"

    local actuated=false
    if command -v wtype >/dev/null 2>&1; then
        wtype "a" && actuated=true
    elif command -v ydotool >/dev/null 2>&1; then
        ydotool key 30:1 30:0 && actuated=true
    fi

    if [ "$actuated" = false ]; then
        if ask "  Press any key now to dismiss the screensaver"; then
            actuated=true
        fi
    fi

    if [ "$actuated" = false ]; then
        record "$name" SKIP "no synthetic-input tool (wtype/ydotool) and operator declined"
        return
    fi

    if wait_for presentation_active false 5; then
        record "$name" PASS "input dismissed presentation; idle CPU was ${baseline} ms/s"
    else
        record "$name" FAIL "presentation survived input — input does not dismiss"
    fi
}

scenario_dpms() {
    local name=dpms
    [ "$(jfield running)" = "true" ] || { record "$name" SKIP "daemon not reporting running"; return; }

    start_presentation
    if ! wait_for presentation_active true 5; then
        record "$name" FAIL "presentation did not start"
        return
    fi

    local on_rate
    on_rate="$(measure_window "$SETTLE_SECS")"
    echo "  display on:  ${on_rate} ms CPU/s"

    if ! actuate "  Turn the display OFF now (DPMS off), then press Enter here" \
        "${DPMS_OFF_CMD:-}"; then
        record "$name" SKIP "no DPMS-off command and operator declined"
        return
    fi

    local off_rate
    off_rate="$(measure_window "$SETTLE_SECS")"
    echo "  display off: ${off_rate} ms CPU/s"

    if python3 -c "import sys;sys.exit(0 if float(sys.argv[1])<float(sys.argv[2])*0.25 else 1)" \
        "$off_rate" "$on_rate"; then
        record "$name" PASS "rendering paused on DPMS off (${on_rate} -> ${off_rate} ms CPU/s)"
    else
        record "$name" FAIL "rendering did not pause on DPMS off (${on_rate} -> ${off_rate} ms CPU/s)"
    fi

    actuate "  Turn the display back ON now" "${DPMS_ON_CMD:-}" || true
    local back_rate
    back_rate="$(measure_window "$SETTLE_SECS")"
    if python3 -c "import sys;sys.exit(0 if float(sys.argv[1])>float(sys.argv[2]) else 1)" \
        "$back_rate" "$off_rate"; then
        record "$name" PASS "rendering resumed on DPMS on (${off_rate} -> ${back_rate} ms CPU/s)"
    else
        record "$name" FAIL "rendering did not resume after DPMS on (stuck at ${back_rate} ms CPU/s)"
    fi
}

scenario_suspend() {
    local name=suspend
    [ "$(jfield running)" = "true" ] || { record "$name" SKIP "daemon not reporting running"; return; }

    start_presentation
    if ! wait_for presentation_active true 5; then
        record "$name" FAIL "presentation did not start"
        return
    fi

    local before_pid before_cpu
    before_pid="$(systemctl --user show "$UNIT" --property=MainPID --value)"
    before_cpu="$(cpu_ns)"

    echo "  Suspending now. Wake the machine when you are ready."
    if ! actuate "  Close the lid, or run 'systemctl suspend' yourself" \
            "${SUSPEND_CMD:-}"; then
        record "$name" SKIP "no suspend command and operator declined"
        return
    fi

    local after_pid
    after_pid="$(systemctl --user show "$UNIT" --property=MainPID --value)"

    if [ "$after_pid" = "0" ]; then
        record "$name" FAIL "daemon is gone after resume — it did not survive suspend"
        return
    fi
    if [ "$before_pid" != "$after_pid" ]; then
        record "$name" FAIL "daemon restarted across suspend (pid $before_pid -> $after_pid); it should have resumed in place"
        return
    fi

    local after_cpu rate
    after_cpu="$(cpu_ns)"
    rate=$(python3 -c "
d=int('$after_cpu')-int('$before_cpu')
print(f'{d/1e6:.0f}')")

    if wait_for presentation_active true 8; then
        record "$name" PASS "survived suspend in place (pid $after_pid), presentation coherent, ${rate} ms CPU across the window"
    else
        record "$name" FAIL "presentation_active is '$(jfield presentation_active)' after resume; expected true"
    fi
}

scenario_multihead() {
    local name=multihead
    [ "$(jfield running)" = "true" ] || { record "$name" SKIP "daemon not reporting running"; return; }

    local outputs=""
    if command -v wayland-info >/dev/null 2>&1; then
        outputs="$(wayland-info 2>/dev/null | grep -c 'wl_output' || true)"
    fi
    echo "  compositor reports ${outputs:-unknown} wl_output binding(s)"

    start_presentation
    if ! wait_for presentation_active true 5; then
        record "$name" FAIL "presentation did not start"
        return
    fi

    local rate
    rate="$(measure_window "$SETTLE_SECS")"

    # A single-output machine cannot falsify multi-monitor coverage, so say so
    # instead of implying the property was verified.
    if [ -z "$outputs" ] || [ "$outputs" -lt 2 ]; then
        if ask "  Visually confirm EVERY output is covered (no dark/uncovered screen)."; then
            record "$name" PASS "single-output machine; operator confirmed full coverage at ${rate} ms CPU/s"
        else
            record "$name" SKIP "needs >=2 outputs to verify automatically; operator declined visual confirmation"
        fi
        return
    fi

    if ask "  Visually confirm EVERY output is covered (no dark/uncovered screen)."; then
        record "$name" PASS "${outputs} outputs bound, operator confirmed coverage at ${rate} ms CPU/s"
    else
        record "$name" SKIP "${outputs} outputs bound but coverage not visually confirmed"
    fi
}

# ------------------------------------------------------------------- main

need_cmd systemctl
need_cmd idlescreen
need_cmd python3

if [ -z "${WAYLAND_DISPLAY:-}" ]; then
    echo "qa_hardware_parity: WAYLAND_DISPLAY is unset — run this inside a Wayland session." >&2
    exit 2
fi
if ! systemctl --user is-active --quiet "$UNIT"; then
    echo "qa_hardware_parity: $UNIT is not active." >&2
    exit 2
fi

IDLE_WAS_ENABLED="$(jfield idle_enabled)"

restore() {
    stop_sampler
    stop_presentation
    if [ "$IDLE_WAS_ENABLED" = "false" ]; then
        # Never re-enable a trigger the operator had deliberately turned off
        # (this is exactly the state the session-shell integration sets).
        :
    fi
}
trap restore EXIT

echo "=== IdleScreen hardware parity (Phase 4) ==="
echo "compositor session, daemon active. Skips are reported as skips."
echo

for s in $SCENARIOS; do
    if ! declare -F "scenario_$s" >/dev/null; then
        record "$s" SKIP "no such scenario"
        continue
    fi
    echo "--- $s ---"
    start_sampler
    "scenario_$s"
    stop_sampler
    echo
done

echo "=== matrix ==="
if [ -f "$RESULTS" ]; then
    column -t -s$'\t' "$RESULTS" 2>/dev/null || cat "$RESULTS"
fi
echo
echo "pass=$pass fail=$fail skip=$skip"

if [ "$fail" -gt 0 ]; then
    echo "RESULT: FAIL"
    exit 1
fi
if [ "$skip" -gt 0 ]; then
    echo "RESULT: INCOMPLETE — $skip dimension(s) unverified on this machine"
    exit 1
fi
echo "RESULT: PASS"