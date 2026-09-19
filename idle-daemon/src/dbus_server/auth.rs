// SPDX-License-Identifier: Apache-2.0

//! Authorization for D-Bus control methods.

#[path = "auth_peer.rs"]
mod auth_peer;

use auth_peer::{PeerExeCheck, check_peer_exe, comm_matches_trusted, our_euid, peer_comm};

use zbus::Connection;
use zbus::message::Header;

/// Require peer Unix UID to match our euid (session-bus same-user defense in depth).
fn peer_uid_matches_ours(peer_uid: Option<u32>) -> bool {
    let Some(our_uid) = our_euid() else {
        return peer_uid.is_none();
    };
    match peer_uid {
        Some(uid) if uid == our_uid => true,
        Some(uid) => {
            idle_log::warn!("D-Bus auth: peer uid {uid} != our uid {our_uid}; denying");
            false
        }
        None => {
            idle_log::warn!("D-Bus auth: peer UID unavailable; denying");
            false
        }
    }
}

fn dbus_trust_all_enabled() -> bool {
    if !cfg!(debug_assertions) {
        return false;
    }
    std::env::var("IDLE_DBUS_TRUST_ALL").ok().as_deref() == Some("1")
}

/// Strict control: no `/proc/pid/comm` fallback when exe is unreadable.
///
/// Enabled by `IDLE_STRICT_CONTROL=1` (process env) — operators who refuse
/// the documented same-UID `prctl` residual should set this (or
/// `strict_control: true` in config.yaml once the daemon reloads env via unit).
pub(crate) fn strict_control_enabled() -> bool {
    matches!(
        std::env::var("IDLE_STRICT_CONTROL").ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes")
    )
}

fn is_trusted_control_peer(pid: u32, peer_uid: Option<u32>, peer_name: &str) -> bool {
    // Escape hatch is debug-only so release builds cannot be opened with
    // `IDLE_DBUS_TRUST_ALL=1` by a local attacker.
    if dbus_trust_all_enabled() {
        idle_log::warn!("D-Bus auth: IDLE_DBUS_TRUST_ALL=1 (debug build only)");
        return true;
    }

    // Always require same-UID before trusting path or comm (closes cross-user
    // edge cases if the service is ever bound on a broader bus).
    if !peer_uid_matches_ours(peer_uid) {
        return false;
    }

    match check_peer_exe(pid) {
        PeerExeCheck::Trusted => {
            // Narrow TOCTOU window: re-check exe still resolves to a trusted peer.
            match check_peer_exe(pid) {
                PeerExeCheck::Trusted => true,
                other => {
                    idle_log::warn!(
                        "D-Bus auth: peer {peer_name} (pid {pid}) failed re-check after Trusted ({other:?})"
                    );
                    false
                }
            }
        }
        PeerExeCheck::Untrusted => false,
        PeerExeCheck::Unreadable => {
            // Strict mode: refuse comm fallback (closes prctl spoof residual).
            if strict_control_enabled() {
                idle_log::warn!(
                    "D-Bus auth: peer {peer_name} (pid {pid}) denied — exe unreadable and IDLE_STRICT_CONTROL is set (no comm fallback)"
                );
                return false;
            }
            // Do **not** accept pure same-UID: any compromised same-user process
            // could otherwise call control methods. Prefer `/proc/pid/comm`, which
            // remains readable under typical Yama/systemd hardening when `exe` is not.
            match peer_comm(pid) {
                Some(comm) if comm_matches_trusted(&comm) => {
                    // Same-UID + comm is a known residual (prctl spoof). Surface at
                    // WARN so journal reviews can detect non-exe trust accepts.
                    idle_log::warn!(
                        "D-Bus auth: peer {peer_name} (pid {pid}, comm {comm}) accepted via same-UID + trusted comm (exe unreadable; spoof residual — see docs/BOUNDARIES.md or IDLE_STRICT_CONTROL=1)"
                    );
                    true
                }
                Some(comm) => {
                    idle_log::warn!(
                        "D-Bus auth: peer pid {pid} comm {comm:?} not trusted; denying (exe unreadable)"
                    );
                    false
                }
                None => {
                    idle_log::warn!("D-Bus auth: peer pid {pid} exe and comm unreadable; denying");
                    false
                }
            }
        }
    }
}

/// Control methods (preview, config writes) require idle CLI, TUI, or applet.
pub async fn require_control_peer(
    connection: &Connection,
    header: &Header<'_>,
) -> zbus::fdo::Result<()> {
    if dbus_trust_all_enabled() {
        return Ok(());
    }

    let sender = header.sender().ok_or_else(|| {
        zbus::fdo::Error::AccessDenied("control request missing D-Bus sender".into())
    })?;

    let dbus = zbus::fdo::DBusProxy::new(connection)
        .await
        .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
    let creds = dbus
        .get_connection_credentials((*sender).clone().into())
        .await
        .map_err(|_| zbus::fdo::Error::AccessDenied("cannot verify D-Bus peer".into()))?;
    let pid = creds
        .process_id()
        .ok_or_else(|| zbus::fdo::Error::AccessDenied("D-Bus peer PID unavailable".into()))?;
    let peer_uid = creds.unix_user_id();

    if is_trusted_control_peer(pid, peer_uid, sender.as_str()) {
        idle_log::info!("D-Bus control peer accepted (pid {pid})");
        Ok(())
    } else {
        idle_log::info!("D-Bus control peer rejected (pid {pid})");
        Err(zbus::fdo::Error::AccessDenied(
            "control methods require idle CLI, TUI, or panel applet".into(),
        ))
    }
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
