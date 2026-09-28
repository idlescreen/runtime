// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Bench harness for `wayland-present`. Backs the T2 pages that live in
//! this crate (tier table: `.github/RULES.md` §4).
//!
//! These pages are **T2 — benched on demand, not gated**. They sit on
//! the frame path and are lock/alloc/syscall-sensitive, but none is
//! currently in `perf-baseline.json`, and this target is deliberately
//! left out of `.github/workflows/perf.yml` so CI time is spent on the
//! T1 pages that are actually gated. Run it by hand when touching the
//! overlay hot path:
//!
//! ```bash
//! cargo bench -p wayland-present --bench hot_path
//! ```
//!
//! Covered pages:
//!
//! - `frame_pool`      — buffer acquire/release around one Wayland commit
//! - `overlay::epoll`  — epoll setup and the self-wake eventfd drain
//! - `drop_presenter`  — the bounded join on presenter teardown
//!
//! Every page named by a `// perf:` label in this crate has a bench
//! here; `scripts/check-perf-labels.sh` fails CI if one goes missing.

use std::hint::black_box as bb;
use std::sync::Arc;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use wayland_present::bench_exports::{
    drain_eventfd, empty_frame_pool, epoll_ctl_add, get_frame_buffer, join_with_timeout,
    make_epoll, return_frame_buffer,
};

/// One 1920×1080 BGRA frame — the buffer size the overlay recycles.
const FRAME_BYTES: usize = 1920 * 1080 * 4;

/// T2 · `frame_pool` — the per-frame acquire/release. Steady state is
/// one `Arc::try_unwrap` and one `VecDeque` push/pop, no allocation.
fn bench_frame_pool(c: &mut Criterion) {
    let pool = empty_frame_pool();
    let mut g = c.benchmark_group("frame_pool");
    g.throughput(criterion::Throughput::Bytes(FRAME_BYTES as u64));

    // Cold: the pool is empty, so this allocates a fresh zeroed buffer.
    g.bench_function("get_cold_alloc", |b| {
        b.iter(|| bb(get_frame_buffer(&pool, FRAME_BYTES)));
    });

    // Warm: a buffer is in flight and gets recycled. This is the branch
    // that runs on every frame after the first.
    let warm = empty_frame_pool();
    g.bench_function("get_return_cycle", |b| {
        b.iter(|| {
            let mut buf = get_frame_buffer(&warm, FRAME_BYTES);
            buf[0] = 1;
            return_frame_buffer(&warm, Arc::new(buf));
        });
    });
    g.finish();
}

/// T2 · `overlay::epoll` — epoll setup and the eventfd drain. Both run
/// outside the per-frame steady state (setup at thread start, drain on
/// every self-wake), but the drain *is* per-wake and is syscall-bound.
fn bench_epoll(c: &mut Criterion) {
    let mut g = c.benchmark_group("epoll");

    g.bench_function("make_epoll", |b| {
        b.iter(|| {
            // Close immediately: this measures setup, and leaking the fd
            // would exhaust the process limit partway through a run.
            let fd = make_epoll().expect("epoll_create1");
            bb(fd);
            // SAFETY: `fd` is a fresh, owned epoll descriptor.
            unsafe { libc::close(fd) };
        });
    });

    // A non-blocking eventfd stands in for the Wayland socket and for
    // the daemon's self-wake counter.
    // SAFETY: plain syscall setup; the fds are closed at the end.
    let ep = make_epoll().expect("epoll_create1");
    let ev = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
    assert!(
        ev >= 0,
        "eventfd failed: {}",
        std::io::Error::last_os_error()
    );

    g.bench_function("epoll_ctl_add", |b| {
        b.iter(|| {
            epoll_ctl_add(ep, ev, libc::EPOLLIN, 1).expect("epoll_ctl");
            bb(1);
        });
    });

    g.bench_function("drain_eventfd", |b| {
        b.iter(|| {
            // Refill the counter, then drain it exactly the way the
            // event thread does: read 8 bytes until EAGAIN.
            let one: u64 = 1;
            // SAFETY: writing 8 bytes to a valid eventfd counter.
            let n = unsafe { libc::write(ev, std::ptr::from_ref(&one).cast(), 8) };
            assert_eq!(n, 8, "eventfd write failed");
            drain_eventfd(ev);
        });
    });

    g.finish();
    // SAFETY: closing descriptors this bench created.
    unsafe {
        libc::close(ev);
        libc::close(ep);
    }
}

/// T2 · `drop_presenter` — the bounded join. A real teardown spawns a
/// watcher thread, so this is expensive by construction; the point of
/// measuring it is to see the healthy-compositor cost, which is what
/// bounds daemon shutdown on the common path.
fn bench_join_with_timeout(c: &mut Criterion) {
    let mut g = c.benchmark_group("drop_presenter");
    g.bench_function("join_fast_thread", |b| {
        b.iter(|| {
            let handle = std::thread::spawn(|| {});
            join_with_timeout(handle, Duration::from_secs(2));
        });
    });
    g.finish();
}

criterion_group!(
    benches,
    bench_frame_pool,
    bench_epoll,
    bench_join_with_timeout,
);
criterion_main!(benches);
