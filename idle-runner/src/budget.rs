// SPDX-License-Identifier: MIT

//! Per-saver CPU budget enforcement (Sprint 03 B).
//!
//! Strategy: attach the current thread to a cgroup v2 child whose `cpu.max`
//! is set to a percentage of one CPU. The kernel throttles; the session also
//! polls `cpu.stat` and drops the plugin if usage exceeds a hard ceiling.
//!
//! When cgroup v2 is unavailable or write access is denied, we fall back to
//! in-process measurement only — the operator can force fail-closed with
//! `IDLE_REQUIRE_CPU_BUDGET=1`.
//!
//! GPU budget is not implemented; tracked as a residual (no portable
//! cross-vendor GPU usage API on Linux short of `nvidia-smi`, which is
//! vendor-specific).

use std::io;
use std::path::PathBuf;
use std::time::Instant;

/// Default CPU quota as a fraction of one CPU: 50% of one core.
pub const DEFAULT_CPU_QUOTA_PCT: u32 = 50;

/// Default hard ceiling: 2x the throttle quota over a 5-second window.
pub const DEFAULT_HARD_LIMIT_MULTIPLIER: u32 = 2;
pub const DEFAULT_HARD_LIMIT_WINDOW_SECS: u64 = 5;

/// Whether the kernel-side cgroup enforcement is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetStatus {
    /// Attached to a cgroup v2 child; kernel will throttle.
    Enforced,
    /// cgroup v2 unavailable / unwritable; only in-process measurement runs.
    Unenforced,
}

/// Per-plugin CPU budget.
#[derive(Debug)]
pub struct CpuBudget {
    cgroup_dir: Option<PathBuf>,
    started: Instant,
    start_proc_cpu_micros: u64,
    /// Soft quota (`cpu.max` quota field).
    quota_us: u64,
    /// Period (`cpu.max` period field).
    period_us: u64,
    /// Hard ceiling: drop the plugin if usage_us exceeds `hard_limit_us` over
    /// the last `hard_window` seconds.
    hard_limit_us: u64,
    hard_window_secs: u64,
    status: BudgetStatus,
    // --- Sample cache (closes M-A19: per-tick open+read+parse of /proc). ---
    // Interior mutability so `usage_micros(&self)` can update without
    // disturbing the public API. Sample period: SAMPLE_CACHE_MS.
    last_sample_micros: std::cell::Cell<u64>,
    last_sample_at: std::cell::Cell<Option<Instant>>,
}

const SAMPLE_CACHE_MS: u64 = 250;

impl CpuBudget {
    /// Attach a cgroup v2 child named `idle/<plugin_id>` and apply the
    /// default quota. Returns `BudgetStatus::Unenforced` when the kernel-side
    /// throttle could not be applied; in that case the budget still measures
    /// in-process CPU and reports against the same hard ceiling.
    pub fn attach(plugin_id: &str) -> io::Result<Self> {
        let quota_pct = std::env::var("IDLE_CPU_QUOTA_PCT")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(DEFAULT_CPU_QUOTA_PCT)
            .clamp(1, 1000);
        let period_us: u64 = 100_000; // 100 ms
        let quota_us = (period_us as u128 * quota_pct as u128 / 100) as u64;

        let (dir, status) = match try_attach_cgroup(plugin_id, quota_us, period_us) {
            Ok(d) => (Some(d), BudgetStatus::Enforced),
            Err(_) => (None, BudgetStatus::Unenforced),
        };

        let start_proc_cpu_micros = read_proc_cpu_micros().unwrap_or(0);

        Ok(Self {
            cgroup_dir: dir,
            started: Instant::now(),
            start_proc_cpu_micros,
            quota_us,
            period_us,
            hard_limit_us: quota_us * DEFAULT_HARD_LIMIT_MULTIPLIER as u64,
            hard_window_secs: DEFAULT_HARD_LIMIT_WINDOW_SECS,
            status,
            last_sample_micros: std::cell::Cell::new(0),
            last_sample_at: std::cell::Cell::new(None),
        })
    }

    pub fn status(&self) -> BudgetStatus {
        self.status
    }

    pub fn quota_us(&self) -> u64 {
        self.quota_us
    }

    pub fn period_us(&self) -> u64 {
        self.period_us
    }

