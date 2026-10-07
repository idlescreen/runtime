#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# qa_hardware_parity_tests.sh — mock-driven tests for qa_hardware_parity.sh.
#
# The parity harness itself can only run on real hardware, which means it would
# otherwise never be executed by CI and would rot silently. These tests drive it
# against mock `idlescreen` / `systemctl` / `wtype` binaries so the control flow,
# the CPU-rate comparison and the pass/fail/skip accounting are all exercised
# without a compositor.
#
# Exit 0 = all checks passed, 1 = a check failed.

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HARNESS="$HERE/qa_hardware_parity.sh"
pass=0
fail=0

ok() { echo "ok: $*"; pass=$((pass + 1)); }
bad() { echo "FAIL: $*" >&2; fail=$((fail + 1)); }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
STATE="$WORK/state"
BIN="$WORK/bin"
mkdir -p "$STATE" "$BIN"

# ------------------------------------------------------------------- mocks
# Presentation state and a CPU counter the tests advance explicitly.

mock_install() {
    printf 'true' >"$STATE/presentation"
    printf '0' >"$STATE/cpu"
    printf 'off' >"$STATE/advance"

    cat >"$BIN/idlescreen" <<'EOF'
#!/usr/bin/env bash
S="$(dirname "$0")/../state"
case "${1:-}" in
    start) printf 'true' >"$S/presentation" ;;
    stop)  printf 'false' >"$S/presentation" ;;
    status)
        pres="$(cat "$S/presentation")"
        printf '{"running":true,"idle_enabled":true,"presentation_active":%s,"preview_active":false,"session_locked":false,"inhibited":false,"current_saver":"beams"}\n' "$pres"
        ;;
    *) exit 0 ;;
esac
EOF

    cat >"$BIN/systemctl" <<'EOF'
#!/usr/bin/env bash
# The CPU counter only advances while $S/advance says "on". The DPMS test flips
# it off to model "the renderer stopped producing pixels"; the mock systemctl
# appends on every read, so any read while it is "on" burns CPU.
S="$(dirname "$0")/../state"
case "$*" in
    *CPUUsageNSec*)
        if [ "$(cat "$S/advance" 2>/dev/null || true)" = "on" ]; then
            python3 -c "
import sys
p=sys.argv[1]; v=int(open(p).read() or 0)
open(p,'w').write(str(v+20_000_000))
" "$S/cpu"
        fi
        cat "$S/cpu" ;;
    *MainPID*) echo 4242 ;;
    *is-active*) exit 0 ;;
    *) exit 0 ;;
esac
EOF

    cat >"$BIN/wtype" <<'EOF'
#!/usr/bin/env bash
# Synthetic keystroke: the harness must observe the presentation go away.
S="$(dirname "$0")/../state"
printf 'false' >"$S/presentation"
EOF

    cat >"$BIN/wayland-info" <<'EOF'
