// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

// See `idle-daemon.rs` for the rationale on this being binary-local.
#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> idle_err::Result<()> {
    idle_daemon::cli_main::run()
}
