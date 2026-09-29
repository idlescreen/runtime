// SPDX-License-Identifier: MIT

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, PanicHookInfo, catch_unwind};
use std::sync::Mutex;

use zbus::zvariant::OwnedValue;

use crate::status::DaemonStatus;
use crate::{OBJECT_PATH, SERVICE_NAME};

static PANIC_HOOK_LOCK: Mutex<()> = Mutex::new(());

struct PanicHookGuard {
    prev_hook: Option<Box<dyn Fn(&PanicHookInfo<'_>) + Sync + Send + 'static>>,
}

impl PanicHookGuard {
    fn suppress() -> Self {
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        Self {
            prev_hook: Some(prev_hook),
        }
    }
}

impl Drop for PanicHookGuard {
    fn drop(&mut self) {
        if let Some(prev) = self.prev_hook.take() {
            std::panic::set_hook(prev);
        }
    }
}

fn catch_unwind_silent<F, R>(f: F) -> std::thread::Result<R>
where
    F: FnOnce() -> R,
{
    let _lock = PANIC_HOOK_LOCK
        .lock()
        .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p));
    let _guard = PanicHookGuard::suppress();
    catch_unwind(AssertUnwindSafe(f))
}

fn check_fd_availability(required: usize) -> bool {
    let mut fds = Vec::with_capacity(required);
    let mut available = true;
    for _ in 0..required {
        match std::fs::File::open("/dev/null") {
            Ok(file) => fds.push(file),
            Err(_) => {
                available = false;
                break;
            }
        }
    }
    drop(fds);
    available
}

#[zbus::proxy(
    interface = "io.github.idlescreen.Idle",
    default_service = "io.github.idlescreen.Idle",
    default_path = "/io/github/idlescreen/Idle",
    gen_blocking = true
)]
trait Idle {
    fn get_status(&self) -> zbus::Result<HashMap<String, OwnedValue>>;
    fn enable(&self) -> zbus::Result<()>;
    fn disable(&self) -> zbus::Result<()>;
    fn set_timeout(&self, minutes: u32) -> zbus::Result<()>;
    fn set_saver(&self, name: &str) -> zbus::Result<()>;
    fn list_savers(&self) -> zbus::Result<Vec<String>>;
    fn preview(&self, name: &str) -> zbus::Result<()>;
    fn activate(&self) -> zbus::Result<()>;
    fn stop_preview(&self) -> zbus::Result<()>;
    fn inhibit(&self, application: &str, reason: &str) -> zbus::Result<u32>;
    fn un_inhibit(&self, cookie: u32) -> zbus::Result<()>;
    fn list_inhibitors(&self) -> zbus::Result<Vec<(u32, String, String)>>;
    fn set_gpu_enabled(&self, enabled: bool) -> zbus::Result<()>;
    fn set_show_fps_overlay(&self, enabled: bool) -> zbus::Result<()>;
    fn set_render_scale(&self, scale: f64) -> zbus::Result<()>;
}

/// Blocking D-Bus client for the IdleScreen daemon.
pub struct TranceClient {
    connection: zbus::blocking::Connection,
}

impl TranceClient {
    pub fn connect() -> zbus::Result<Self> {
        if !check_fd_availability(4) {
            return Err(zbus::Error::Failure(
                "insufficient file descriptors available".into(),
            ));
        }
        let res = catch_unwind_silent(|| {
            let connection = zbus::blocking::Connection::session()?;
            let proxy = IdleProxyBlocking::new(&connection)?;
            proxy.get_status()?;
            Ok(Self { connection })
        });
        match res {
            Ok(r) => r,
            Err(_) => Err(zbus::Error::Failure(
                "panic connecting to D-Bus session".into(),
            )),
        }
    }

    pub fn get_status(&self) -> zbus::Result<DaemonStatus> {
        let map = IdleProxyBlocking::new(&self.connection)?.get_status()?;
        parse_status(map)
    }

    pub fn enable(&self) -> zbus::Result<()> {
        IdleProxyBlocking::new(&self.connection)?.enable()
    }

    pub fn disable(&self) -> zbus::Result<()> {
        IdleProxyBlocking::new(&self.connection)?.disable()
    }

    pub fn set_timeout(&self, minutes: u32) -> zbus::Result<()> {
        IdleProxyBlocking::new(&self.connection)?.set_timeout(minutes)
    }

