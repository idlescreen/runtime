// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! RFC 3164 Syslog datagram sink (`/dev/log`).

use std::os::unix::net::UnixDatagram;
use std::sync::OnceLock;

static SOCK: OnceLock<Option<UnixDatagram>> = OnceLock::new();

fn socket() -> Option<&'static UnixDatagram> {
    SOCK.get_or_init(|| UnixDatagram::unbound().ok()).as_ref()
}

/// Calculate RFC 3164 PRI value: facility * 8 + severity.
#[inline]
pub fn rfc3164_pri(facility: u8, severity: u8) -> u8 {
    facility.saturating_mul(8).saturating_add(severity)
}

/// Format an RFC 3164 packet: `<PRI>tag[pid]: message`.
pub fn format_rfc3164(pri: u8, tag: &str, pid: u32, msg: &str) -> String {
    format!("<{pri}>{tag}[{pid}]: {}\n", msg.replace('\n', " "))
}

/// Send message to `/dev/log`.
pub fn send(pri: u8, tag: &str, msg: &str) {
    let Some(sock) = socket() else {
        return;
    };
    let pid = std::process::id();
    let packet = format_rfc3164(pri, tag, pid, msg);
    let _ = sock.send_to(packet.as_bytes(), "/dev/log");
}
