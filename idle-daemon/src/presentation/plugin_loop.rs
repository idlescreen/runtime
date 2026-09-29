// SPDX-License-Identifier: MIT

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use super::ipc_session::IpcPluginSession;
use idle_api::{OutputLayout, OverlaySurface, clear_caption, clear_primary_bounds};

use super::frame_loop::ActiveSession;
use super::frame_pacing::{FramePacing, log_run_startup};
use super::layout::{
    install_primary_bounds_callback, primary_bounds_in_grid, span_simulation_grid, virtual_desktop,
};
use super::refresh::wait_for_output_layouts;
use crate::presentation::PresentationOptions;

/// Run presentation using **out-of-process** plugin sessions only (crash isolation).
pub fn run_plugin_loop(
    presenter: &dyn OverlaySurface,
    saver_name: &str,
    stop: &AtomicBool,
    options: PresentationOptions,
) -> Result<(), String> {
    presenter.show_screensaver();

    let mut layouts = wait_for_output_layouts(presenter, Duration::from_secs(3))?;
    if layouts.is_empty() {
        return Err("no configured outputs for screensaver presentation".into());
    }

    let topology = super::topology::DisplayTopologyMap::build(&layouts);
    for layout in &mut layouts {
        if let Some(topo) = topology.monitors.iter().find(|m| m.id == layout.id) {
            layout.x = topo.x;
            layout.y = topo.y;
            layout.width = topo.width;
            layout.height = topo.height;
        }
    }
    log_output_layouts(&layouts);

    let mut sessions = build_sessions(saver_name, &layouts, &topology, &options)?;

    let primary = layouts
        .iter()
        .max_by_key(|layout| layout.width.saturating_mul(layout.height))
        .copied()
        .ok_or_else(|| "no primary output found".to_string())?;

    let primary_bounds = if topology.independent_rendering {
        let primary_session = sessions
            .iter()
            .find(|s| s.output_id == primary.id)
            .ok_or_else(|| "primary session missing".to_string())?;
        idle_api::MonitorCellBounds {
            start_col: 0,
            end_col: primary_session.cols,
            start_row: 0,
            end_row: primary_session.rows,
            is_primary: true,
        }
    } else {
        let s = &sessions[0];
        let (min_x, min_y, total_w, total_h) = virtual_desktop(&layouts);
        primary_bounds_in_grid(primary, min_x, min_y, total_w, total_h, s.cols, s.rows)
    };

    let (v_cols, v_rows) = if topology.independent_rendering {
        let primary_session = sessions
            .iter()
            .find(|s| s.output_id == primary.id)
            .ok_or_else(|| "primary session missing".to_string())?;
        (primary_session.cols, primary_session.rows)
    } else {
        (sessions[0].cols, sessions[0].rows)
    };

    install_layout_callbacks(primary_bounds, v_cols, v_rows);

    let pacing = FramePacing::compute(&layouts, primary, &mut sessions);
    log_run_startup(saver_name, &layouts, &pacing, &sessions[0].session);

    let result = pacing.run_loop(
        presenter,
        stop,
        &mut sessions,
        &layouts,
        primary,
        topology.independent_rendering,
        options,
    );

    clear_primary_bounds();
    clear_caption();
    result
}

fn build_sessions(
    saver_name: &str,
    layouts: &[OutputLayout],
    topology: &super::topology::DisplayTopologyMap,
    options: &PresentationOptions,
) -> Result<Vec<ActiveSession>, String> {
    // GPU cell raster only pays for itself on large canvases — CPU raster
    // is ~1.8µs/cell, so a single ~1080p grid (~4k cells) fits the 16.6ms
    // frame budget with room to spare. Past ~2.6MP of output (1440p, or
    // multi-monitor spans/independent sessions) per-frame CPU raster starts
    // crowding the budget, so the wgpu probe's ~400MB transient is worth it.
    let total_px: u64 = layouts
        .iter()
        .map(|l| l.width as u64 * l.height as u64)
        .sum();
    let want_gpu = total_px > 2_600_000;

    let mut sessions = Vec::new();
    if topology.independent_rendering {
        for layout in layouts {
            let mut session = IpcPluginSession::load_with_options(
                saver_name,
                &options.launch_mode,
                options.render_scale,
                options.saver_params.clone(),
                want_gpu,
            )?;
            let (cols, rows) = session.grid_for_pixels(layout.width, layout.height);
            session.init(cols, rows)?;
            sessions.push(ActiveSession {
                output_id: layout.id,
                session,
                cols,
                rows,
            });
        }
    } else {
        let mut session = IpcPluginSession::load_with_options(
            saver_name,
            &options.launch_mode,
            options.render_scale,
            options.saver_params.clone(),
            want_gpu,
        )?;
        let (_min_x, _min_y, total_w, total_h) = virtual_desktop(layouts);
        let (virtual_cols, virtual_rows) = span_simulation_grid(&session, total_w, total_h);
        session.init(virtual_cols, virtual_rows)?;
        sessions.push(ActiveSession {
            output_id: 0,
            session,
            cols: virtual_cols,
            rows: virtual_rows,
        });
    }
    Ok(sessions)
}

fn log_output_layouts(layouts: &[OutputLayout]) {
    for layout in layouts {
        idle_log::info!(
            "output {} @ ({}, {}) — {}x{} @ {} Hz",
            layout.id,
            layout.x,
            layout.y,
            layout.width,
            layout.height,
            layout.refresh_mhz
        );
    }
}

fn install_layout_callbacks(
    primary_bounds: idle_api::MonitorCellBounds,
    virtual_cols: usize,
    virtual_rows: usize,
) {
    idle_api::publish_primary_bounds(primary_bounds);
    install_primary_bounds_callback(primary_bounds, virtual_cols, virtual_rows);
    let _ = idle_api::IS_SECONDARY_MONITOR_CALLBACK.set(|| false);
}
