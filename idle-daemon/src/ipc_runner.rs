// SPDX-License-Identifier: MIT

use idle_ipc::{
    FfiTerminalCell, IpcCommand, IpcResponse, SharedMemory, compute_shm_size,
    is_plausible_socket_path, is_valid_shm_name, validate_grid_dims,
};
use idle_runner::launcher::{LaunchMode, is_allowed_saver, sanitize_saver_name};
use idle_runner::plugin_session::PluginSession;
use std::os::unix::net::UnixStream;
use std::time::Duration;

pub fn run_ipc_runner(
    saver_name: &str,
    socket_path: &str,
    shm_name: &str,
    cols: usize,
    rows: usize,
    render_scale: Option<f32>,
) -> Result<(), String> {
    // Adversarial argv hardening: reject path-like savers, odd SHM names, and
    // socket paths outside absolute `.sock` form before any open/mmap.
    if !is_allowed_saver(saver_name) {
        return Err(format!("screensaver '{saver_name}' is not allowlisted"));
    }
    let saver_name = sanitize_saver_name(saver_name)
        .ok_or_else(|| format!("invalid screensaver name: {saver_name}"))?;
    if !is_plausible_socket_path(socket_path) {
        return Err(format!("implausible socket path: {socket_path}"));
    }
    if !is_valid_shm_name(shm_name) {
        return Err(format!("invalid shm name: {shm_name}"));
    }
    validate_grid_dims(cols, rows).map_err(|e| e.to_string())?;
    if let Some(scale) = render_scale
        && (!scale.is_finite() || !(0.0..=1.0).contains(&scale))
    {
        return Err(format!("render_scale out of range: {scale}"));
    }

    idle_log::info!(
        "IPC Runner starting for saver '{}', cols: {}, rows: {}, scale: {:?}",
        saver_name,
        cols,
        rows,
        render_scale
    );

    let mut socket = UnixStream::connect(socket_path)
        .map_err(|e| format!("failed to connect to socket {}: {}", socket_path, e))?;

    let shm_size = compute_shm_size(cols, rows).ok_or("shm size overflow")?;
    let shm = SharedMemory::open(shm_name, shm_size)
        .map_err(|e| format!("failed to open shm {}: {}", shm_name, e))?;

    let mut session =
        PluginSession::load_with_options(&saver_name, &LaunchMode::Daemon, render_scale)
            .map_err(|e| format!("failed to load plugin {}: {}", saver_name, e))?;

    if let Err(e) = session.start_watcher() {
        idle_log::warn!("Failed to start screensaver file watcher: {:?}", e);
    }

    IpcResponse::Ready
        .write_to(&mut socket)
        .map_err(|e| format!("failed to send Ready response: {}", e))?;

    loop {
        if let Ok(true) = session.poll_reload() {
            idle_log::info!("Screensaver reloaded successfully.");
        }

        let command = match IpcCommand::read_from(&mut socket) {
            Ok(cmd) => cmd,
            Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                idle_log::info!("IPC control socket closed, runner exiting.");
                break;
            }
            Err(e) => {
                return Err(format!("failed to read IPC command: {}", e));
            }
        };

        match command {
            IpcCommand::Init { cols: c, rows: r } => {
                let c = c as usize;
                let r = r as usize;
                if let Err(e) = validate_grid_dims(c, r) {
                    idle_log::warn!("rejecting Init dims: {e}");
                    return Err(e.to_string());
                }
                session.init(c, r);
                IpcResponse::Ack
                    .write_to(&mut socket)
                    .map_err(|e| format!("failed to send Ack: {}", e))?;
            }
            IpcCommand::TickAndDraw { dt_micros } => {
                // Cap pathological dt (e.g. u64::MAX micros) to 1s of sim time.
                let dt_micros = dt_micros.min(1_000_000);
                session.tick(Duration::from_micros(dt_micros));
                let scanlines = session.draw_frame(cols, rows);

                // SAFETY: SHM opened for this process; header dims set by daemon
                // Init and validated against map size inside `cells_mut`.
                let cells = unsafe { shm.cells_mut() }
                    .map_err(|e| format!("shm cells view rejected: {e}"))?;
                let mut dirty = false;
                for (i, cell) in session.grid().iter().enumerate() {
                    if i < cells.len() {
                        let new_val = FfiTerminalCell::from(*cell);
                        if cells[i] != new_val {
                            cells[i] = new_val;
                            dirty = true;
                        }
                    }
                }

                // SAFETY: same mapping; single writer for frame_counter (this runner).
                unsafe {
                    let header = shm.header_mut();
                    header.frame_counter = header.frame_counter.wrapping_add(1);
                }

                IpcResponse::FrameReady { scanlines, dirty }
                    .write_to(&mut socket)
                    .map_err(|e| format!("failed to send FrameReady: {}", e))?;
            }
            IpcCommand::SetSimulationRate { hz } => {
                let hz = if hz.is_finite() {
                    hz.clamp(1.0, 240.0)
                } else {
                    60.0
                };
                session.set_simulation_rate(hz);
                IpcResponse::Ack
                    .write_to(&mut socket)
                    .map_err(|e| format!("failed to send Ack: {}", e))?;
            }
            IpcCommand::Stop => {
                idle_log::info!("received Stop command, exiting.");
                break;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_like_saver() {
        let err = run_ipc_runner(
            "../beams",
            "/tmp/idle-uds-1-0.sock",
            "/idle-shm-1-0",
            80,
            24,
            None,
        )
        .unwrap_err();
        assert!(err.contains("allowlist") || err.contains("invalid"));
    }

    #[test]
    fn rejects_bad_shm_name() {
        let err = run_ipc_runner("beams", "/tmp/idle-uds-1-0.sock", "/evil-shm", 80, 24, None)
            .unwrap_err();
        assert!(err.contains("shm"));
    }

    #[test]
    fn rejects_relative_socket() {
        let err =
            run_ipc_runner("beams", "relative.sock", "/idle-shm-1-0", 80, 24, None).unwrap_err();
        assert!(err.contains("socket"));
    }

    #[test]
    fn rejects_zero_dims() {
        let err = run_ipc_runner(
            "beams",
            "/tmp/idle-uds-1-0.sock",
            "/idle-shm-1-0",
            0,
            24,
            None,
        )
        .unwrap_err();
        assert!(err.contains("dimension") || err.contains("non-zero"));
    }
}
