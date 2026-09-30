// SPDX-License-Identifier: MIT

//! MPRIS media player monitor for automatic idle inhibition during playback.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use zbus::fdo::DBusProxy;
use zbus::message::Type;
use zbus::names::{BusName, UniqueName};
use zbus::zvariant::Value;
use zbus::{MatchRule, MessageStream};

use crate::controller::DaemonController;
use crate::futures_util::next;

/// Extract player name from well-known bus name (e.g. `org.mpris.MediaPlayer2.vlc` -> `vlc`).
pub fn player_name_from_bus_name(bus_name: &str) -> Option<&str> {
    bus_name.strip_prefix("org.mpris.MediaPlayer2.")
}

async fn query_playback_status(connection: &zbus::Connection, dest: &str) -> Option<String> {
    for path in &["/org/mpris/MediaPlayer2", "/org/mpris/MediaPlayer2/Player"] {
        if let Ok(reply) = connection
            .call_method(
                Some(dest),
                *path,
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &("org.mpris.MediaPlayer2.Player", "PlaybackStatus"),
            )
            .await
            && let Ok((val,)) = reply.body().deserialize::<(zbus::zvariant::OwnedValue,)>()
        {
            let v: zbus::zvariant::Value = val.into();
            if let Value::Str(s) = &v {
                return Some(s.to_string());
            }
        }
    }
    None
}

pub(crate) fn sync_media_inhibit(
    controller: &DaemonController,
    unique_owner: &str,
    player_name: &str,
    playing: bool,
) {
    let Ok(unique) = UniqueName::try_from(unique_owner.to_string()) else {
        return;
    };
    let inhibit_enabled = controller
        .config
        .lock()
        .map(|c| c.inhibit_on_media)
        .unwrap_or(true);

    controller.inhibitors.remove_client(&unique);
    if playing && inhibit_enabled {
        let _ = controller.inhibitors.add(
            player_name.to_string(),
            "PlaybackStatus=Playing".into(),
            unique,
        );
    }
    controller.mark_dirty();
}

