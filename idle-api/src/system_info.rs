/// Cross-platform "where are we running" descriptor.
#[derive(Debug, Clone)]
pub struct SystemInfo {
    pub os: String,
    pub logo_text: String,
    pub kernel: String,
    pub hostname: String,
    pub cpu: String,
    pub uptime_secs: u64,
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    pub mem_used_pct: f32,
    pub cpu_usage_pct: f32,
    pub power_status: String,
    pub disk_summary: String,
    pub gpus: String,
    pub monitors: String,
}

/// `PRETTY_NAME` from `/etc/os-release`, or `None` when there is no such file
/// — a browser, a container, or any non-Linux host.
fn os_pretty_name() -> Option<String> {
    let content = std::fs::read_to_string("/etc/os-release").ok()?;
    content.lines().find_map(|line| {
        let val = line.strip_prefix("PRETTY_NAME=")?;
        let val = val.trim_matches('"').trim();
        (!val.is_empty()).then(|| val.to_string())
    })
}

impl Default for SystemInfo {
    fn default() -> Self {
        if Self::export_mode_enabled() {
            return Self::export_fixture();
        }
        let os = crate::env_var_first(&["IDLE_OS_NAME"]).unwrap_or_else(|| {
            let mut temp_os = "Linux".to_string();
            if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
                for line in content.lines() {
                    if line.starts_with("PRETTY_NAME=") {
                        let val = line.split('=').nth(1).unwrap_or("").trim_matches('"');
                        if !val.is_empty() {
                            temp_os = val.to_string();
                            break;
                        }
                    }
                }
            }
            temp_os
        });

        // The shared wordmark every saver renders.
        //
        // One source of truth: the host's OS name, overridable with
        // IDLE_LOGO_TEXT. Savers used to derive this independently and drift —
        // eleven read it from here while ascii hardcoded its own string, so
        // one machine showed different words on different savers.
        //
        // The fallback matters for the browser demos, where there is no
        // /etc/os-release and every saver would otherwise render the bare
        // string "Linux". Falling back to the product name keeps every demo
        // identical to every other.
        let logo_text = crate::env_var_first(&["IDLE_LOGO_TEXT"])
            .unwrap_or_else(|| os_pretty_name().unwrap_or_else(|| "IDLESCREEN".into()));

        let hostname = std::env::var("HOSTNAME").unwrap_or_else(|_| "localhost".to_string());

        let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        Self {
            os,
            logo_text,
            kernel,
            hostname,
            cpu: "CPU".to_string(),
            uptime_secs: 0,
            mem_used_mb: 1,
            mem_total_mb: 2,
            mem_used_pct: 50.0,
            cpu_usage_pct: 0.0,
            power_status: "AC".to_string(),
            disk_summary: "disks".to_string(),
            gpus: "GPU".to_string(),
            monitors: "1 monitor".to_string(),
        }
    }
}

impl SystemInfo {
    /// Stable fixture for offline export (`IDLE_EXPORT_MODE=1` / render).
    pub fn export_fixture() -> Self {
        Self {
            os: "IdleScreen Export".into(),
            logo_text: "IdleScreen".into(),
            kernel: "export".into(),
            hostname: "export-host".into(),
            cpu: "CPU".into(),
            uptime_secs: 0,
            mem_used_mb: 1024,
            mem_total_mb: 2048,
            mem_used_pct: 50.0,
            cpu_usage_pct: 0.0,
            power_status: "AC".into(),
            disk_summary: "disk".into(),
            gpus: "GPU".into(),
            monitors: "1 monitor".into(),
        }
    }

    /// True when host should prefer deterministic export fixtures.
    pub fn export_mode_enabled() -> bool {
        crate::env_truthy(&["IDLE_EXPORT_MODE"])
            || std::env::var_os("RENDER_SEED").is_some()
            || std::env::var_os("IDLE_RENDER_SEED").is_some()
    }
}
