// SPDX-License-Identifier: MIT

//! Background idle daemon: Wayland idle detection, overlay presentation, D-Bus API.
//!
//! `run_daemon` is the orchestrator; setup helpers stay here and the runtime
//! tick loop lives in sibling modules.

pub mod battery;
pub(crate) mod idle_decision;
#[cfg(test)]
mod liveness_validation_tests;
#[cfg(test)]
mod m2_concurrency_stress_tests;
pub(crate) mod pidfile;
pub(crate) mod presentation;
pub(crate) mod preview_queue;
pub(crate) mod recovery;
pub(crate) mod runtime;
pub(crate) mod tick_loop;
pub(crate) mod watchdog;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use idle_err::anyhow;

use crate::config::DaemonConfig;
use crate::controller::DaemonController;

pub use tick_loop::tick_loop_until_shutdown;

pub fn run_daemon() -> idle_err::Result<()> {
    check_wayland_env()?;
    let Some(pidfile) = pidfile::acquire_pidfile()? else {
        return Ok(());
    };
    let config = DaemonConfig::load();
    // Honor config.yaml strict_control for D-Bus auth (env is the auth check surface).
    if config.strict_control {
        // SAFETY: single-threaded startup before other threads read the flag.
        unsafe {
            std::env::set_var("IDLE_STRICT_CONTROL", "1");
        }
    }
    let controller = Arc::new(DaemonController::new(config));
    crate::config_watcher::start_config_watcher(controller.clone());
    install_signal_handlers(&controller)?;
    log_daemon_startup();
    runtime::log_posture();
    let dbus_handle = spawn_dbus_thread(Arc::clone(&controller))?;
    let result = tick_loop_until_shutdown(Arc::clone(&controller));
    controller.shutdown.store(true, Ordering::Relaxed);
    let _ = dbus_handle.join();
    pidfile::release_pidfile(&pidfile);
    result
}

fn check_wayland_env() -> idle_err::Result<()> {
    if std::env::var("WAYLAND_DISPLAY").is_err() {
        return Err(anyhow!(
            "WAYLAND_DISPLAY is not set; IdleScreen requires a Wayland session"
        ));
    }
    Ok(())
}

/// Raw pointer to the controller's shutdown flag, kept alive by a leaked
/// `Arc` clone below — the same lifetime contract `signal_hook::flag` used.
static SHUTDOWN_FLAG: AtomicPtr<AtomicBool> = AtomicPtr::new(std::ptr::null_mut());

extern "C" fn on_term_sig(_sig: libc::c_int) {
    // Async-signal-safe: only an atomic load + store.
    let p = SHUTDOWN_FLAG.load(Ordering::Relaxed);
    if !p.is_null() {
        unsafe { (*p).store(true, Ordering::Relaxed) };
    }
}

fn install_signal_handlers(controller: &Arc<DaemonController>) -> idle_err::Result<()> {
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    let raw = Arc::into_raw(Arc::clone(&controller.shutdown)) as *mut AtomicBool;
    SHUTDOWN_FLAG.store(raw, Ordering::Relaxed);
    unsafe {
        if libc::signal(libc::SIGINT, on_term_sig as *const () as libc::sighandler_t)
            == libc::SIG_ERR
        {
            idle_err::bail!("registering SIGINT handler failed");
        }
        if libc::signal(
            libc::SIGTERM,
            on_term_sig as *const () as libc::sighandler_t,
        ) == libc::SIG_ERR
        {
            idle_err::bail!("registering SIGTERM handler failed");
        }
    }
    Ok(())
}

fn log_daemon_startup() {
    idle_log::info!("idle-daemon running (pid {})...", std::process::id());
    if cfg!(debug_assertions) {
        idle_log::warn!(
            "WARNING — debug build is very slow (~1 FPS). \
             Use target/release/idle-daemon for real performance."
        );
    }
}

fn spawn_dbus_thread(
    controller: Arc<DaemonController>,
) -> idle_err::Result<std::thread::JoinHandle<()>> {
    let ctrl = controller.clone();
    let handle = std::thread::spawn(move || {
        let mut retries = 0;
        while !controller.shutdown.load(Ordering::Relaxed) && retries < 3 {
            let start = std::time::Instant::now();
            let ctrl_loop = ctrl.clone();
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::dbus_server::run(ctrl_loop)
            }));
            if start.elapsed() > std::time::Duration::from_secs(5) {
                retries = 0;
            }
            match res {
                Ok(Err(error)) => {
                    idle_log::error!("D-Bus server stopped: {error}");
                    retries += 1;
                }
                Err(payload) => {
                    let msg = payload
                        .downcast_ref::<&str>()
                        .copied()
                        .or_else(|| payload.downcast_ref::<String>().map(|s| s.as_str()))
                        .unwrap_or("unknown panic");
                    idle_log::error!("D-Bus server thread panicked: {msg}");
                    retries += 1;
                }
                Ok(Ok(())) => break,
            }
            if controller.shutdown.load(Ordering::Relaxed) {
                break;
            }
            idle_log::warn!("Restarting D-Bus server after error or panic...");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if retries >= 3 {
            idle_log::error!(
                "D-Bus server thread exceeded max retries (3); stopping D-Bus thread."
            );
        }
    });
    Ok(handle)
}

#[cfg(test)]
mod signal_tests {
    use super::*;
    use std::sync::Mutex;

    // SHUTDOWN_FLAG and real signal handlers are process-global — serialize.
    static SIG_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn handler_sets_pointed_flag() {
        let _g = SIG_LOCK.lock().unwrap();
        let flag = AtomicBool::new(false);
        SHUTDOWN_FLAG.store(&flag as *const AtomicBool as *mut _, Ordering::SeqCst);
        on_term_sig(libc::SIGINT);
        assert!(flag.load(Ordering::SeqCst));
        SHUTDOWN_FLAG.store(std::ptr::null_mut(), Ordering::SeqCst);
    }

    #[test]
    fn handler_tolerates_null_target() {
        let _g = SIG_LOCK.lock().unwrap();
        SHUTDOWN_FLAG.store(std::ptr::null_mut(), Ordering::SeqCst);
        on_term_sig(libc::SIGTERM); // must not crash
    }

    #[test]
    fn real_signal_delivery_sets_flag() {
        let _g = SIG_LOCK.lock().unwrap();
        let flag = AtomicBool::new(false);
        SHUTDOWN_FLAG.store(&flag as *const AtomicBool as *mut _, Ordering::SeqCst);

        // Install on_term_sig for SIGUSR1 (unused elsewhere), deliver it to
        // this thread, verify the flag trips — end-to-end sigaction path.
        let mut old: libc::sigaction = unsafe { std::mem::zeroed() };
        let mut sa: libc::sigaction = unsafe { std::mem::zeroed() };
        sa.sa_sigaction = on_term_sig as *const () as usize;
        unsafe {
            libc::sigemptyset(&mut sa.sa_mask);
            assert_eq!(libc::sigaction(libc::SIGUSR1, &sa, &mut old), 0);
            assert_eq!(libc::raise(libc::SIGUSR1), 0);
            libc::sigaction(libc::SIGUSR1, &old, std::ptr::null_mut());
        }
        assert!(flag.load(Ordering::SeqCst), "SIGUSR1 must set the flag");
        SHUTDOWN_FLAG.store(std::ptr::null_mut(), Ordering::SeqCst);
    }
}
