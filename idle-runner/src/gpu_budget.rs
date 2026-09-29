// SPDX-License-Identifier: MIT

//! GPU budget enforcement (Sprint 04 G1).
//!
//! Per-vendor tooling, default **off** (`IDLE_GPU_BUDGET=1` to enable).
//! Failure mode: vendor tool missing → unenforced + warning, never silent OK.
//!
//! Supported backends:
//! - NVIDIA: `nvidia-smi pmon -c 1` polled periodically.
//! - Intel:  `intel_gpu_top -J -s 1` JSON snapshot.
//! - AMD:    `amdgpu_top -n 1` ASCII snapshot, scanning for utilization %.
//!
//! Privacy posture (DESIGN §"Privacy posture"): no audio/network here.
//! `nvidia-smi pmon` only reports per-process GPU utilization; nothing else.

mod backends;

use backends::{sample_amd, sample_intel, sample_nvidia};

use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

/// Default GPU sample interval.
pub const DEFAULT_SAMPLE_INTERVAL: Duration = Duration::from_secs(2);
/// Default GPU budget as a percentage of one GPU.
pub const DEFAULT_GPU_QUOTA_PCT: u32 = 75;
/// Hard ceiling multiplier (over quota for N samples → drop).
pub const DEFAULT_HARD_MULTIPLIER: u32 = 2;
/// Number of consecutive over-budget samples that trigger a drop.
pub const DEFAULT_HARD_STREAK: u32 = 3;
/// Consecutive sample failures before the health watchdog logs a warning.
/// Vendor tools occasionally fail (driver hiccup, permissions); a single
/// failure is debug-logged. After `DEFAULT_FAILURE_STREAK` consecutive
/// failures we surface a `warn!` so the operator sees the budget is silent.
pub const DEFAULT_FAILURE_STREAK: u32 = 5;

/// Status of the GPU budget probe at attach time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuStatus {
    /// A vendor tool was found and we will poll it.
    Active(GpuBackend),
    /// No vendor tool available; budget unenforced.
    Unavailable,
}

/// Vendor tool driving the GPU budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuBackend {
    Nvidia,
    Intel,
    Amd,
}

impl GpuBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            GpuBackend::Nvidia => "nvidia-smi",
            GpuBackend::Intel => "intel_gpu_top",
            GpuBackend::Amd => "amdgpu_top",
        }
    }
}

/// Per-plugin GPU budget.
pub struct GpuBudget {
    backend: GpuBackend,
    sample_interval: Duration,
    quota_pct: u32,
    hard_multiplier: u32,
    hard_streak: u32,
    over_streak: u32,
    last_sample: Instant,
    last_usage_pct: u32,
    /// Consecutive failed samples (vendor tool errored). Health watchdog
    /// reads this to surface a `warn!` when the budget is effectively
    /// silenced by a broken tool rather than running clean.
    consecutive_failures: u32,
}

impl GpuBudget {
    /// Detect the available backend. Returns `GpuStatus::Unavailable` when
    /// none is on `PATH`. Operators opt in via `IDLE_GPU_BUDGET=1`; this
    /// function never spawns the tool.
    pub fn detect() -> GpuStatus {
        for tool in ["nvidia-smi", "intel_gpu_top", "amdgpu_top"] {
            if tool_on_path(tool) {
                #[allow(clippy::unreachable)]
                let backend = match tool {
                    "nvidia-smi" => GpuBackend::Nvidia,
                    "intel_gpu_top" => GpuBackend::Intel,
                    "amdgpu_top" => GpuBackend::Amd,
                    _ => unreachable!(),
                };
                return GpuStatus::Active(backend);
            }
        }
        GpuStatus::Unavailable
    }

