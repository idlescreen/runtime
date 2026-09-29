// SPDX-License-Identifier: MIT

mod auth;
#[cfg(test)]
mod dbus_validation_tests;
#[cfg(test)]
mod queue_overflow_tests;
mod screensaver;
mod service;
pub mod service_helpers;
mod sniff_policy;
mod watchers;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use idle_dbus::{OBJECT_PATH, SERVICE_NAME};
use idle_err::Context;
use zbus::fdo::RequestNameFlags;

use crate::controller::DaemonController;
use crate::{lock_monitor, sleep_monitor};

use service::TranceService;

pub fn run(controller: Arc<DaemonController>) -> idle_err::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(4)
        .thread_name("idle-dbus")
        .build()
        .context("building D-Bus tokio runtime")?;

    let res = runtime.block_on(serve(controller));
    runtime.shutdown_timeout(Duration::from_millis(500));
    res
}

async fn serve(controller: Arc<DaemonController>) -> idle_err::Result<()> {
    let (status_emit_tx, status_emit_rx) = tokio::sync::mpsc::channel(64);
    {
        let mut slot = controller
            .status_emit_tx
            .lock()
            .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p));
        *slot = Some(status_emit_tx);
    }

    let flags = RequestNameFlags::ReplaceExisting | RequestNameFlags::AllowReplacement;

    let connection = zbus::connection::Builder::session()
        .context("opening D-Bus session connection")?
        .name(SERVICE_NAME)
        .with_context(|| format!("claiming D-Bus name {SERVICE_NAME}"))?
        .serve_at(
            OBJECT_PATH,
            TranceService {
                controller: controller.clone(),
            },
        )
        .with_context(|| format!("serving object at {OBJECT_PATH}"))?
        .serve_at(
            "/org/freedesktop/ScreenSaver",
            screensaver::ScreenSaverService {
                controller: controller.clone(),
            },
        )
        .context("serving ScreenSaver object")?
        .build()
        .await
        .context("building D-Bus connection")?;

    let _ = connection
        .request_name_with_flags("org.freedesktop.ScreenSaver", flags)
        .await;

    controller.set_dbus_connection(connection.clone());

    idle_log::info!("exporting D-Bus service {SERVICE_NAME}");

    tokio::spawn(lock_monitor::watch_session_lock(
        controller.session_locked.clone(),
        controller.shutdown.clone(),
    ));

    tokio::spawn(sleep_monitor::watch_prepare_for_sleep(
        controller.watchdog.clone(),
        controller.shutdown.clone(),
    ));

    tokio::spawn(watchers::watch_inhibitor_clients(
        connection.clone(),
        controller.inhibitors.clone(),
        controller.clone(),
    ));

    tokio::spawn(watchers::watch_external_dbus_inhibits(
        connection.clone(),
        controller.inhibitors.clone(),
        controller.clone(),
    ));

    tokio::spawn(emit_status_changes(
        connection.clone(),
        status_emit_rx,
        controller.shutdown.clone(),
    ));

    while !controller.shutdown.load(Ordering::Relaxed) {
        if connection.is_closed() {
            idle_log::error!("D-Bus connection closed unexpectedly");
            idle_err::bail!("D-Bus connection closed unexpectedly");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    Ok(())
}

pub async fn emit_status_changes(
    connection: zbus::Connection,
    mut receiver: tokio::sync::mpsc::Receiver<idle_dbus::DaemonStatus>,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
) {
    while !shutdown.load(Ordering::Relaxed) {
        if connection.is_closed() {
            break;
        }
        tokio::select! {
            opt = receiver.recv() => match opt {
                Some(mut status) => {
                    while let Ok(latest) = receiver.try_recv() {
                        status = latest;
                    }
                    let map = status.to_map();
                    if let Ok(emitter) =
                        zbus::object_server::SignalEmitter::new(&connection, OBJECT_PATH)
                    {
                        let _ = TranceService::status_changed(&emitter, map).await;
                    }
                }
                None => break,
            },
            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
        }
    }
}
