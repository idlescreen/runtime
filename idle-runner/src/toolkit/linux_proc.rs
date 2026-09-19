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
pub struct ProcStats {
    mem_total_kb: u64,
    mem_available_kb: u64,
    prev_idle: u64,
    prev_total: u64,
    cpu_usage_pct: f32,
}

impl ProcStats {
    pub fn new() -> Self {
        let mut s = Self {
            mem_total_kb: 0,
            mem_available_kb: 0,
            prev_idle: 0,
            prev_total: 0,
            cpu_usage_pct: 0.0,
        };
        s.refresh_memory();
        s.refresh_cpu_usage();
        s
    }

    /// sysinfo `refresh_memory`: MemTotal/MemAvailable from /proc/meminfo.
    pub fn refresh_memory(&mut self) {
        if let Ok(text) = std::fs::read_to_string("/proc/meminfo") {
            self.mem_total_kb = meminfo_kb(&text, "MemTotal");
            self.mem_available_kb = meminfo_kb(&text, "MemAvailable");
        }
    }

    /// sysinfo `refresh_cpu_usage`: usage % from /proc/stat jiffies deltas.
    pub fn refresh_cpu_usage(&mut self) {
        let Ok(text) = std::fs::read_to_string("/proc/stat") else {
            return;
        };
        let Some(line) = text.lines().next() else {
            return;
        };
        // "cpu  user nice system idle iowait irq softirq steal ..."
        let fields: Vec<u64> = line
            .split_whitespace()
            .skip(1)
            .filter_map(|f| f.parse().ok())
            .collect();
        if fields.len() < 4 {
            return;
        }
        let idle = fields[3] + fields.get(4).copied().unwrap_or(0);
        let total: u64 = fields.iter().sum();
        let d_total = total.saturating_sub(self.prev_total);
        let d_idle = idle.saturating_sub(self.prev_idle);
        if d_total > 0 {
            self.cpu_usage_pct = ((d_total - d_idle) as f32 / d_total as f32) * 100.0;
        }
        self.prev_idle = idle;
        self.prev_total = total;
    }

    pub fn total_memory_bytes(&self) -> u64 {
        self.mem_total_kb * 1024
    }
    pub fn available_memory_bytes(&self) -> u64 {
        self.mem_available_kb * 1024
    }
    pub fn global_cpu_usage(&self) -> f32 {
        self.cpu_usage_pct
    }
}

fn kb_value(field: &str) -> u64 {
    field
        .split_whitespace()
        .next()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// Look up `key:` in meminfo text → its kB value (0 if absent/unparseable).
fn meminfo_kb(text: &str, key: &str) -> u64 {
    for line in text.lines() {
        if let Some(v) = line.strip_prefix(key)
            && let Some(v) = v.strip_prefix(':')
        {
            return kb_value(v);
        }
    }
    0
}

/// sysinfo `cpus()[0].brand()`: first "model name" in /proc/cpuinfo.
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
mod tests {
    use super::*;

    // --- synthetic /proc text ---

    #[test]
    fn meminfo_kb_reads_named_key() {
        let text =
            "MemTotal:       16384000 kB\nMemFree:         100000 kB\nMemAvailable:   8000000 kB\n";
        assert_eq!(meminfo_kb(text, "MemTotal"), 16_384_000);
        assert_eq!(meminfo_kb(text, "MemAvailable"), 8_000_000);
    }

    #[test]
    fn meminfo_kb_rejects_prefix_collision() {
        // "MemTotalFoo" must not satisfy a "MemTotal" lookup.
        let text = "MemTotalExtra: 999 kB\n";
        assert_eq!(meminfo_kb(text, "MemTotal"), 0);
        assert_eq!(meminfo_kb("", "MemTotal"), 0);
        assert_eq!(meminfo_kb("MemTotal: abc kB\n", "MemTotal"), 0);
    }

    #[test]
    fn parse_cpu_brand_x86() {
        let text = "processor\t: 0\nmodel name\t: AMD Ryzen 9 5900X\ncores\t: 12\n";
        assert_eq!(parse_cpu_brand(text).as_deref(), Some("AMD Ryzen 9 5900X"));
    }

    #[test]
    fn parse_cpu_brand_arm_fallback() {
        let text = "processor\t: 0\nHardware\t: BCM2835\nModel\t\t: Raspberry Pi 4\n";
        assert_eq!(parse_cpu_brand(text).as_deref(), Some("BCM2835"));
    }

    #[test]
    fn parse_cpu_brand_empty_value_falls_through() {
        // Empty "Hardware:" value must be skipped, not returned.
        let text = "Hardware\t:\nModel\t\t: Real Board\n";
        assert_eq!(parse_cpu_brand(text).as_deref(), Some("Real Board"));
        assert_eq!(parse_cpu_brand("nothing here\n"), None);
    }

    #[test]
    fn parse_pretty_name_variants() {
        assert_eq!(
            parse_pretty_name("NAME=\"Debian\"\nPRETTY_NAME=\"Debian GNU/Linux 12\"\n").as_deref(),
            Some("Debian GNU/Linux 12")
        );
        assert_eq!(
            parse_pretty_name("PRETTY_NAME=No Quotes OS\n").as_deref(),
            Some("No Quotes OS")
        );
        assert_eq!(parse_pretty_name("NAME=x\n"), None);
    }

    #[test]
    fn parse_uptime_secs_truncates_fraction() {
        assert_eq!(parse_uptime_secs("123456.78 999.99\n"), Some(123456));
        assert_eq!(parse_uptime_secs("garbage\n"), None);
        assert_eq!(parse_uptime_secs(""), None);
    }

    // --- live /proc (Linux CI/dev boxes only) ---

    #[test]
    fn proc_stats_reads_real_meminfo() {
        let s = ProcStats::new();
        assert!(s.total_memory_bytes() > 0);
        assert!(s.available_memory_bytes() <= s.total_memory_bytes());
        assert!(s.global_cpu_usage() >= 0.0 && s.global_cpu_usage() <= 100.0);
    }

    #[test]
    fn proc_stats_cpu_usage_stays_in_range() {
        let mut s = ProcStats::new();
        s.refresh_cpu_usage();
        let u = s.global_cpu_usage();
        assert!((0.0..=100.0).contains(&u));
    }

    #[test]
    fn uname_and_hostname_nonempty() {
        let kv = kernel_version().expect("uname works on Linux");
        assert!(!kv.is_empty());
        assert!(kv.contains('.'), "kernel release has a version: {kv}");
        let hn = host_name().expect("gethostname works on Linux");
        assert!(!hn.is_empty());
    }

    #[test]
    fn os_version_matches_os_release() {
        // If /etc/os-release is readable it must yield a non-empty name.
        if std::path::Path::new("/etc/os-release").exists() {
            let v = long_os_version().expect("os-release parsed");
            assert!(!v.is_empty());
        }
    }

    #[test]
    fn uptime_positive() {
        assert!(uptime_secs() > 0);
    }

    #[test]
    fn power_status_plausible_when_present() {
        if let Some(ps) = query_power_status_linux() {
            assert!(ps.battery_percent <= 100);
        }
    }
}
