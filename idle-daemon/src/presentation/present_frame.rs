// SPDX-License-Identifier: MIT

//! Main frame-presentation entry point.
//!
//! `present_frame` runs once per OODA tick: walks the
//! [`FrameLoopState`] sessions + layouts, draws each session into a
//! recycled BGRA buffer, applies the fade-in, and submits to the
//! Wayland presenter. The fade-in is a separate page in
//! `apply_fade_in.rs` so each one-fn-per-page rule stays clean.

use std::sync::Arc;
use std::time::Duration;

use idle_api::OutputId;

use super::apply_fade_in::apply_fade_in;
use super::frame_loop::FrameLoopState;
use super::layout::{monitor_cell_bounds, virtual_desktop};
use super::overlays::maybe_draw_overlays;

/// Main frame-presentation entry. See module docs.
pub fn present_frame(state: &mut FrameLoopState) {
    let (min_x, min_y, total_w, total_h) = virtual_desktop(state.layouts);

    if state.independent_rendering {
        for s in state.sessions.iter_mut() {
            let (scanlines, dirty) = s.session.draw_frame(s.cols, s.rows);
            if !dirty
                && state.frame_start.duration_since(state.session_start)
                    >= Duration::from_millis(500)
            {
                continue;
            }
            if let Some(layout) = state.layouts.iter().find(|l| l.id == s.output_id) {
                let target_w = if state.use_hw_scaling {
                    s.session.content_width(s.cols)
                } else {
                    layout.width
                };
                let target_h = if state.use_hw_scaling {
                    s.session.content_height(s.rows)
                } else {
                    layout.height
                };

                let mut pixels = state
                    .presenter
                    .get_frame_buffer((target_w * target_h * 4) as usize);
                s.session.raster_viewport(
                    0,
                    0,
                    s.cols,
                    s.rows,
                    s.cols,
                    s.rows,
                    target_w,
                    target_h,
                    scanlines,
                    &mut pixels,
                );
                apply_fade_in(
                    &mut pixels,
                    state.frame_start.duration_since(state.session_start),
                );
                maybe_draw_overlays(
                    &mut pixels,
                    target_w,
                    target_h,
                    layout.id == state.primary.id,
                    state.options.show_fps_overlay,
                    state.achieved_fps,
                );
                state.presenter.submit_frame(
                    OutputId(layout.id),
                    Arc::new(pixels),
                    target_w,
                    target_h,
                );
            }
        }
    } else {
        if state.sessions.is_empty() {
            return;
        }
        let s = &mut state.sessions[0];
        let (scanlines, dirty) = s.session.draw_frame(s.cols, s.rows);
        if !dirty
            && state.frame_start.duration_since(state.session_start) >= Duration::from_millis(500)
        {
            return;
        }
        for layout in state.layouts {
            let bounds = monitor_cell_bounds(
                *layout,
                min_x,
                min_y,
                total_w,
                total_h,
                s.cols,
                s.rows,
                layout.id == state.primary.id,
            );
            let col_w = bounds.end_col.saturating_sub(bounds.start_col).max(1);
            let row_h = bounds.end_row.saturating_sub(bounds.start_row).max(1);

            let (target_w, target_h) = if state.use_hw_scaling {
                (
                    s.session.content_width(col_w),
                    s.session.content_height(row_h),
                )
            } else {
                (layout.width, layout.height)
            };

            let mut pixels = state
                .presenter
                .get_frame_buffer((target_w * target_h * 4) as usize);
            s.session.raster_viewport(
                bounds.start_col,
                bounds.start_row,
                col_w,
                row_h,
                s.cols,
                s.rows,
                target_w,
                target_h,
                scanlines,
                &mut pixels,
            );
            apply_fade_in(
                &mut pixels,
                state.frame_start.duration_since(state.session_start),
            );
            maybe_draw_overlays(
                &mut pixels,
                target_w,
                target_h,
                layout.id == state.primary.id,
                state.options.show_fps_overlay,
                state.achieved_fps,
            );
            state
                .presenter
                .submit_frame(OutputId(layout.id), Arc::new(pixels), target_w, target_h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::present_frame;

    #[test]
    fn present_frame_compiles_with_current_state() {
        // Type-only smoke test: present_frame's signature is what
        // frame_loop.rs calls. If the type of `FrameLoopState` ever
        // drifts (e.g. an extra required field), this compile-time
        // check fires before the runtime frame loop tries to.
        fn _takes_present_frame(s: &mut super::super::frame_loop::FrameLoopState) {
            present_frame(s);
        }
    }
}
