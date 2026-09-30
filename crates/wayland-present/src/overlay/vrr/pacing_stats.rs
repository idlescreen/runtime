// SPDX-License-Identifier: MIT

//! Timing statistics and VRR mode analysis for presentation feedback.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum RefreshMode {
    FixedVsync(u32),
    AdaptiveSync { min_hz: u32, max_hz: u32 },
    Unknown,
}

#[derive(Debug, Clone, Default)]
pub struct VrrPacingStats {
    pub last_interval: Option<Duration>,
    pub frame_times: Vec<Duration>,
    pub reported_refresh_hz: Option<u32>,
}

#[allow(dead_code)]
impl VrrPacingStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_presentation(&mut self, refresh_ns: u32) {
        if refresh_ns > 0 {
            let dur = Duration::from_nanos(refresh_ns as u64);
            self.last_interval = Some(dur);
            if self.frame_times.len() >= 60 {
                self.frame_times.remove(0);
            }
            self.frame_times.push(dur);
            let hz = (1_000_000_000u64 / (refresh_ns as u64)) as u32;
            self.reported_refresh_hz = Some(hz);
        }
    }

    pub fn detect_mode(&self) -> RefreshMode {
        if self.frame_times.len() < 5 {
            return self
                .reported_refresh_hz
                .map_or(RefreshMode::Unknown, RefreshMode::FixedVsync);
        }

        let mut min_ns = u64::MAX;
        let mut max_ns = 0;
        for &t in &self.frame_times {
            let ns = t.as_nanos() as u64;
            min_ns = min_ns.min(ns);
            max_ns = max_ns.max(ns);
        }

        if min_ns > 0 && max_ns > min_ns && (max_ns - min_ns) > 2_000_000 {
            let max_hz = (1_000_000_000 / min_ns) as u32;
            let min_hz = (1_000_000_000 / max_ns) as u32;
            RefreshMode::AdaptiveSync { min_hz, max_hz }
        } else if let Some(hz) = self.reported_refresh_hz {
            RefreshMode::FixedVsync(hz)
        } else {
            RefreshMode::Unknown
        }
    }
}
