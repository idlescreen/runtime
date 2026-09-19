# runtime

<div align="center">

| Security Pillar | Verification Badge |
| :--- | :---: |
| **Platform Standard** | [![secured by studio2201][b-studio]][u-home] |
| **Credential Defense** | [![snip][b-snip]][u-snip] |
| **Supply Chain Surface** | [![vigil][b-vigil]][u-vigil] |
| **Post-Quantum Cryptography** | [![aegis][b-aegis]][u-aegis] |
| **Build Provenance & SLSA** | [![proven][b-proven]][u-proven] |
| **Repository Governance** | [![boneyard][b-boneyard]][u-boneyard] |

[b-studio]: https://img.shields.io/badge/secured%20by-studio2201-2f6f5e?logo=shield
[u-home]: https://studio2201.com
[b-snip]: https://img.shields.io/badge/snip-0%20secrets-2f6f5e?logo=shield
[u-snip]: https://studio2201.com/snip
[b-vigil]: https://img.shields.io/badge/vigil-0%20dependencies-2f6f5e?logo=shield
[u-vigil]: https://studio2201.com/vigil
[b-aegis]: https://img.shields.io/badge/aegis-PQC%20compliant-2f6f5e?logo=shield
[u-aegis]: https://studio2201.com/aegis
[b-proven]: https://img.shields.io/badge/proven-ML--DSA--65%20verified-2f6f5e?logo=shield
[u-proven]: https://studio2201.com/proven
[b-boneyard]: https://img.shields.io/badge/boneyard-maintained-2f6f5e?logo=shield
[u-boneyard]: https://studio2201.com/boneyard

</div>

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
