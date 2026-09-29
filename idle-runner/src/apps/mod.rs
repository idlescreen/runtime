//! Application identity helpers used by screensaver plugins at load time.

pub fn username() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "user".to_string())
}

pub fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "localhost".to_string())
}

pub fn user_host() -> String {
    format!("{}@{}", username(), hostname())
}

pub fn os_str() -> String {
    crate::toolkit::sys_info::get_system_info().os
}

pub fn shell_name() -> String {
    if cfg!(target_os = "windows") {
        if std::env::var("PSModulePath").is_ok() {
            "PowerShell v7.4".to_string()
        } else {
            "cmd.exe".to_string()
        }
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
    }
}

pub fn refresh_rate_hz() -> i32 {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Graphics::Gdi::{GetDC, GetDeviceCaps, ReleaseDC};
        unsafe {
            let hdc = GetDC(std::ptr::null_mut());
            if !hdc.is_null() {
                let rate = GetDeviceCaps(hdc, 116);
                ReleaseDC(std::ptr::null_mut(), hdc);
                if rate <= 0 { 144 } else { rate }
            } else {
                144
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        60
    }
}
