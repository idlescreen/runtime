// SPDX-License-Identifier: MIT

//! Wayland protocol event handlers for overlay [`SessionState`](super::state::SessionState).
//!
//! Handlers are split by object family:
//! - [`buffer_objects`] — compositor, SHM, buffers, surfaces
//! - [`registry`] — global registry and output geometry
//! - [`input`] — seat, pointer, and keyboard dismiss paths
//! - [`layer_shell`] — zwlr_layer_surface_v1 configure events
//!
//! Each submodule only contains `Dispatch` implementations; shared mutation
//! logic lives on [`SessionState`](super::state::SessionState) itself.
//!
//! Registry handlers populate [`OutputLayout`](crate::OutputLayout) entries as
//! outputs report geometry and refresh rates during presentation.

mod buffer_objects;
mod input;
mod layer_shell;
mod registry;

// Empty Dispatch stubs remain for protocol objects we bind but do not handle.
// Layer-shell configure drives buffer allocation in state/overlay.rs.
// Pointer motion dismisses after the post-show grace window elapses.
// Keyboard events use the same dismiss path as pointer buttons.
//
