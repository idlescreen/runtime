// SPDX-License-Identifier: Apache-2.0
// perf: T3 · metric: bounded single-pass work; no syscalls, no locks, no allocation on the steady path · check: review
// Copyright 2026 IdleScreen

// mimalloc is the default allocator on glibc Linux (x86_64 + aarch64);
// pulled in via `[target.'cfg(target_env = "gnu")'.dependencies]` in
// `idle-daemon/Cargo.toml`. musl static builds, FreeBSD, and macOS keep
// the system allocator automatically. On glibc the per-CPU heap wins
// are typically 1.5–2× on high-allocation-rate workloads (per-frame
// Vec<u8> in the render path, Arc bumps in the saver→presenter pipe).
//
// Why this lives in the binary, not the library: `#[global_allocator]`
// is a process-global setting. Putting it in `idle_daemon::cli_main`
// would force every consumer of the library (tests, benches, downstream
// tools) to inherit the choice. Keeping it in the binary crate is the
// blast-radius-small pattern.
#[cfg(target_env = "gnu")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> idle_err::Result<()> {
    idle_daemon::cli_main::run()
}
