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
