# runtime

[![studio2201 gate](https://github.com/idlescreen/runtime/actions/workflows/studio2201.yml/badge.svg)](https://github.com/idlescreen/runtime/actions/workflows/studio2201.yml)

The engine — idle daemon, sandboxed plugin host, D-Bus API, and the saver
ABI every plugin builds against. Part of
[IdleScreen](https://idlescreen.github.io) — modular Wayland screensavers
for Linux.

| Path | Role |
|---|---|
| `idle-daemon/` | The service: idle monitoring, presentation sessions, config, D-Bus API, plugin orchestration |
| `idle-runner/` | Sandboxed plugin host: manifest + signature verification, capability gates, watchdog, cell/GPU raster |
| `idle-api/` | Plugin ABI: savers link against this (`param()`, palette/system-info callbacks) |
| `crates/wayland-idle` | `ext-idle-notify` idle detection |
| `crates/wayland-present` | `zwlr_layer_shell` presentation, output topology, overlays |
| `crates/idle-dbus` | D-Bus client helpers + systemd service lifecycle |
| `crates/idle-ipc` | Daemon↔runner wire protocol |
| `crates/idle-upscaler` | CPU frame upscaler |
| `crates/idle-err` | Shared error plumbing (`Result`, `Context`, `bail!`/`ensure!`, `{:#}` chaining) |
| `crates/idle-log` | Shared logging (`RUST_LOG` filter, `error!`..`trace!`, journald mirroring) |

## Install

Ships with the `idlescreen` product package. On its own:

```sh
idlescreen install runtime
```

The daemon runs as `idle-daemon` under `systemctl --user`.

## Commands

Driven through the `idlescreen` router:

```sh
idlescreen doctor --fix   # diagnose + repair the daemon
idlescreen preview storm  # fullscreen preview
idlescreen update         # upgrade packages
idlescreen tui            # runtime configuration
```

## License

Apache-2.0 · © 2026 IdleScreen

---

<div align="center">

[![Necrometer](necrometer.svg)](https://necrometer.dev/?u=idlescreen)

</div>