#!/usr/bin/env bash
printf 'wl_output #1\n'
printf 'wl_output #2\n'
EOF

    chmod +x "$BIN"/*
}

# Tiny helpers the DPMS/suspend scenarios invoke via DPMS_*_CMD / SUSPEND_CMD.
for helper in cpu-on cpu-off noop; do
    cat >"$BIN/$helper" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
    chmod +x "$BIN/$helper"
done
cat >"$BIN/cpu-off" <<'EOF'
#!/usr/bin/env bash
printf 'off' > "$(dirname "$0")/../state/advance"
EOF
chmod +x "$BIN/cpu-off"
cat >"$BIN/cpu-on" <<'EOF'
#!/usr/bin/env bash
printf 'on' > "$(dirname "$0")/../state/advance"
EOF
chmod +x "$BIN/cpu-on"

# Turn the mock CPU counter on or off. While on, every read burns 20ms.
cpu_on()  { printf 'on'  >"$STATE/advance"; }
cpu_off() { printf 'off' >"$STATE/advance"; }

run_harness() { # run_harness <env assignments...>   (HARNESS_BIN overrides the mock PATH dir)
    local bindir="${HARNESS_BIN:-$BIN}"
    env PATH="$bindir:$PATH" WAYLAND_DISPLAY=wayland-0 \
        SETTLE_SECS=1 SAMPLE_HZ=4 "$@" \
        bash "$HARNESS" 2>&1
}

mock_install

# 1. Without a compositor the harness must refuse to pretend it can test.
out="$(env -u WAYLAND_DISPLAY PATH="$BIN:$PATH" bash "$HARNESS" 2>&1)"
if [ $? -eq 2 ] && printf '%s' "$out" | grep -q "WAYLAND_DISPLAY is unset"; then
    ok "refuses to run outside a Wayland session (exit 2)"
else
    bad "should refuse without WAYLAND_DISPLAY; got: $out"
fi

# 2. An unknown scenario name is a SKIP, not a crash.
out="$(run_harness SCENARIOS=teleportation NONINTERACTIVE=1)"
if printf '%s' "$out" | grep -q "SKIP \[teleportation\] no such scenario"; then
    ok "unknown scenario degrades to SKIP"
else
    bad "unknown scenario should SKIP; got: $out"
fi

# 3. With no operator and no input tooling, every hardware dimension is
#    unverifiable. The harness must say so rather than claim a pass.
out="$(run_harness SCENARIOS="input dpms suspend multihead" NONINTERACTIVE=1)"
if printf '%s' "$out" | grep -q "RESULT: INCOMPLETE"; then
    ok "non-interactive run reports INCOMPLETE rather than PASS"
else
    bad "should report INCOMPLETE; got: $out"
fi

# The input dimension has a second SKIP path: no synthetic-input tool anywhere
# on PATH and nobody at the keyboard. Build a mock dir without wtype.
NOINPUT_BIN="$WORK/bin_noinput"
mkdir -p "$NOINPUT_BIN"
for f in "$BIN"/*; do
    case "$(basename "$f")" in
        wtype) ;;
        *) ln -sf "$f" "$NOINPUT_BIN/$(basename "$f")" ;;
    esac
done
out="$(HARNESS_BIN="$NOINPUT_BIN" run_harness SCENARIOS=input NONINTERACTIVE=1)"
if printf '%s' "$out" | grep -q "SKIP \[input\] no synthetic-input tool"; then
    ok "input SKIPs without wtype on PATH and without an operator"
else
    bad "input should SKIP with no input tool; got: $out"
fi

# 4. With a synthetic input tool the input scenario must actually PASS, and it
#    must do so because the presentation flag went false.
out="$(run_harness SCENARIOS=input NONINTERACTIVE=1)"
if printf '%s' "$out" | grep -q "PASS \[input\]"; then
    ok "input scenario passes when wtype dismisses the presentation"
else
    bad "input should PASS with wtype present; got: $out"
fi

# 5. The final verdict must be a matrix plus an explicit exit code.
out="$(run_harness SCENARIOS=input NONINTERACTIVE=1)"
if printf '%s' "$out" | grep -qE '^pass=[0-9]+ fail=[0-9]+ skip=[0-9]+'; then
    ok "prints a machine-greppable tally line"
else
    bad "missing tally line; got: $out"
fi

# 6. DPMS: rendering must stop burning CPU when the display goes off and start
#    again when it comes back. This is the core claim of the harness, so it is
#    driven end-to-end rather than mocked at the comparison.
cpu_on
out="$(run_harness SCENARIOS=dpms NONINTERACTIVE=1 \
    DPMS_OFF_CMD="bash $BIN/cpu-off" \
    DPMS_ON_CMD="bash $BIN/cpu-on")"
cpu_off

if printf '%s' "$out" | grep -q "PASS \[dpms\] rendering paused on DPMS off"; then
    ok "DPMS off drops CPU to the paused threshold"
else
    bad "DPMS off should show paused rendering; got: $out"
fi
if printf '%s' "$out" | grep -q "PASS \[dpms\] rendering resumed on DPMS on"; then
    ok "DPMS on brings rendering back"
else
    bad "DPMS on should resume rendering; got: $out"
fi

# 7. The inverse must FAIL: if the renderer keeps burning CPU with the display
#    off, that is a real battery bug and the harness has to say so.
cpu_on
out="$(run_harness SCENARIOS=dpms NONINTERACTIVE=1 \
    DPMS_OFF_CMD="bash $BIN/noop" \
    DPMS_ON_CMD="bash $BIN/noop")"
cpu_off
if printf '%s' "$out" | grep -q "FAIL \[dpms\] rendering did not pause on DPMS off"; then
    ok "DPMS scenario fails when rendering does not pause"
else
    bad "harness should FAIL when the renderer ignores DPMS off; got: $out"
fi

# 8. Suspend: the daemon must come back in the same process.
out="$(run_harness SCENARIOS=suspend NONINTERACTIVE=1 SUSPEND_CMD="true")"
if printf '%s' "$out" | grep -q "PASS \[suspend\] survived suspend in place"; then
    ok "suspend resumes in the same pid with a coherent presentation"
else
    bad "suspend should pass with a stable pid; got: $out"
fi

# 9. A daemon that did not survive must FAIL, not SKIP — that is the bug the
#    dimension exists to catch.
cat >"$BIN/systemctl" <<'EOF'
#!/usr/bin/env bash
S="$(dirname "$0")/../state"
case "$*" in
    *CPUUsageNSec*) cat "$S/cpu" ;;
    *MainPID*) echo 0 ;;
    *is-active*) exit 0 ;;
    *) exit 0 ;;
esac
EOF
chmod +x "$BIN/systemctl"
out="$(run_harness SCENARIOS=suspend NONINTERACTIVE=1 SUSPEND_CMD="true")"
if printf '%s' "$out" | grep -q "FAIL \[suspend\] daemon is gone after resume"; then
    ok "suspend scenario fails when the daemon dies"
else
    bad "harness should FAIL when the daemon is gone; got: $out"
fi

echo
if [ "$fail" -gt 0 ]; then
    echo "$fail check(s) failed"
    exit 1
fi
echo "all checks passed"