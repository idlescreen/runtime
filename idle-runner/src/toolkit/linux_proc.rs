//! Linux-specific power supply and theme helper queries.

use crate::toolkit::platform::PowerStatus;

pub fn query_dark_mode_linux() -> bool {
    // 1. Try reading standard GTK-3.0 / GTK-4.0 settings files directly
    for path in &[
        ".config/gtk-4.0/settings.ini",
        ".config/gtk-3.0/settings.ini",
    ] {
        if let Some(home) = std::env::var_os("HOME") {
            let full_path = std::path::Path::new(&home).join(path);
            if let Ok(content) = std::fs::read_to_string(full_path) {
                let lower = content.to_lowercase();
                for line in lower.lines() {
                    if line.contains("gtk-application-prefer-dark-theme") && line.contains("true") {
                        return true;
                    }
                    if line.contains("gtk-theme-name") && line.contains("dark") {
                        return true;
                    }
                }
            }
        }
    }

    // 2. Fallback to gsettings if ini files are missing or inconclusive
    if let Ok(output) = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
    {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if s.contains("prefer-dark") {
            return true;
        }
    }
    if let Ok(output) = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "gtk-theme"])
        .output()
    {
        let s = String::from_utf8_lossy(&output.stdout).to_lowercase();
        if s.contains("dark") {
            return true;
        }
    }
    true
}

pub fn query_power_status_linux() -> Option<PowerStatus> {
    let mut ac_online = true;
    let mut has_ac = false;
    let mut battery_percent: Option<u8> = None;
    if let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") {
        for entry in entries.take(64).flatten() {
            let path = entry.path();
            if let Ok(ty_str) = std::fs::read_to_string(path.join("type")) {
                match ty_str.trim() {
                    "Mains" => {
                        if let Ok(online_str) = std::fs::read_to_string(path.join("online")) {
                            let online = online_str.trim() == "1";
                            if !has_ac {
                                ac_online = online;
                                has_ac = true;
                            } else {
                                ac_online = ac_online || online;
                            }
                        }
                    }
                    "Battery" => {
                        if let Ok(cap_str) = std::fs::read_to_string(path.join("capacity"))
                            && let Ok(pct) = cap_str.trim().parse::<u8>()
                        {
                            battery_percent = Some(pct);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    if !has_ac && battery_percent.is_some() {
        idle_log::warn!(
            "Detected battery but no AC adapter in /sys/class/power_supply; assuming AC is offline (running on battery)"
        );
    }
    battery_percent.map(|pct| PowerStatus {
        ac_online: if has_ac { ac_online } else { false },
        battery_percent: pct,
    })
}

// ---------------------------------------------------------------------------
// sysinfo::System replacements — /proc + uname + gethostname
// ---------------------------------------------------------------------------

/// Memory + CPU stats refreshed from /proc on demand.
#[path = "linux_proc/proc_stats.rs"]
mod proc_stats;
pub use proc_stats::*;
pub fn cpu_brand() -> Option<String> {
    let text = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    parse_cpu_brand(&text)
}

fn parse_cpu_brand(text: &str) -> Option<String> {
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("model name") {
            return rest.split(':').nth(1).map(|s| s.trim().to_string());
        }
    }
    // ARM/SoC fallback: "Hardware" or "Model" fields.
    for line in text.lines() {
        for key in ["Hardware", "Model"] {
            if let Some(rest) = line.strip_prefix(key)
                && let Some(v) = rest.split(':').nth(1)
            {
                let v = v.trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

/// sysinfo `System::long_os_version()`: PRETTY_NAME from /etc/os-release.
pub fn long_os_version() -> Option<String> {
    for path in ["/etc/os-release", "/usr/lib/os-release"] {
        if let Ok(text) = std::fs::read_to_string(path)
            && let Some(v) = parse_pretty_name(&text)
        {
            return Some(v);
        }
    }
    None
}

fn parse_pretty_name(text: &str) -> Option<String> {
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("PRETTY_NAME=") {
            return Some(v.trim().trim_matches('"').to_string());
        }
    }
    None
}

/// sysinfo `System::kernel_version()`: uname -r.
pub fn kernel_version() -> Option<String> {
    let mut uts: libc::utsname = unsafe { std::mem::zeroed() };
    if unsafe { libc::uname(&mut uts) } != 0 {
        return None;
    }
    let release = unsafe { std::ffi::CStr::from_ptr(uts.release.as_ptr()) };
    Some(release.to_string_lossy().to_string())
}

/// sysinfo `System::host_name()`: gethostname(2).
pub fn host_name() -> Option<String> {
    let mut buf = [0u8; 256];
    if unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } != 0 {
        return None;
    }
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    Some(String::from_utf8_lossy(&buf[..len]).to_string())
}

/// sysinfo `System::uptime()`: first field of /proc/uptime.
pub fn uptime_secs() -> u64 {
    std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|t| parse_uptime_secs(&t))
        .unwrap_or(0)
}

fn parse_uptime_secs(text: &str) -> Option<u64> {
    text.split_whitespace()
        .next()
        .and_then(|v| v.parse::<f64>().ok())
        .map(|v| v as u64)
}

#[cfg(test)]
#[cfg(test)]
#[path = "linux_proc/tests.rs"]
mod tests;
