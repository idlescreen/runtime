# idlescreen: House Laws & Agent Engineering Standards (v1)

This document is the **single canonical source of truth** for all code, architecture, and system integration standards across the `idlescreen` organization. Every human contributor and AI agent must strictly follow these rules without exception.

---

## 1. The Page Rule (Code Layout & Sizing)

A **page** is one committed Rust file. Every page holds one idea, fits in one head, and carries its own weight. This rule is enforced by an automated integration test (`page_rule`) under `cargo test`: red pages fail the build.

### Hard Sizing Invariants
- **16–256 Lines**: Every committed `.rs` file must be between **16 and 256 lines**, counted as `content.lines().count()` (blank lines and comments count).
- **Shim Exemption (Floor Only)**: A file is a shim when every code line is a module declaration or re-export: after dropping blank lines, comments (`//`, `//!`, `///`), and attributes (`#[...]`), each remaining line must start with `mod `, `use `, or `pub` (covers `pub mod` / `pub use`). Shims skip the 16-line floor. The 256-line ceiling still strictly applies.
- **Directory Density (≤ 8 files)**: At most **8 `.rs` files per directory**, tests included. Crowded directories must split into functional subdirectories grouped by area.
- **Banned File Names (Name the function, not the drawer)**:
  `util.rs`, `utils.rs`, `helper.rs`, `helpers.rs`, `common.rs`, `misc.rs`, `shared.rs`, `base.rs`, `core.rs`.

### Splitting, Folding, and Naming
- **Over 256 lines**: Split along functional boundaries into a new subdirectory with a shim `mod.rs`. One page = one verb or one wholly owned noun.
- **Under 16 lines (and not a shim)**: Fold into its closest sibling or absorb into its single caller/callee. Never pad lines to reach 16.
- **Blobs Out**: Test fixtures and static data must live in dedicated files loaded via `include_str!` or `include_bytes!`, never inline literals.
- **Systems & Trust Naming**:
  - The tree mirrors the running system: paths read as subsystems (`daemon/`, `presenter/`, `runner/`, `plugin/`, `upscaler/`).
  - Action pages lead with a verb: `verify_peer.rs`, `admit_request.rs`, `render_frame.rs`.
  - State pages name what they own: `session_state.rs`, `inhibitor_table.rs`.
  - Boundary pages speak trust verbs: `verify`, `admit`, `attest`, `seal`, `enforce`, `audit`.

---

## 2. Pure Rust & Crash-Resilience Standards

### The Zero-Panic Invariant
- **No Bare `.unwrap()` or `.expect()` in Production Code**: Long-running daemons must never panic on malformed inputs, socket disconnects, or unexpected journal lines.
- Always propagate errors via domain-typed `Result<T, E>` using `thiserror` or `anyhow`.
- `.unwrap()` and `.expect()` are permitted **only** inside `#[cfg(test)]` modules.

### The `unsafe` Discipline
- **`#![deny(unsafe_code)]` by default** across all crate roots.
- When raw syscalls, shared memory, or Wayland FFI are strictly required (e.g. `rustix`, `libc`, `memfd`), `#![allow(unsafe_code)]` is permitted **only on that specific page**.
- **Mandatory `// SAFETY:` Comment**: Every `unsafe { ... }` block must be immediately preceded by a comment documenting the exact pointer validity, lifetime invariant, and kernel/Wayland error contract.

### Async / Event-Loop Protection
- **Zero Blocking Locks Across Yield Boundaries**: Never hold a `std::sync::Mutex` or `std::sync::RwLock` across event processing or `.await` boundaries.
- **Cancellation Safety**: Background threads and event loops selecting on channels or timeouts must be clean and cancellation-safe. Resource cleanup must be encapsulated in **RAII `Drop` guards**, not manual cleanup calls.

### Pure Rust Dependencies
- Rely strictly on pure Rust crates: `zbus` (D-Bus), `wayland-client` (Wayland protocol bindings), and `rustix` (Linux syscalls) where possible.

---

## 3. Native systemd Citizenship

`idlescreen` daemons run as first-class systemd user/system services. They must adhere to native systemd lifecycle and security primitives:

### Socket Activation & Descriptors (`$LISTEN_FDS`)
- Inspect inherited file descriptors if passed via systemd. Native binding is strictly a local debug fallback.

### Lifecycle & Heartbeat Notifications (`sd_notify`)
- **Accurate `READY=1`**: Never emit `READY=1` until all sockets, Wayland globals, and channels are fully initialized and ready.
- **Dynamic `WATCHDOG=1`**: When watchdog is enabled, heartbeats must be gated on internal healthchecks, never a blind background loop.
- **Immediate `STOPPING=1`**: Emit `STOPPING=1` as the very first action upon intercepting `SIGTERM` or `SIGINT`.

### Standardized Exit Codes (`sysexits.h`)
- On fatal termination, return standard systemd exit codes:
  - `78` (`EX_CONFIG`): Configuration or policy validation failure. Systemd will mark the unit failed without thrashing.
  - `69` (`EX_UNAVAILABLE`): Missing required compositor protocol or device.
  - `0` (`EX_OK`): Clean shutdown on signal.

### Standardized System Directories
- Never hardcode absolute system paths in daemon code. Use standard systemd and XDG environment variables:
  - `$XDG_RUNTIME_DIR` / `$RUNTIME_DIRECTORY` (default: `/run/user/<uid>/idle`)
  - `$XDG_STATE_HOME` / `$STATE_DIRECTORY` (default: `~/.local/state/idle`)
  - `$XDG_CONFIG_HOME` / `$CONFIGURATION_DIRECTORY` (default: `~/.config/idle`)

---

## 4. Linux Kernel & Zero-Trust Safety

### Non-Blocking Kernel Telemetry & Inotify
- Never busy-loop reading `/sys/class/power_supply` or `/proc/`.
- Telemetry readers must use non-blocking event-driven monitors (inotify/epoll) or bounded ticks.

### File Descriptor Discipline (`O_CLOEXEC`)
- Every socket, pipe, and file descriptor opened must set `O_CLOEXEC` / `SOCK_CLOEXEC`. Descriptors must never leak across `fork`/`exec` boundaries into plugin processes.

### Shared Memory Sealing (`memfd_create`)
- Frame buffers and plugin cell grids passed across IPC boundaries must use sealed shared memory file descriptors (`memfd_create`). Never pass raw unrestricted write buffers across trust boundaries.

### Safe Subprocess Execution (No Shell Execution)
- In plugin and runner hosts, **never** invoke `/bin/sh -c` or `/bin/bash -c`. Always execute discrete binary paths directly via `std::process::Command::new(binary_path)` or `tokio::process::Command::new(binary_path)`.
- Environment variables must be cleansed before spawning child plugin processes, populating only declared keys.

---

## 5. Verification & QA Gate

Before any change is committed or marked complete:
1. **Compilation**: Clean compilation with zero warnings under `cargo check --workspace`.
2. **Clippy Quality**: `cargo clippy --workspace --all-targets -- -D warnings` must pass cleanly.
3. **Test Suite**: `cargo test --workspace` must pass completely.
4. **Structured Tracing**: No bare `println!` or `eprintln!` in daemons. All logging must use structured `tracing` macros with field keys.
