// SPDX-License-Identifier: MIT
// perf: T3 · metric: bounded single-pass work; no syscalls, no locks, no allocation on the steady path · check: review
// Copyright 2026 IdleScreen

// Binary entry point for `idlescreen-daemon`.
//
// `idle-daemon.rs` (the library crate's root) has the full module
// layout and the daemon's documentation. This binary-local file
// exists for one reason only: install a global allocator (mimalloc
// on glibc Linux) before any daemon code allocates, and then hand
// off to `idle_daemon::cli_main::run`.
//
// mimalloc is pulled in via
// `[target.'cfg(target_env = "gnu")'.dependencies]` in
// `idle-daemon/Cargo.toml`, so this `#[global_allocator]` is
// unconditional on glibc Linux. musl/BSD/macOS skip the static and
// use the system allocator — the daemon is portable, but the
// allocation-per-frame win is glibc-only.
//
// What this binary does NOT do:
//   - Does not load config, parse CLI flags, or open D-Bus. All of
//     that lives in `idle_daemon::cli_main::run`, so unit tests can
//     exercise the daemon's library surface without spawning a
//     process.
//   - Does not install a `#[ctor]` to pre-warm anything. The daemon
//     measures its cold-start against `tick_loop_until_shutdown`
//     from `runtime::initialize_runtime`; pre-warming belongs in
//     that function, not in the binary entry.
//   - Does not `init` a logger here. `cli_main::run` owns the
//     `idle-log` init so the library tests can opt out of stderr
//     noise via `RUST_LOG` env (the canonical idiom in the rest of
//     the org).
#[cfg(target_env = "gnu")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> idle_err::Result<()> {
    idle_daemon::cli_main::run()
}
