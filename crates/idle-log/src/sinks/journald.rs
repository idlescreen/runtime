// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Systemd journal datagram socket sink (`/run/systemd/journal/socket`).

use std::os::unix::net::UnixDatagram;
use std::sync::OnceLock;

static SOCK: OnceLock<Option<UnixDatagram>> = OnceLock::new();

fn socket() -> Option<&'static UnixDatagram> {
    SOCK.get_or_init(|| UnixDatagram::unbound().ok()).as_ref()
}

pub fn send(priority: u8, ident: &str, msg: &str) {
    let Some(sock) = socket() else {
        return;
    };
    let payload = format!(
        "PRIORITY={priority}\nMESSAGE={}\nSYSLOG_IDENTIFIER={ident}\n",
        msg.replace('\n', " ")
    );
    let _ = sock.send_to(payload.as_bytes(), "/run/systemd/journal/socket");
}