    pub fn set_saver(&self, name: &str) -> zbus::Result<()> {
        IdleProxyBlocking::new(&self.connection)?.set_saver(name)
    }

    pub fn list_savers(&self) -> zbus::Result<Vec<String>> {
        IdleProxyBlocking::new(&self.connection)?.list_savers()
    }

    pub fn preview(&self, name: &str) -> zbus::Result<()> {
        IdleProxyBlocking::new(&self.connection)?.preview(name)
    }

    /// Force-activate the configured saver (`idlescreen start`). Older
    /// daemons without the method surface a D-Bus error to the caller.
    pub fn activate(&self) -> zbus::Result<()> {
        IdleProxyBlocking::new(&self.connection)?.activate()
    }

    pub fn stop_preview(&self) -> zbus::Result<()> {
        IdleProxyBlocking::new(&self.connection)?.stop_preview()
    }

    pub fn inhibit(&self, application: &str, reason: &str) -> zbus::Result<u32> {
        IdleProxyBlocking::new(&self.connection)?.inhibit(application, reason)
    }

    pub fn un_inhibit(&self, cookie: u32) -> zbus::Result<()> {
        IdleProxyBlocking::new(&self.connection)?.un_inhibit(cookie)
    }

    pub fn list_inhibitors(&self) -> zbus::Result<Vec<(u32, String, String)>> {
        IdleProxyBlocking::new(&self.connection)?.list_inhibitors()
    }

    pub fn set_gpu_enabled(&self, enabled: bool) -> zbus::Result<()> {
        IdleProxyBlocking::new(&self.connection)?.set_gpu_enabled(enabled)
    }

    pub fn set_show_fps_overlay(&self, enabled: bool) -> zbus::Result<()> {
        IdleProxyBlocking::new(&self.connection)?.set_show_fps_overlay(enabled)
    }

    pub fn set_render_scale(&self, scale: f32) -> zbus::Result<()> {
        IdleProxyBlocking::new(&self.connection)?.set_render_scale(f64::from(scale))
    }
}

fn parse_status(map: HashMap<String, OwnedValue>) -> zbus::Result<DaemonStatus> {
    Ok(DaemonStatus {
        running: read_bool(&map, "running"),
        idle_enabled: read_bool(&map, "idle_enabled"),
        idle_timeout_mins: read_u32(&map, "idle_timeout_mins"),
        active_saver: read_string(&map, "active_saver"),
        presentation_active: read_bool(&map, "presentation_active"),
        preview_active: read_bool(&map, "preview_active"),
        system_idle: read_bool(&map, "system_idle"),
        session_locked: read_bool(&map, "session_locked"),
        inhibited: read_bool(&map, "inhibited"),
        current_saver: read_string(&map, "current_saver"),
        show_fps_overlay: read_bool(&map, "show_fps_overlay"),
        render_scale: read_string(&map, "render_scale"),
    })
}

fn read_bool(map: &HashMap<String, OwnedValue>, key: &str) -> bool {
    map.get(key)
        .and_then(|value| value.downcast_ref::<bool>().ok())
        .unwrap_or(false)
}

fn read_u32(map: &HashMap<String, OwnedValue>, key: &str) -> u32 {
    map.get(key)
        .and_then(|value| value.downcast_ref::<u32>().ok())
        .unwrap_or(0)
}

fn read_string(map: &HashMap<String, OwnedValue>, key: &str) -> String {
    map.get(key)
        .and_then(|value| value.downcast_ref::<String>().ok())
        .unwrap_or_default()
}

/// Returns whether the IdleScreen daemon is reachable on the session bus.
pub fn daemon_available() -> bool {
    if !check_fd_availability(4) {
        return false;
    }
    let res = catch_unwind_silent(|| {
        let connection = match zbus::blocking::Connection::session() {
            Ok(connection) => connection,
            Err(_) => return false,
        };
        let dbus = match zbus::blocking::fdo::DBusProxy::new(&connection) {
            Ok(dbus) => dbus,
            Err(_) => return false,
        };

        if let Ok(bus) = zbus::names::BusName::try_from(SERVICE_NAME)
            && dbus.name_has_owner(bus).unwrap_or(false)
        {
            return true;
        }
        let _ = OBJECT_PATH;
        false
    });
    res.unwrap_or(false)
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
