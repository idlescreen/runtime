//! Host system information. Vendored and slimmed from `runner::toolkit::sys_info`.
//!
//! Public API: `get_system_info`, `query_dark_mode`,
//! `query_disk_drives` (delegated to `linux_queries`), `query_current_palette`.

mod monitors;
mod theme;

use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub use crate::toolkit::platform::{
    DiskDriveInfo, NetworkAdapterInfo, PowerStatus, SystemBiosInfo, SystemInfo,
};

#[path = "../linux_proc.rs"]
#[cfg(target_os = "linux")]
mod linux_proc;
#[path = "../linux_queries.rs"]
mod linux_queries;

pub use idle_api::MonitorCellBounds;
pub use linux_queries::{
    query_all_monitors as linux_query_all_monitors, query_disk_drives, query_gpu_names,
};
pub use monitors::{
    get_monitor_layouts, get_primary_monitor_bounds, is_secondary_monitor,
    query_monitors_from_xrandr,
};
pub use theme::{SystemTheme, query_current_palette, query_dark_mode, query_system_theme};

use std::sync::RwLock;

static SYSTEM_INFO_CACHE: OnceLock<RwLock<(Option<SystemInfo>, Instant)>> = OnceLock::new();
static SYSTEM_OBJECT: OnceLock<Mutex<linux_proc::ProcStats>> = OnceLock::new();
/// Host identity strings that do not change while the process is running.
static STATIC_HOST: OnceLock<StaticHostInfo> = OnceLock::new();

struct StaticHostInfo {
    os: String,
    logo_text: String,
    kernel: String,
    hostname: String,
    cpu: String,
    gpus: String,
    monitors: String,
}

fn get_system() -> std::sync::MutexGuard<'static, linux_proc::ProcStats> {
    SYSTEM_OBJECT
        .get_or_init(|| Mutex::new(linux_proc::ProcStats::new()))
        .lock()
        .unwrap_or_else(|e| {
            idle_log::error!("mutex poisoned: {e}");
            std::process::abort()
        })
}

fn static_host() -> &'static StaticHostInfo {
    STATIC_HOST.get_or_init(|| {
        let os = linux_proc::long_os_version().unwrap_or_else(|| "Linux".to_string());
        let kernel = linux_proc::kernel_version().unwrap_or_else(|| "unknown".to_string());
        let kernel_short = kernel.split('-').next().unwrap_or(&kernel);
        let logo_text = format!("Linux {}", kernel_short);
        let hostname = linux_proc::host_name().unwrap_or_else(|| "localhost".to_string());
        let cpu = linux_proc::cpu_brand().unwrap_or_else(|| "CPU".to_string());
        let gpus = {
            let joined = query_gpu_names().join(", ");
            if joined.is_empty() {
                "GPU(s)".to_string()
            } else {
                joined
            }
        };
        let monitors = format!("{} monitor(s)", linux_query_all_monitors().len());
        StaticHostInfo {
            os,
            logo_text,
            kernel,
            hostname,
            cpu,
            gpus,
            monitors,
        }
    })
}

/// Returns rich live system info. Cross-platform. Cached for 3 seconds.
pub fn get_system_info() -> SystemInfo {
    let cache_rw = SYSTEM_INFO_CACHE.get_or_init(|| RwLock::new((None, Instant::now())));
    if let Ok(read_guard) = cache_rw.read()
        && let Some(ref val) = read_guard.0
        && read_guard.1.elapsed() < Duration::from_secs(3)
    {
        return val.clone();
    }
    let mut cache = cache_rw.write().unwrap_or_else(|e| {
        idle_log::error!("mutex poisoned: {e}");
        std::process::abort()
    });
    if let Some(ref val) = cache.0
        && cache.1.elapsed() < Duration::from_secs(3)
    {
        return val.clone();
    }
    let val = get_system_info_raw();
    // Store then clone once for the caller (avoids clone-then-store double clone).
    cache.0 = Some(val.clone());
    cache.1 = Instant::now();
    val
}

fn get_system_info_raw() -> SystemInfo {
    let host = static_host();
    let mut sys = get_system();
    // Selective refresh: processes/components are unused by SystemInfo.
    sys.refresh_memory();
    sys.refresh_cpu_usage();

    let total = sys.total_memory_bytes();
    let available = sys.available_memory_bytes();
    let used = total.saturating_sub(available);
    let mem_total_mb = total / (1024 * 1024);
    let mem_used_mb = used / (1024 * 1024);
    let mem_used_pct = if total > 0 {
        (used as f32 / total as f32) * 100.0
    } else {
        0.0
    };

    let cpu_usage_pct = sys.global_cpu_usage();
    let uptime_secs = linux_proc::uptime_secs();

    let power = query_power_status().unwrap_or_default();
    let power_status = if power.ac_online {
        "AC".to_string()
    } else {
        format!("{}% (Battery)", power.battery_percent)
    };
    let disks = query_disk_drives();
    let disk_summary = if let Some(d) = disks.first() {
        format!("{} ~{}G free", d.path, d.free_bytes / (1024 * 1024 * 1024))
    } else {
        "disks".to_string()
    };

    SystemInfo {
        os: host.os.clone(),
        logo_text: host.logo_text.clone(),
        kernel: host.kernel.clone(),
        hostname: host.hostname.clone(),
        cpu: host.cpu.clone(),
        uptime_secs,
        mem_used_mb,
        mem_total_mb,
        mem_used_pct,
        cpu_usage_pct,
        power_status,
        disk_summary,
        gpus: host.gpus.clone(),
        monitors: host.monitors.clone(),
    }
}

/// Power status: AC online + battery percent.
pub fn query_power_status() -> Option<PowerStatus> {
    #[cfg(target_os = "linux")]
    {
        linux_proc::query_power_status_linux()
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_system_info_is_stable_and_cached() {
        let a = get_system_info();
        let b = get_system_info();
        assert_eq!(a.hostname, b.hostname);
        assert_eq!(a.os, b.os);
        assert!(!a.cpu.is_empty());
    }
}
