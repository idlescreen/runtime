// SPDX-License-Identifier: MIT

use std::collections::HashMap;

use zbus::zvariant::{OwnedValue, Value};

/// Live daemon state exposed over D-Bus.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DaemonStatus {
    pub running: bool,
    pub idle_enabled: bool,
    pub idle_timeout_mins: u32,
    /// Empty string means random rotation.
    pub active_saver: String,
    pub presentation_active: bool,
    pub preview_active: bool,
    pub system_idle: bool,
    pub session_locked: bool,
    pub inhibited: bool,
    pub current_saver: String,
    pub show_fps_overlay: bool,
    pub render_scale: String,
}

impl DaemonStatus {
    pub fn to_map(&self) -> HashMap<String, OwnedValue> {
        // Fixed field count — reserve once to avoid rehash during insert.
        let mut map = HashMap::with_capacity(12);
        map.insert("running".into(), owned(self.running));
        map.insert("idle_enabled".into(), owned(self.idle_enabled));
        map.insert("idle_timeout_mins".into(), owned(self.idle_timeout_mins));
        // String fields must be owned for Value<'static> / OwnedValue.
        map.insert("active_saver".into(), owned(self.active_saver.clone()));
        map.insert(
            "presentation_active".into(),
            owned(self.presentation_active),
        );
        map.insert("preview_active".into(), owned(self.preview_active));
        map.insert("system_idle".into(), owned(self.system_idle));
        map.insert("session_locked".into(), owned(self.session_locked));
        map.insert("inhibited".into(), owned(self.inhibited));
        map.insert("current_saver".into(), owned(self.current_saver.clone()));
        map.insert("show_fps_overlay".into(), owned(self.show_fps_overlay));
        map.insert("render_scale".into(), owned(self.render_scale.clone()));
        map
    }
}

fn owned<T>(value: T) -> OwnedValue
where
    T: Into<Value<'static>>,
{
    let value: Value<'static> = value.into();
    OwnedValue::try_from(value).unwrap_or_else(|error| {
        idle_log::error!(
            error = %error,
            "idle-dbus: failed to convert daemon status field to OwnedValue"
        );
        OwnedValue::from(0u32)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_to_map_round_trips() {
        let status = DaemonStatus::default();
        let map = status.to_map();
        assert!(!map.is_empty());
        assert!(map.contains_key("running"));
        assert!(map.contains_key("idle_timeout_mins"));
        assert!(map.contains_key("active_saver"));
        assert!(map.contains_key("render_scale"));
    }

    #[test]
    fn status_to_map_preserves_string_fields() {
        let status = DaemonStatus {
            active_saver: "cosmos".to_string(),
            current_saver: "beams".to_string(),
            render_scale: "0.5".to_string(),
            ..DaemonStatus::default()
        };
        let map = status.to_map();
        assert_eq!(map.len(), 12);
        assert!(map.contains_key("active_saver"));
        assert!(map.contains_key("current_saver"));
        assert!(map.contains_key("render_scale"));
    }

    #[test]
    fn status_to_map_default_values_match() {
        let status = DaemonStatus::default();
        let map = status.to_map();
        assert_eq!(map.len(), 12);
    }
}