    /// Total CPU time consumed by the saver (microseconds).
    ///
    /// Reads cgroup v2 `cpu.stat` when the throttle is active, otherwise
    /// `/proc/self/stat` field 14 (utime + stime). Both report cumulative time
    /// so callers compare against the budget's `started` baseline.
    pub fn usage_micros(&self) -> u64 {
        let now = Instant::now();
        if let Some(at) = self.last_sample_at.get()
            && now.duration_since(at) < std::time::Duration::from_millis(SAMPLE_CACHE_MS)
        {
            return self.last_sample_micros.get();
        }
        let proc_delta = || -> u64 {
            match read_proc_cpu_micros() {
                Ok(now) => now.saturating_sub(self.start_proc_cpu_micros),
                Err(_) => 0,
            }
        };
        let v = match &self.cgroup_dir {
            Some(dir) => read_cgroup_usage_micros(dir).unwrap_or_else(|_| proc_delta()),
            None => proc_delta(),
        };
        self.last_sample_micros.set(v);
        self.last_sample_at.set(Some(now));
        v
    }

    /// True when the plugin has been over the hard ceiling for the configured
    /// window. Caller is expected to drop the plugin session in response.
    pub fn exceeded_hard_limit(&self) -> bool {
        let elapsed = self.started.elapsed().as_secs();
        if elapsed < self.hard_window_secs {
            return false;
        }
        self.usage_micros() > self.hard_limit_us
    }

    /// Hard limit in microseconds (the ceiling against which `usage_micros` is
    /// compared once `hard_window_secs` have elapsed).
    pub fn hard_limit_us(&self) -> u64 {
        self.hard_limit_us
    }

    /// Release the cgroup child. Best-effort; cgroup v2 children with no
    /// processes are auto-removed by the kernel on release, but we rmdir
    /// when possible so the next load has a clean slate.
    pub fn release(self) {
        if let Some(dir) = &self.cgroup_dir {
            let _ = std::fs::remove_dir(dir);
        }
    }
}

/// Outcome of attaching a budget during plugin load.
#[derive(Debug)]
pub enum AttachOutcome {
    /// Attached; the kernel will throttle the saver.
    Enforced(CpuBudget),
    /// cgroup unavailable / unwritable; only in-process measurement is live.
    /// Operator may force fail-closed via `IDLE_REQUIRE_CPU_BUDGET=1`.
    Unenforced(CpuBudget),
}

/// Helper used by `idle-runner::load_path_with_options`. Strips the
/// `libscreensaver_` prefix from a `.so` path so the cgroup child name is
/// the saver stem rather than the FFI artifact name.
pub fn attach_for_path(path: &std::path::Path) -> io::Result<AttachOutcome> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("plugin");
    let plugin_id = stem.trim_start_matches("libscreensaver_").to_string();
    let budget = CpuBudget::attach(&plugin_id)?;
    Ok(match budget.status {
        BudgetStatus::Enforced => AttachOutcome::Enforced(budget),
        BudgetStatus::Unenforced => AttachOutcome::Unenforced(budget),
    })
}

/// Read the current process's user + system CPU time in microseconds.
/// `/proc/self/stat` field 14 (1-indexed) is `utime`; field 15 is `stime`;
/// both are in clock ticks. Convert via `sysconf(_SC_CLK_TCK)`.
fn read_proc_cpu_micros() -> io::Result<u64> {
    let text = std::fs::read_to_string("/proc/self/stat")?;
    // Field 1 is "comm (name)" which contains spaces — split from the right.
    let close_paren = text
        .rfind(')')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "/proc/self/stat: no ')'"))?;
    let after = &text[close_paren + 1..];
    let fields: Vec<&str> = after.split_whitespace().collect();
    // After the ')', field index resets: after[0] is field 3, after[13] is field 16.
    let utime_ticks: u64 = fields
        .get(11)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "/proc/self/stat: utime"))?;
    let stime_ticks: u64 = fields
        .get(12)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "/proc/self/stat: stime"))?;
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if hz <= 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "sysconf(_SC_CLK_TCK) returned non-positive",
        ));
    }
    let total_ticks = utime_ticks.saturating_add(stime_ticks);
    Ok((total_ticks as u128 * 1_000_000 / hz as u128) as u64)
}

#[path = "budget/cgroup.rs"]
mod cgroup;
use cgroup::{read_cgroup_usage_micros, try_attach_cgroup};

#[cfg(test)]
#[path = "budget_tests.rs"]
mod tests;
