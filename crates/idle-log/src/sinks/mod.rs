// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Logging sinks: journald datagram socket, RFC 3164 syslog, rolling state file.

pub mod file;
pub mod journald;
pub mod syslog;
