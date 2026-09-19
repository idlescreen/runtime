//! Linux-specific platform queries (disk drives, GPU names, monitor enumeration).
//! Bypasses lspci and df subprocess commands using native FFI (statvfs) and sysfs.

use crate::toolkit::platform::DiskDriveInfo;

#[cfg(target_os = "linux")]
pub fn query_disk_drives() -> Vec<DiskDriveInfo> {
    // sysinfo::Disks equivalent: /proc/mounts entries backed by real block
    // devices, statvfs'd for space.
    let mut drives = Vec::new();
    if let Ok(mounts) = std::fs::read_to_string("/proc/mounts") {
        for mount in mounted_block_devices(&mounts) {
            let Some((total_bytes, free_bytes)) = statvfs_space(&mount) else {
                continue;
            };
            drives.push(DiskDriveInfo {
                path: unescape_mount(&mount),
                total_bytes,
                free_bytes,
            });
        }
    }
    if drives.is_empty() {
        idle_log::warn!(
            "no block-device mounts found in /proc/mounts; returning empty list instead of fake data"
        );
    }
    drives
}

/// /proc/mounts → mountpoints of real block devices only; first mount
/// wins for bind-mounts sharing a device.
#[cfg(target_os = "linux")]
fn mounted_block_devices(text: &str) -> Vec<String> {
    let mut seen_devices = std::collections::HashSet::new();
    let mut out = Vec::new();
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let (Some(dev), Some(mount)) = (fields.next(), fields.next()) else {
            continue;
        };
        if !dev.starts_with("/dev/") || !seen_devices.insert(dev.to_string()) {
            continue;
        }
        out.push(mount.to_string());
    }
    out
}

/// statvfs a mount point → (total_bytes, available_bytes).
#[cfg(target_os = "linux")]
fn statvfs_space(mount: &str) -> Option<(u64, u64)> {
    let c_path = std::ffi::CString::new(unescape_mount(mount)).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    Some((stat.f_blocks * stat.f_frsize, stat.f_bavail * stat.f_frsize))
}

/// /proc/mounts escapes spaces and control chars as \040-style octal.
#[cfg(target_os = "linux")]
fn unescape_mount(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let oct: String = chars.by_ref().take(3).collect();
            if oct.len() == 3
                && let Ok(v) = u8::from_str_radix(&oct, 8)
            {
                out.push(v as char);
                continue;
            }
            out.push_str(&oct);
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(target_os = "linux")]
pub fn query_gpu_names() -> Vec<String> {
    let mut gpus = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            let path = entry.path().join("device").join("uevent");
            if path.exists()
                && let Ok(content) = std::fs::read_to_string(path)
            {
                for line in content.lines() {
                    if line.starts_with("DRIVER=") {
                        let driver = line.split('=').nth(1).unwrap_or("").to_string();
                        if !driver.is_empty() && !gpus.contains(&driver) {
                            gpus.push(driver);
                        }
                    }
                }
            }
        }
    }
    gpus
}

#[cfg(not(target_os = "linux"))]
pub fn query_gpu_names() -> Vec<String> {
    Vec::new()
}

#[cfg(target_os = "linux")]
pub fn query_all_monitors() -> Vec<String> {
    let mut monitors = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            let modes_path = entry.path().join("modes");
            if modes_path.exists()
                && let Ok(content) = std::fs::read_to_string(&modes_path)
                && let Some(line) = content.lines().next()
            {
                monitors.push(format!("Display: {}", line));
            }
        }
    }
    if monitors.is_empty() {
        idle_log::warn!(
            "Could not detect any monitors in /sys/class/drm; returning empty list instead of fake data"
        );
    }
    monitors
}

#[cfg(not(target_os = "linux"))]
pub fn query_all_monitors() -> Vec<String> {
    vec!["Primary: 1920x1080".to_string()]
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn unescape_mount_decodes_octal() {
        assert_eq!(unescape_mount("/mnt/my\\040dir"), "/mnt/my dir");
        assert_eq!(unescape_mount("/a\\011b"), "/a\tb");
        assert_eq!(unescape_mount("/x\\134y"), "/x\\y");
    }

    #[test]
    fn unescape_mount_passes_through_garbage() {
        assert_eq!(unescape_mount("/plain"), "/plain");
        // Invalid escapes keep their chars; only the backslash is eaten.
        assert_eq!(unescape_mount("/bad\\0"), "/bad0");
        assert_eq!(unescape_mount("/bad\\xyz"), "/badxyz");
    }

    #[test]
    fn mounted_block_devices_filters_and_dedupes() {
        let mounts = "\
proc /proc proc rw,nosuid 0 0
/dev/sda1 / ext4 rw 0 0
tmpfs /run tmpfs rw 0 0
/dev/sda1 /boot/bind ext4 rw 0 0
/dev/sdb2 /home ext4 rw 0 0
malformed-line-without-fields
/dev/mapper/vg-lv /var ext4 rw 0 0
";
        assert_eq!(
            mounted_block_devices(mounts),
            vec!["/".to_string(), "/home".to_string(), "/var".to_string()],
            "bind-mount dedupe + non-/dev filtered"
        );
    }

    #[test]
    fn mounted_block_devices_empty_on_no_block_devs() {
        let mounts = "proc /proc proc rw 0 0\ntmpfs /run tmpfs rw 0 0\n";
        assert!(mounted_block_devices(mounts).is_empty());
    }

    #[test]
    fn statvfs_space_real_root() {
        let (total, free) = statvfs_space("/").expect("root is statvfs-able");
        assert!(total > 0);
        assert!(free <= total);
    }

    #[test]
    fn statvfs_space_bad_path_is_none() {
        assert!(statvfs_space("/definitely/not/a/mount").is_none());
    }

    #[test]
    fn disk_drives_consistent_when_present() {
        // Container/CI mounts may expose no /dev entries — only assert the
        // invariants of whatever is returned.
        for d in query_disk_drives() {
            assert!(d.total_bytes > 0, "{} has zero size", d.path);
            assert!(d.free_bytes <= d.total_bytes, "{} free > total", d.path);
            assert!(d.path.starts_with('/'), "{} not absolute", d.path);
        }
    }

    #[test]
    fn gpu_names_dedupes_drivers() {
        // Whatever drivers exist, the list must contain no duplicates.
        let gpus = query_gpu_names();
        let mut uniq = gpus.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!(gpus, uniq, "driver list must be deduped");
    }
}
