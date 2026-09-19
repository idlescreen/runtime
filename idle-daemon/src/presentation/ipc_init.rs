// SPDX-License-Identifier: MIT

use idle_ipc::{
    IpcCommand, IpcResponse, SHM_MAGIC, SharedMemory, compute_shm_size, validate_grid_dims,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::ipc_peer::{require_child_peer, runtime_socket_dir};
use super::timeout::read_timeout;

static SESSION_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub struct SessionInitResult {
    pub child: Child,
    pub socket: UnixStream,
    pub shm: SharedMemory,
    pub socket_path: PathBuf,
}

/// `Child` Drop neither kills nor waits — explicit reap is required on init failure.
pub(crate) fn kill_and_reap(child: &mut Child, socket_path: &Path) {
    let _ = child.kill();
    let _ = child.wait();
    let _ = fs::remove_file(socket_path);
}

pub fn initialize_ipc_session(
    saver_name: &str,
    cols: usize,
    rows: usize,
    render_scale: f32,
    saver_params: &std::collections::BTreeMap<String, String>,
) -> Result<SessionInitResult, String> {
    validate_grid_dims(cols, rows).map_err(|e| e.to_string())?;
    if !render_scale.is_finite() || !(0.0..=1.0).contains(&render_scale) {
        return Err(format!("render_scale out of range: {render_scale}"));
    }

    let session_idx = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
    let rand_val = std::process::id();
    let socket_path =
        runtime_socket_dir()?.join(format!("idle-uds-{}-{}.sock", rand_val, session_idx));
    if socket_path.exists() {
        let _ = fs::remove_file(&socket_path);
    }
    let listener = UnixListener::bind(&socket_path)
        .map_err(|e| format!("failed to bind UDS listener: {}", e))?;
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("chmod 0600 on UDS: {e}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("failed to set UDS listener nonblocking: {}", e))?;

    let shm_name = format!("/idle-shm-{}-{}", rand_val, session_idx);
    let shm_size = compute_shm_size(cols, rows).ok_or("shm size overflow")?;
    let shm = SharedMemory::create(&shm_name, shm_size)?;

    // SAFETY: header init before spawn; single writer.
    unsafe {
        let header = shm.header_mut();
        header.magic = SHM_MAGIC;
        header.cols = cols as u32;
        header.rows = rows as u32;
        header.frame_counter = 0;
    }

    let current_exe =
        std::env::current_exe().map_err(|e| format!("failed to get current exe path: {}", e))?;
    let scale_str = format!("{:.6}", render_scale);

    // `[saver]` params → `IDLE_SAVER_PARAM_*` env on the runner so plugins
    // read them via `idle_api::param*` without an IPC/ABI change.
    let param_env = saver_param_env(saver_params);

    let mut child = Command::new(current_exe)
        .arg("run-ipc-runner")
        .arg(saver_name)
        .arg(socket_path.to_str().ok_or("invalid socket path")?)
        .arg(&shm_name)
        .arg(cols.to_string())
        .arg(rows.to_string())
        .arg(&scale_str)
        .envs(param_env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .spawn()
        .map_err(|e| format!("failed to spawn runner process: {}", e))?;

    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(5);
    let socket = loop {
        match listener.accept() {
            Ok((stream, _)) => match require_child_peer(&stream, &child) {
                Ok(()) => break stream,
                Err(e) => {
                    idle_log::warn!("rejecting IPC peer: {e}");
                    if start.elapsed() > timeout {
                        kill_and_reap(&mut child, &socket_path);
                        return Err(e);
                    }
                }
            },
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if start.elapsed() > timeout {
                    kill_and_reap(&mut child, &socket_path);
                    return Err("timeout waiting for runner process connection".into());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                kill_and_reap(&mut child, &socket_path);
                return Err(format!("UDS accept error: {}", e));
            }
        }
    };

    for (label, res) in [
        ("blocking", socket.set_nonblocking(false)),
        (
            "read timeout",
            socket.set_read_timeout(Some(read_timeout())),
        ),
        (
            "write timeout",
            socket.set_write_timeout(Some(Duration::from_millis(500))),
        ),
    ] {
        if let Err(e) = res {
            kill_and_reap(&mut child, &socket_path);
            return Err(format!("failed to set {label} on runner stream: {e}"));
        }
    }

    let mut socket = socket;
    match IpcResponse::read_from(&mut socket) {
        Ok(IpcResponse::Ready) => {}
        Ok(resp) => {
            kill_and_reap(&mut child, &socket_path);
            return Err(format!("unexpected connection message: {:?}", resp));
        }
        Err(e) => {
            kill_and_reap(&mut child, &socket_path);
            return Err(format!("failed to read connection message: {}", e));
        }
    }

    if let Err(e) = (IpcCommand::Init {
        cols: cols as u32,
        rows: rows as u32,
    })
    .write_to(&mut socket)
    {
        kill_and_reap(&mut child, &socket_path);
        return Err(format!("failed to send Init: {}", e));
    }

    match IpcResponse::read_from(&mut socket) {
        Ok(IpcResponse::Ack) => {}
        Ok(resp) => {
            kill_and_reap(&mut child, &socket_path);
            return Err(format!("unexpected response to Init: {:?}", resp));
        }
        Err(e) => {
            kill_and_reap(&mut child, &socket_path);
            return Err(format!("failed to read Init Ack: {}", e));
        }
    }

    Ok(SessionInitResult {
        child,
        socket,
        shm,
        socket_path,
    })
}

/// Map `[saver]` params to `IDLE_SAVER_PARAM_*` env pairs for the runner
/// spawn. Keys are sanitized to `[A-Z0-9_]`; unsanitizable keys are dropped.
pub(crate) fn saver_param_env(
    params: &std::collections::BTreeMap<String, String>,
) -> Vec<(String, String)> {
    params
        .iter()
        .filter_map(|(k, v)| idle_api::saver_param_env_key(k).map(|n| (n, v.clone())))
        .collect()
}

#[cfg(test)]
#[path = "ipc_init_tests.rs"]
mod tests;
