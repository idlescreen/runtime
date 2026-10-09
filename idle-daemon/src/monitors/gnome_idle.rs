// SPDX-License-Identifier: MIT

//! GNOME Mutter idle monitor via `org.gnome.Mutter.IdleMonitor` D-Bus interface.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;
use tokio::sync::mpsc;

#[zbus::proxy(
    interface = "org.gnome.Mutter.IdleMonitor",
    default_service = "org.gnome.Mutter.IdleMonitor",
    default_path = "/org/gnome/Mutter/IdleMonitor/Core"
)]
pub trait MutterIdleMonitor {
    fn add_idle_watch(&self, interval: u64) -> zbus::Result<u32>;
    fn add_user_active_watch(&self) -> zbus::Result<u32>;
    fn remove_watch(&self, id: u32) -> zbus::Result<()>;
    fn get_idletime(&self) -> zbus::Result<u64>;

    #[zbus(signal)]
    fn watch_fired(&self, id: u32) -> zbus::Result<()>;
}

pub struct GnomeIdleMonitor {
    is_idle: Arc<AtomicBool>,
    is_alive: Arc<AtomicBool>,
    timeout_tx: Option<mpsc::UnboundedSender<Duration>>,
    event_thread: Option<JoinHandle<()>>,
}

impl GnomeIdleMonitor {
    pub fn new(timeout: Duration) -> Option<Self> {
        if !Self::is_available() {
            return None;
        }

        let is_idle = Arc::new(AtomicBool::new(false));
        let is_alive = Arc::new(AtomicBool::new(true));
        let (timeout_tx, timeout_rx) = mpsc::unbounded_channel();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();

        let thread_idle = is_idle.clone();
        let thread_alive = is_alive.clone();

        let event_thread = std::thread::Builder::new()
            .name("gnome-idle".into())
            .spawn(move || {
                if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    rt.block_on(run_idle_loop(
                        thread_idle,
                        thread_alive,
                        timeout_rx,
                        timeout,
                        ready_tx,
                    ));
                } else {
                    let _ = ready_tx.send(false);
                }
            })
            .ok()?;

        match ready_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(true) => Some(Self {
                is_idle,
                is_alive,
                timeout_tx: Some(timeout_tx),
                event_thread: Some(event_thread),
            }),
            _ => {
                drop(timeout_tx);
                let _ = event_thread.join();
                None
            }
        }
    }

    pub fn is_available() -> bool {
        std::env::var("WAYLAND_DISPLAY").is_ok()
    }
}

async fn sync_idle_state(
    proxy: &MutterIdleMonitorProxy<'_>,
    is_idle: &AtomicBool,
    active_watch: &mut Option<u32>,
    threshold_ms: u64,
) {
    if let Ok(curr) = proxy.get_idletime().await {
        if curr >= threshold_ms {
            is_idle.store(true, Ordering::SeqCst);
            if active_watch.is_none() {
                *active_watch = proxy.add_user_active_watch().await.ok();
            }
        } else if is_idle.load(Ordering::SeqCst) {
            is_idle.store(false, Ordering::SeqCst);
            if let Some(act_id) = active_watch.take() {
                let _ = proxy.remove_watch(act_id).await;
            }
        }
    }
}

async fn run_idle_loop(
    is_idle: Arc<AtomicBool>,
    is_alive: Arc<AtomicBool>,
    mut timeout_rx: mpsc::UnboundedReceiver<Duration>,
    initial_timeout: Duration,
    ready_tx: std::sync::mpsc::Sender<bool>,
) {
    let Ok(connection) = zbus::Connection::session().await else {
        let _ = ready_tx.send(false);
        return;
    };
    let Ok(proxy) = MutterIdleMonitorProxy::new(&connection).await else {
        let _ = ready_tx.send(false);
        return;
    };

    let mut current_ms = initial_timeout.as_millis().min(u64::MAX as u128) as u64;
    let Ok(mut idle_watch) = proxy.add_idle_watch(current_ms).await else {
        let _ = ready_tx.send(false);
        return;
    };

    let mut active_watch: Option<u32> = None;
    sync_idle_state(&proxy, &is_idle, &mut active_watch, current_ms).await;

    let Ok(mut stream) = proxy.receive_watch_fired().await else {
        let _ = proxy.remove_watch(idle_watch).await;
        let _ = ready_tx.send(false);
        return;
    };

    let _ = ready_tx.send(true);

    loop {
        tokio::select! {
            cmd = timeout_rx.recv() => match cmd {
                Some(new_timeout) => {
                    let new_ms = new_timeout.as_millis().min(u64::MAX as u128) as u64;
                    if new_ms != current_ms {
                        let _ = proxy.remove_watch(idle_watch).await;
                        if let Ok(new_id) = proxy.add_idle_watch(new_ms).await {
                            idle_watch = new_id;
                            current_ms = new_ms;
                            sync_idle_state(&proxy, &is_idle, &mut active_watch, new_ms).await;
                        } else {
                            is_alive.store(false, Ordering::SeqCst);
                            break;
                        }
                    }
                }
                None => break,
            },
            signal = crate::futures_util::next(&mut stream) => match signal {
                Some(sig) => if let Ok(args) = sig.args() {
                    if args.id == idle_watch {
                        is_idle.store(true, Ordering::SeqCst);
                        if active_watch.is_none() {
                            active_watch = proxy.add_user_active_watch().await.ok();
                        }
                    } else if Some(args.id) == active_watch {
                        is_idle.store(false, Ordering::SeqCst);
                        active_watch = None;
                    }
                },
                None => {
                    is_alive.store(false, Ordering::SeqCst);
                    break;
                }
            },
        }
    }

    let _ = proxy.remove_watch(idle_watch).await;
    if let Some(act_id) = active_watch {
        let _ = proxy.remove_watch(act_id).await;
    }
}

impl Drop for GnomeIdleMonitor {
    fn drop(&mut self) {
        drop(self.timeout_tx.take());
        if let Some(handle) = self.event_thread.take() {
            let _ = handle.join();
        }
    }
}

impl idle_api::IdleSource for GnomeIdleMonitor {
    fn is_available() -> bool {
        Self::is_available()
    }

    fn new(timeout: Duration) -> Option<Self> {
        Self::new(timeout)
    }

    fn is_idle(&self) -> bool {
        self.is_idle.load(Ordering::SeqCst)
    }

    fn is_alive(&self) -> bool {
        self.is_alive.load(Ordering::SeqCst)
    }

    fn set_timeout(&self, timeout: Duration) {
        if let Some(tx) = &self.timeout_tx {
            let _ = tx.send(timeout);
        }
    }
}

#[cfg(test)]
#[path = "gnome_idle_tests.rs"]
mod tests;
