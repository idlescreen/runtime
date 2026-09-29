// SPDX-License-Identifier: MIT

//! External idle blocks (logind + MPRIS) — same sources as [`super::InhibitorState::is_inhibited`].

/// One external block for CLI/status listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalInhibitor {
    /// e.g. `logind`, `mpris`
    pub source: String,
    /// Application / player name
    pub who: String,
    /// Human reason
    pub why: String,
}

/// Whether a logind idle hold should be ignored by IdleScreen.
///
/// Coding agents (e.g. Grok) take a session `idle` inhibit so the DE does not
/// blank during a turn. That must **not**:
/// - appear in `idlescreen inhibitors`, or
/// - block idle-driven savers / force doctor FAIL for "inhibited",
///   because it is not a user media/fullscreen intent.
///
/// Forced preview (`idlescreen preview` / TUI `p`) already ignores *all*
/// inhibitors in presentation policy; this filter cleans list + idle path.
pub fn ignore_logind_idle_hold(who: &str, why: &str) -> bool {
    // logind Who is typically "grok"; after format we store "grok (block)".
    let who_base = who
        .split(|c: char| c.is_whitespace() || c == '(')
        .next()
        .unwrap_or("")
        .trim();
    if who_base.eq_ignore_ascii_case("grok") {
        return true;
    }
    let why_l = why.to_ascii_lowercase();
    why_l.contains("agent turn")
}

#[cfg(all(target_os = "linux", not(test)))]
type LogindInhibitorInfo = (String, String, String, String, u32, u32);

/// True when logind has any **IdleScreen-relevant** idle inhibitor.
#[cfg(all(target_os = "linux", not(test)))]
pub fn list_logind_idle() -> Vec<ExternalInhibitor> {
    use super::zbus_helper::safe_zbus_blocking;
    safe_zbus_blocking(|| {
        let Ok(conn) = zbus::blocking::Connection::system() else {
            return Vec::new();
        };
        let Ok(reply) = conn.call_method(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1",
            Some("org.freedesktop.login1.Manager"),
            "ListInhibitors",
            &(),
        ) else {
            return Vec::new();
        };
        let Ok(inhibitors): Result<Vec<LogindInhibitorInfo>, _> = reply.body().deserialize() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (what, who, why, mode, _uid, _pid) in inhibitors {
            if !what.split(':').any(|w| w == "idle") {
                continue;
            }
            if ignore_logind_idle_hold(&who, &why) {
                continue;
            }
            out.push(ExternalInhibitor {
                source: "logind".into(),
                who: format!("{who} ({mode})"),
                why,
            });
        }
        out
    })
    .unwrap_or_default()
}

#[cfg(all(target_os = "linux", not(test)))]
pub fn list_mpris_playing() -> Vec<ExternalInhibitor> {
    use super::zbus_helper::safe_zbus_blocking;
    safe_zbus_blocking(|| {
        let Ok(conn) = zbus::blocking::Connection::session() else {
            return Vec::new();
        };
        let Ok(names_reply) = conn.call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "ListNames",
            &(),
        ) else {
            return Vec::new();
        };
        let Ok(names): Result<Vec<String>, _> = names_reply.body().deserialize() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for name in names {
            if !name.starts_with("org.mpris.MediaPlayer2.") {
                continue;
            }
            // Prefer Player path used by most apps.
            let playing = mpris_status_playing(&conn, &name, "/org/mpris/MediaPlayer2/Player")
                || mpris_status_playing(&conn, &name, "/org/mpris/MediaPlayer2");
            if playing {
                let short = name
                    .strip_prefix("org.mpris.MediaPlayer2.")
                    .unwrap_or(name.as_str());
                out.push(ExternalInhibitor {
                    source: "mpris".into(),
                    who: short.to_string(),
                    why: "PlaybackStatus=Playing".into(),
                });
            }
        }
        out
    })
    .unwrap_or_default()
}

#[cfg(all(target_os = "linux", not(test)))]
fn mpris_status_playing(conn: &zbus::blocking::Connection, name: &str, path: &str) -> bool {
    let Ok(prop_reply) = conn.call_method(
        Some(name),
        path,
        Some("org.freedesktop.DBus.Properties"),
        "Get",
        &("org.mpris.MediaPlayer2.Player", "PlaybackStatus"),
    ) else {
        return false;
    };
    let body = prop_reply.body();
    let Ok(val) = body.deserialize::<zbus::zvariant::Value>() else {
        return false;
    };
    match val.downcast::<String>() {
        Ok(s) => s == "Playing",
        Err(_) => false,
    }
}

#[cfg(any(not(target_os = "linux"), test))]
pub fn list_logind_idle() -> Vec<ExternalInhibitor> {
    Vec::new()
}

#[cfg(any(not(target_os = "linux"), test))]
pub fn list_mpris_playing() -> Vec<ExternalInhibitor> {
    Vec::new()
}

/// All external blocks currently considered by IdleScreen.
pub fn list_external() -> Vec<ExternalInhibitor> {
    if std::env::var("IDLE_TEST_MOCK_AC").is_ok()
        || std::env::var("IDLE_TEST_DISABLE_EXTERNAL").is_ok()
    {
        return Vec::new();
    }
    let mut out = list_logind_idle();
    out.extend(list_mpris_playing());
    out
}

#[cfg(test)]
mod ignore_tests {
    use super::ignore_logind_idle_hold;

    #[test]
    fn ignores_grok_who() {
        // Regression: Grok listed as inhibitor during agent turns.
        assert!(ignore_logind_idle_hold("grok", "agent turn in progress"));
        assert!(ignore_logind_idle_hold("Grok", "anything"));
        assert!(ignore_logind_idle_hold(
            "grok (block)",
            "agent turn in progress"
        ));
        assert!(ignore_logind_idle_hold("grok(block)", "idle"));
    }

    #[test]
    fn ignores_agent_turn_why_even_if_who_unknown() {
        assert!(ignore_logind_idle_hold(
            "some-agent",
            "Agent turn in progress"
        ));
        assert!(ignore_logind_idle_hold("tool", "agent turn"));
    }

    #[test]
    fn keeps_real_media_and_fullscreen_holds() {
        assert!(!ignore_logind_idle_hold("vlc", "playing video"));
        assert!(!ignore_logind_idle_hold("firefox", "fullscreen"));
        assert!(!ignore_logind_idle_hold(
            "org.gnome.Shell.fullscreen",
            "application is fullscreen"
        ));
        assert!(!ignore_logind_idle_hold("steam", "game running"));
        // grok sleep delay is not idle-what — who is still grok though:
        // who-based filter still drops it (intentional: agent session).
        assert!(ignore_logind_idle_hold(
            "grok",
            "Pause token refresh across sleep"
        ));
    }

    #[test]
    fn does_not_ignore_empty_who_with_unrelated_why() {
        assert!(!ignore_logind_idle_hold("", "playing video"));
        assert!(!ignore_logind_idle_hold("mpv", "idle inhibit for playback"));
    }
}