    /// Construct an active budget. Caller must have observed
    /// `GpuStatus::Active` first.
    pub fn new_active(backend: GpuBackend) -> Self {
        let quota_pct = std::env::var("IDLE_GPU_QUOTA_PCT")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(DEFAULT_GPU_QUOTA_PCT)
            .clamp(1, 1000);
        let hard_multiplier = std::env::var("IDLE_GPU_HARD_MULTIPLIER")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(DEFAULT_HARD_MULTIPLIER)
            .max(1);
        let hard_streak = std::env::var("IDLE_GPU_HARD_STREAK")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(DEFAULT_HARD_STREAK)
            .max(1);
        Self {
            backend,
            sample_interval: DEFAULT_SAMPLE_INTERVAL,
            quota_pct,
            hard_multiplier,
            hard_streak,
            over_streak: 0,
            // checked_sub: `Instant - Duration` panics when it would underflow
            // the platform's Instant epoch (early-boot process start).
            last_sample: Instant::now()
                .checked_sub(DEFAULT_SAMPLE_INTERVAL)
                .unwrap_or_else(Instant::now),
            last_usage_pct: 0,
            consecutive_failures: 0,
        }
    }

    pub fn backend(&self) -> GpuBackend {
        self.backend
    }

    pub fn quota_pct(&self) -> u32 {
        self.quota_pct
    }

    /// True when the next sample is due. Callers should poll on each tick;
    /// we throttle internally to `sample_interval`.
    pub fn sample_due(&self) -> bool {
        self.last_sample.elapsed() >= self.sample_interval
    }

    /// Run a single sample of the vendor tool. Returns the most recent GPU
    /// utilization percentage for our process (0 if unavailable or our pid
    /// is not in the listing).
    pub fn sample(&mut self) -> io::Result<u32> {
        if !self.sample_due() {
            return Ok(self.last_usage_pct);
        }
        self.last_sample = Instant::now();
        let result = match self.backend {
            GpuBackend::Nvidia => sample_nvidia(),
            GpuBackend::Intel => sample_intel(),
            GpuBackend::Amd => sample_amd(),
        };
        self.record_result(&result);
        let pct = result?;
        self.last_usage_pct = pct;
        self.over_streak = if pct > self.hard_ceiling() {
            self.over_streak.saturating_add(1)
        } else {
            0
        };
        Ok(pct)
    }

    pub fn hard_ceiling(&self) -> u32 {
        self.quota_pct * self.hard_multiplier
    }

    /// True when the budget has been over its hard ceiling for at least
    /// `hard_streak` consecutive samples. Caller should drop the plugin.
    pub fn exceeded(&self) -> bool {
        self.over_streak >= self.hard_streak
    }

    /// Most recent observed usage percentage.
    pub fn last_usage_pct(&self) -> u32 {
        self.last_usage_pct
    }

    /// Health check: true when the vendor tool has errored on the last
    /// `DEFAULT_FAILURE_STREAK` consecutive samples. Caller should emit a
    /// `warn!` so the operator sees the budget is silently degraded
    /// (broken tool, permissions, missing dev/dri node, etc.) and can set
    /// `IDLE_GPU_BUDGET=0` to disable until resolved.
    pub fn unhealthy(&self) -> bool {
        self.consecutive_failures >= DEFAULT_FAILURE_STREAK
    }

    /// Record a sample result. On `Err`, the failure streak increments.
    /// On `Ok`, it resets. Exposed so callers can keep the streak in sync
    /// even when they sample through paths other than `sample()` (e.g.,
    /// for tests).
    pub fn record_result(&mut self, result: &io::Result<u32>) {
        match result {
            Ok(_) => self.consecutive_failures = 0,
            Err(_) => self.consecutive_failures = self.consecutive_failures.saturating_add(1),
        }
    }
}

fn tool_on_path(tool: &str) -> bool {
    if let Ok(paths) = std::env::var("PATH") {
        for dir in paths.split(':') {
            if Path::new(dir).join(tool).exists() {
                return true;
            }
        }
    }
    false
}

/// Decide whether the operator has opted into GPU budget enforcement.
pub fn gpu_budget_enabled() -> bool {
    std::env::var_os("IDLE_GPU_BUDGET").is_some()
}

#[cfg(test)]
#[path = "gpu_budget_tests.rs"]
mod tests;
