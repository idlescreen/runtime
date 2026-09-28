// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

// See `idle-daemon.rs` for the rationale on this being binary-local.
// mimalloc is pulled in via `[target.'cfg(target_env = "gnu")'.dependencies]`
// in `idle-daemon/Cargo.toml`, so this `#[global_allocator]` is unconditional
// on glibc Linux. musl/BSD/macOS skip the static and use the system allocator.
#[cfg(target_env = "gnu")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> idle_err::Result<()> {
    idle_daemon::cli_main::run()
}
