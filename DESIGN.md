# DESIGN.md — IdleScreen product contract

**Headline:** A Unix screensaver. We don't lock, dim, or authenticate —
we render content during idle and yield back cleanly.

**Org:** [github.com/idlescreen](https://github.com/idlescreen)
**Human brand:** IdleScreen · **Ship names:** `idle-*` / `idlescreen*`
**Process:** `AGENTS.md` (used while coding) · gates in `scripts/`

---

## What we are

> A screensaver is ambient software — it runs when no one is watching.
> We treat that as a privilege: do one thing well, get out of the way,
> compose with the rest of the desktop.

Three claims:

- **We are a screensaver.** Not a lock, not a power manager, not an
  auth gate.
- **We are composable.** Other tools handle the *consequences* of
  being idle.
- **We are private by default.** No network, no audio capture, no
  telemetry.

## Why this shape

Four constraints:

1. **Render, don't lock.** Activity resumes → screensaver stops.
2. **Render, don't dim.** DPMS is the compositor's job.
3. **Render, don't replace.** Plug in, play, yield back.
4. **Render, don't phone home.** Plugins declare network; the user
   grants.

## Architectural lanes

The OS / compositor / window system owns the idle signal and the
screen surface. We are a guest everywhere we run:

- **Linux:** layer-shell + ext-idle-notify.
- **macOS:** `NSWindow` above-dock + IOKit idle.
- **Windows:** DXGI / Direct3D + `GetLastInputInfo`.

Plugins are pure sim; the host owns sandbox, raster, and the
control plane. No plugin talks to the OS directly.

## Product shape

- **Engine:** platform-agnostic. GPU via wgpu / Metal / DX12; CPU
  fallback. WASM and native plugins. Capability system.
- **Shims:** paper-thin, one per OS. Idle + surface + sandbox init.
  Replaceable without touching the engine.
- **Plugins:** one effect per crate / module. Declarative manifest
  with capabilities. Sandboxed at launch.

## Cross-platform bet

Engine is pure Rust. Shims are thin. WASM plugins compile once.
**Headless render mode** ships the engine's visual output to
PNG / MP4 / stdout on any platform — that's what makes the
Fedora-only test host survivable.

## Distribution shape

| Family | Package |
|--------|---------|
| Linux RPM (Fedora / RHEL) | `idlescreen` |
| Linux DEB (Debian / Ubuntu) | `idlescreen` |
| Linux Arch | `idlescreen` |
| macOS | `idlescreen` (Homebrew tap) |
| Windows | `idlescreen` (scoop / winget / MSI) |

Plugins distributed separately. Sandbox profile setup is part of
install, not user-configured.

## Privacy posture (defaults)

- **No network** from any plugin without capability grant.
- **No audio capture** without capability grant.
- **No telemetry.** It doesn't exist.
- **No filesystem read** of user data outside plugin-declared dirs.

## Efficiency posture (always)

- Pause render when monitor is off / DPMS.
- Frame-rate cap on battery.
- Per-saver GPU / CPU / memory budget.
- Watchdog on render loop and on each plugin.

## Done bars

- Workspace tests green; chaos suite green; line lock ≤256.
- Publish gate green for any channel claim.
- Install fail-closed on incomplete planned package set.
- Plugin capability declarations verified at install.
- Headless render mode produces deterministic output for a given
  seed.

## See also

- `AGENTS.md` — house laws & engineering standards (used while
  coding)

