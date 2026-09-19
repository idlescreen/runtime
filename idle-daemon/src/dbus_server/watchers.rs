// SPDX-License-Identifier: MIT

//! D-Bus watchers for inhibitor lifecycle.
//!
//! **Important:** We *own* `org.freedesktop.ScreenSaver` and handle Inhibit/UnInhibit
//! in [`super::screensaver`]. Do **not** also sniff those method calls — that
//! double-counted holds with fake cookies (10000+) that Firefox never UnInhibits,
//! leaving stale “Playing video” forever after Firefox exits.

use std::sync::Arc;

use crate::futures_util::next;
use zbus::fdo::DBusProxy;
use zbus::names::BusName;

use crate::controller::DaemonController;
use crate::inhibit::InhibitorState;

/// Drop all inhibitors for a unique name when it leaves the bus.
pub async fn watch_inhibitor_clients(
    connection: zbus::Connection,
    inhibitors: Arc<InhibitorState>,
    controller: Arc<DaemonController>,
) {
    let dbus = match DBusProxy::new(&connection).await {
        Ok(proxy) => proxy,
        Err(error) => {
            idle_log::error!("failed to watch inhibitor clients: {error}");
            return;
        }
    };

    let mut stream = match dbus.receive_name_owner_changed().await {
        Ok(stream) => stream,
        Err(error) => {
            idle_log::error!("failed to subscribe to NameOwnerChanged: {error}");
            return;
        }
    };

    while let Some(event) = next(&mut stream).await {
        let args = match event.args() {
            Ok(args) => args,
            Err(_) => continue,
        };
        // Only care about names that disappeared.
        if args.new_owner.is_some() {
            continue;
        }
        // Unique connection names (`:1.NNN`) are what Inhibit senders use.
        let BusName::Unique(name) = &args.name else {
            continue;
        };
        let before = inhibitors.len();
        inhibitors.remove_client(name);
        let after = inhibitors.len();
        if before != after {
            idle_log::info!(
                "cleared {} inhibitor(s) for departed peer {}",
                before - after,
                name
            );
            controller.mark_dirty();
        }
    }
}

/// Sniff **only** interfaces we do not implement ourselves (see `sniff_policy`).
pub async fn watch_external_dbus_inhibits(
    connection: zbus::Connection,
    inhibitors: Arc<InhibitorState>,
    controller: Arc<DaemonController>,
) {
    use zbus::MatchRule;
    use zbus::message::Type;

    use super::sniff_policy::{may_sniff_screensaver_interface, sniffable_screensaver_interfaces};

    // Hard guard: never reintroduce freeness sniffing in this function.
    debug_assert!(!may_sniff_screensaver_interface(
        "org.freedesktop.ScreenSaver"
    ));

    for iface in sniffable_screensaver_interfaces() {
        if !may_sniff_screensaver_interface(iface) {
            continue;
        }
        let Ok(builder) = MatchRule::builder()
            .msg_type(Type::MethodCall)
            .interface(*iface)
        else {
            continue;
        };
        let rule = builder.build();
        match zbus::MessageStream::for_match_rule(rule, &connection, None).await {
            Ok(stream) => {
                let inhibitors = inhibitors.clone();
                let controller = controller.clone();
                tokio::spawn(async move {
                    process_message_stream(stream, inhibitors, controller).await;
                });
            }
            Err(err) => {
                idle_log::debug!("No {iface} match (ok if unused on this DE): {err}");
            }
        }
    }
    // Keep this task alive so spawned sniffer tasks are not the only owners;
    // they outlive us via the runtime. Loop until shutdown requested.
    while !controller
        .shutdown
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

async fn process_message_stream(
    mut stream: zbus::MessageStream,
    inhibitors: Arc<InhibitorState>,
    controller: Arc<DaemonController>,
) {
    while let Some(Ok(msg)) = next(&mut stream).await {
        let header = msg.header();
        let member = match header.member() {
            Some(m) => m.as_str(),
            None => continue,
        };

        let sender = match header.sender() {
            Some(s) => s.to_owned(),
            None => continue,
        };

        match member {
            "Inhibit" => {
                if let Ok((app, reason)) = msg.body().deserialize::<(String, String)>() {
                    // Coalesced add: same client/app/reason reuses one hold.
                    match inhibitors.add(app.clone(), reason.clone(), sender.clone()) {
                        Ok(cookie) => {
                            idle_log::info!(
                                "GNOME ScreenSaver Inhibit from {} ({}: {}) cookie={}",
                                sender,
                                app,
                                reason,
                                cookie
                            );
                            controller.mark_dirty();
                        }
                        Err(e) => {
                            idle_log::warn!("GNOME ScreenSaver Inhibit rejected: {e}");
                        }
                    }
                }
            }
            "UnInhibit" => {
                if let Ok(cookie) = msg.body().deserialize::<u32>()
                    && inhibitors.remove_for_client(cookie, &sender)
                {
                    idle_log::info!(
                        "GNOME ScreenSaver UnInhibit from {} cookie={}",
                        sender,
                        cookie
                    );
                    controller.mark_dirty();
                }
            }
            _ => {}
        }
    }
}
