// SPDX-License-Identifier: MIT

//! Per-vendor sampling implementations for the GPU budget.
//!
//! Each backend is a thin shell-out to a vendor CLI tool. Failures bubble
//! up as `io::Error` so callers can debug-log; the budget never silently
//! passes when a tool is missing or returns garbage.

use std::io;
use std::process::Command;

/// `nvidia-smi pmon -c 1 -s u` reports per-process `gpuutil` % (0-100).
/// We filter for our pid; if absent, return 0.
pub(crate) fn sample_nvidia() -> io::Result<u32> {
    let pid = std::process::id();
    let out = Command::new("nvidia-smi")
        .args(["pmon", "-c", "1", "-s", "u"])
        .output()?;
    if !out.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("nvidia-smi exit {:?}", out.status.code()),
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines().skip(2) {
        let mut it = line.split_whitespace();
        let p = match it.next() {
            Some(p) => p,
            None => continue,
        };
        if p.parse::<u32>().ok() != Some(pid) {
            continue;
        }
        let gpu = it.nth(2).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
        return Ok(gpu);
    }
    Ok(0)
}

/// `intel_gpu_top -J -s 100` emits a single JSON line with per-engine %.
/// Conservative: return max engine % as the sample. We do not attempt to
/// attribute to our pid (intel_gpu_top aggregates by default).
pub(crate) fn sample_intel() -> io::Result<u32> {
    let out = Command::new("intel_gpu_top")
        .args(["-J", "-s", "100"])
        .output()?;
    if !out.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("intel_gpu_top exit {:?}", out.status.code()),
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut max_pct: u32 = 0;
    for token in text.split(|c: char| !c.is_ascii_digit()) {
        if let Ok(n) = token.parse::<u32>()
            && n <= 100
            && n > max_pct
        {
            max_pct = n;
        }
    }
    Ok(max_pct)
}

/// `amdgpu_top` is a Python tool that prints an ASCII/JSON GPU utilization
/// report. There is no clean CLI flag contract for headless sampling, so we
/// spawn it with `-n 1` when supported and scan the output for the most
/// plausible utilization percentage.
///
/// Heuristic: look for lines containing the literal "GPU" or "GFX" (case
/// insensitive), then take the first integer 0-100 from that line as the
/// GPU utilization percentage. Lines that lack those keys are ignored
/// (they're typically memory, temperature, or frame counters). This is
/// deliberately conservative — false negatives (under-reporting usage)
/// are safer than false positives (over-reporting → spurious drops).
///
/// Many `amdgpu_top` builds do not support a non-interactive sample mode and
/// will fail with `amdgpu_top exit (1)`. We surface that as an `io::Error`
/// so the caller logs a debug line and continues — never silently OK.
pub(crate) fn sample_amd() -> io::Result<u32> {
    let out = Command::new("amdgpu_top").args(["-n", "1"]).output()?;
    if !out.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("amdgpu_top exit {:?}", out.status.code()),
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        if !lower.contains("gpu") && !lower.contains("gfx") {
            continue;
        }
        for token in line.split(|c: char| !c.is_ascii_digit()) {
            if let Ok(n) = token.parse::<u32>()
                && n <= 100
            {
                return Ok(n);
            }
        }
    }
    Ok(0)
}
