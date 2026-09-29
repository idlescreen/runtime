// SPDX-License-Identifier: MIT

//! IPC plugin sessions and lifecycle.

pub mod init;
pub mod lifecycle;
pub mod methods;
pub mod peer;
pub mod session;
#[cfg(test)]
mod tests;

pub use init::{SessionInitResult, initialize_ipc_session};
pub use peer::{require_child_peer, runtime_socket_dir};
pub use session::IpcPluginSession;
