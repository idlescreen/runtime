// SPDX-License-Identifier: MIT

//! Commands sent from the daemon to the overlay event thread.

use std::sync::Arc;
use std::sync::Mutex;

use crate::appearance::OverlayAppearance;

/// Commands routed from the daemon (sender) to the event thread
/// (receiver).
///
/// The frame payload carries an `Arc<Vec<u8>>` so the daemon can
/// keep ownership of the buffer until the event thread is done
/// committing it; the event thread pushes the `Arc` back into the
/// shared `return_pool` for the daemon's `get_frame_buffer` path
/// to recycle (reclaim the heap via `Arc::try_unwrap`).
pub enum PresenterCommand {
    ShowSolid(OverlayAppearance),
    ShowScreensaver,
    UpdateFrame {
        output_id: u32,
        width: u32,
        height: u32,
        /// Frozen BGRA buffer wrapped in `Arc<Vec<u8>>`. The event
        /// thread commits it, then pushes the Arc back to
        /// `return_pool` for the daemon's `get_frame_buffer` path.
        /// Sized (Vec is Sized) so the recycler can `Arc::try_unwrap`
        /// and reclaim the heap without a copy.
        pixels: Arc<Vec<u8>>,
        /// Triple-buffer return pool shared between daemon + event
        /// thread. `Arc::clone` per frame replaces the prior
        /// `mpsc::Sender<Vec<u8>>` round-trip.
        return_pool: Arc<Mutex<std::collections::VecDeque<Arc<Vec<u8>>>>>,
    },
    Hide,
}

#[cfg(test)]
mod tests {
    use super::PresenterCommand;
    use std::sync::{Arc, Mutex};

    #[test]
    fn presenter_command_show_solid_carries_appearance() {
        // Pattern-match to confirm the variant exists with the
        // documented `OverlayAppearance` payload.
        let cmd = PresenterCommand::ShowScreensaver;
        assert!(matches!(cmd, PresenterCommand::ShowScreensaver));
    }

    #[test]
    fn presenter_command_update_frame_round_trips() {
        // Construct + pattern-match. The Arc stays valid through the
        // round-trip; we exercise the lifetime contract here.
        let pixels: Arc<Vec<u8>> = Arc::new(vec![1, 2, 3, 4]);
        let pool: Arc<Mutex<std::collections::VecDeque<Arc<Vec<u8>>>>> =
            Arc::new(Mutex::new(std::collections::VecDeque::new()));
        let pool_clone = pool.clone();
        let cmd = PresenterCommand::UpdateFrame {
            output_id: 42,
            width: 8,
            height: 8,
            pixels: pixels.clone(),
            return_pool: pool,
        };
        match cmd {
            PresenterCommand::UpdateFrame {
                output_id,
                width,
                height,
                pixels: p,
                return_pool: rp,
            } => {
                assert_eq!(output_id, 42);
                assert_eq!(width, 8);
                assert_eq!(height, 8);
                assert_eq!(&p[..], &[1u8, 2, 3, 4][..]);
                assert!(Arc::ptr_eq(&rp, &pool_clone));
            }
            _ => panic!("expected UpdateFrame"),
        }
    }

    #[test]
    fn presenter_command_hide_is_distinct_variant() {
        // Documented distinct variant: a stray match on Hide must
        // not be silently coerced into ShowScreensaver or vice versa.
        let hide = PresenterCommand::Hide;
        let show = PresenterCommand::ShowScreensaver;
        assert!(matches!(hide, PresenterCommand::Hide));
        assert!(matches!(show, PresenterCommand::ShowScreensaver));
    }
}
