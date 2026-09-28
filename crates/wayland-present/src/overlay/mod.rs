// SPDX-License-Identifier: MIT

//! Layer-shell overlay thread: binds outputs, manages SHM buffers, routes input.
//!
//! [`OverlayPresenter`](crate::OverlayPresenter) communicates with the event thread
//! through [`PresenterCommand`] messages. [`state::SessionState`] owns all Wayland
//! objects and is updated from the handler modules under [`handlers`].
//!
//! Frame submission uses double-buffered SHM pools in [`buffer`]; configure events
//! from the compositor resize overlays and refresh [`crate::output::OutputLayout`].
//!
//! User pointer and keyboard events dismiss the overlay after a short grace period
//! so accidental motion during fade-in does not immediately hide the screensaver.

pub(crate) mod buffer;
pub(crate) mod command;
pub(crate) mod epoll;
pub(crate) mod error_utils;
pub(crate) mod event_thread;
pub(crate) mod handlers;
mod state;

pub use command::PresenterCommand;
pub use event_thread::spawn_event_thread;

// Solid-color previews and screensaver frames share the same overlay map.
// Configure events may arrive before the first frame submission.
// Output removal destroys layer surfaces and registry entries together.
// Thread startup is triggered from presenter.rs when OverlayPresenter is created.
//
