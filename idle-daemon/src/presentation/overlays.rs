// SPDX-License-Identifier: MIT

use idle_api::with_caption;
use idle_runner::toolkit::theme_query;
use idle_runner::{caption_overlay, fps_overlay};
use std::fmt::Write;

pub fn maybe_draw_overlays(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    is_primary: bool,
    show_fps: bool,
    achieved_fps: f32,
) {
    if !is_primary {
        return;
    }

    with_caption(|caption| {
        if !caption.is_empty() {
            caption_overlay::draw_bottom_center(pixels, width, height, caption, (245, 240, 200));
        }
    });

    if show_fps {
        // Small stack-backed string avoids heap format! per frame.
        let mut label = String::with_capacity(16);
        let _ = write!(&mut label, "FPS {achieved_fps:.1}");
        let (accent, _) = theme_query::load_global_theme();
        let color = accent.unwrap_or((0, 191, 255));
        fps_overlay::draw_top_right(pixels, width, height, &label, color);
    }
}
