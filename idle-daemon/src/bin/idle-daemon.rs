// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

// mimalloc behind a feature flag (default off). Builds without the
// feature use the system allocator and remain musl-portable. Opt in
// via `cargo build --release --features mimalloc` on a glibc Linux
// host where mimalloc's per-CPU heaps give 1.5–2× wins on
// high-allocation-rate workloads.
//
// Why this lives in the binary, not the library: `#[global_allocator]`
// is a process-global setting. Putting it in `idle_daemon::cli_main`
// would force every consumer of the library (tests, benches, downstream
// tools) to inherit the choice. Keeping it in the binary crate is the
// blast-radius-small pattern.
#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> idle_err::Result<()> {
    idle_daemon::cli_main::run()
}