pub async fn watch_media_players(connection: zbus::Connection, controller: Arc<DaemonController>) {
    let dbus = match DBusProxy::new(&connection).await {
        Ok(proxy) => proxy,
        Err(e) => {
            idle_log::error!("failed to create DBusProxy for media watch: {e}");
            return;
        }
    };

    let mut player_names: HashMap<String, String> = HashMap::new();

    if let Ok(names) = dbus.list_names().await {
        for name in names {
            let name_str = name.as_str();
            if let Some(player) = player_name_from_bus_name(name_str)
                && let Ok(owner) = dbus.get_name_owner(name.as_ref()).await
            {
                let owner_str = owner.to_string();
                player_names.insert(owner_str.clone(), player.to_string());
                if let Some(status) = query_playback_status(&connection, &owner_str).await
                    && status == "Playing"
                {
                    sync_media_inhibit(&controller, &owner_str, player, true);
                }
            }
        }
    }

    let mut owner_stream = match dbus.receive_name_owner_changed().await {
        Ok(s) => s,
        Err(e) => {
            idle_log::error!("failed to subscribe to NameOwnerChanged: {e}");
            return;
        }
    };

    let rule = match MatchRule::builder()
        .msg_type(Type::Signal)
        .interface("org.freedesktop.DBus.Properties")
        .and_then(|b| b.member("PropertiesChanged"))
        .and_then(|b| b.arg(0, "org.mpris.MediaPlayer2.Player"))
    {
        Ok(b) => b.build(),
        Err(e) => {
            idle_log::error!("failed to build MatchRule for MPRIS: {e}");
            return;
        }
    };

    let mut prop_stream = match MessageStream::for_match_rule(rule, &connection, None).await {
        Ok(s) => s,
        Err(e) => {
            idle_log::warn!("failed to subscribe to MPRIS PropertiesChanged: {e}");
            return;
        }
    };

    while !controller.shutdown.load(Ordering::Relaxed) {
        tokio::select! {
            Some(event) = next(&mut owner_stream) => {
                let Ok(args) = event.args() else { continue };
                let BusName::WellKnown(ref well_known) = args.name else { continue };
                let Some(player) = player_name_from_bus_name(well_known.as_str()) else { continue };

                if let Some(ref old_owner) = *args.old_owner {
                    player_names.remove(old_owner.as_str());
                    sync_media_inhibit(&controller, old_owner.as_str(), player, false);
                }
                if let Some(ref new_owner) = *args.new_owner {
                    let owner_str = new_owner.to_string();
                    player_names.insert(owner_str.clone(), player.to_string());
                    if let Some(status) = query_playback_status(&connection, &owner_str).await {
                        sync_media_inhibit(&controller, &owner_str, player, status == "Playing");
                    }
                }
            }
            Some(Ok(msg)) = next(&mut prop_stream) => {
                let header = msg.header();
                let Some(sender) = header.sender() else { continue };
                let sender_str = sender.as_str();
                let body = msg.body();
                let Ok((_iface, changed, _inv)) =
                    body.deserialize::<(String, HashMap<String, Value>, Vec<String>)>()
                else {
                    continue;
                };
                if let Some(status_val) = changed.get("PlaybackStatus") {
                    let is_playing = match status_val {
                        Value::Str(s) => s.as_str() == "Playing",
                        _ => false,
                    };
                    let app_name = player_names
                        .get(sender_str)
                        .cloned()
                        .unwrap_or_else(|| "mpris".to_string());
                    sync_media_inhibit(&controller, sender_str, &app_name, is_playing);
                } else if _inv.iter().any(|p| p == "PlaybackStatus")
                    && let Some(status) = query_playback_status(&connection, sender_str).await
                {
                    let app_name = player_names
                        .get(sender_str)
                        .cloned()
                        .unwrap_or_else(|| "mpris".to_string());
                    sync_media_inhibit(&controller, sender_str, &app_name, status == "Playing");
                }
            }
            else => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_player_name_from_bus_name() {
        assert_eq!(
            player_name_from_bus_name("org.mpris.MediaPlayer2.spotify"),
            Some("spotify")
        );
        assert_eq!(
            player_name_from_bus_name("org.mpris.MediaPlayer2.vlc"),
            Some("vlc")
        );
        assert_eq!(
            player_name_from_bus_name("org.mpris.MediaPlayer2.firefox.instance_1"),
            Some("firefox.instance_1")
        );
        assert_eq!(player_name_from_bus_name("org.freedesktop.DBus"), None);
    }

    #[test]
    fn test_sync_media_inhibit_adds_and_removes() {
        let config = crate::config::DaemonConfig::default();
        let controller = DaemonController::new(config);
        let owner = ":1.4242";
        let app = "spotify";

        sync_media_inhibit(&controller, owner, app, true);
        assert!(controller.inhibitors.is_inhibited());
        let list = controller.inhibitors.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].1, "spotify");
        assert_eq!(list[0].2, "PlaybackStatus=Playing");

        sync_media_inhibit(&controller, owner, app, false);
        assert!(!controller.inhibitors.is_inhibited());
        assert_eq!(controller.inhibitors.list().len(), 0);
    }

    #[test]
    fn test_sync_media_inhibit_respects_config_disabled() {
        let config = crate::config::DaemonConfig {
            inhibit_on_media: false,
            ..Default::default()
        };
        let controller = DaemonController::new(config);
        let owner = ":1.4242";

        sync_media_inhibit(&controller, owner, "vlc", true);
        assert!(!controller.inhibitors.is_inhibited());
        assert_eq!(controller.inhibitors.list().len(), 0);
    }

    #[test]
    fn test_sync_media_inhibit_replaces_existing_hold_without_duplicates() {
        let config = crate::config::DaemonConfig::default();
        let controller = DaemonController::new(config);
        let owner = ":1.4242";

        sync_media_inhibit(&controller, owner, "mpris", true);
        sync_media_inhibit(&controller, owner, "spotify", true);
        let list = controller.inhibitors.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].1, "spotify");
    }
}
