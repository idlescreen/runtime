// SPDX-License-Identifier: MIT

pub fn spawn_failsafe_locker() -> Result<(), String> {
    idle_log::warn!("Plugin crashed! Executing fail-closed loginctl session lock...");

    // Asynchronously call loginctl to lock the session, so we don't block the daemon tick loop
    std::thread::spawn(|| {
        let status = std::process::Command::new("loginctl")
            .arg("lock-session")
            .status();

        match status {
            Ok(s) if s.success() => {
                idle_log::info!("Successfully issued loginctl lock-session.");
            }
            Ok(s) => {
                idle_log::error!(
                    "loginctl lock-session failed with status {s}. Executing swaylock fallback..."
                );
                let _ = std::process::Command::new("swaylock").arg("-f").status();
            }
            Err(e) => {
                idle_log::error!("Failed to execute loginctl: {e}. Executing swaylock fallback...");
                let _ = std::process::Command::new("swaylock").arg("-f").status();
            }
        }
    });

    Ok(())
}
