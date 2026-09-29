// SPDX-License-Identifier: MIT

use std::sync::OnceLock;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use idle_api::MonitorCellBounds;

type MonitorLayoutCacheEntry = (Vec<MonitorCellBounds>, (usize, usize), Instant);

static MONITOR_LAYOUT_CACHE: OnceLock<RwLock<Option<MonitorLayoutCacheEntry>>> = OnceLock::new();

pub fn get_monitor_layouts(cols: usize, rows: usize) -> Vec<MonitorCellBounds> {
    let cache_rw = MONITOR_LAYOUT_CACHE.get_or_init(|| RwLock::new(None));
    if let Ok(read_guard) = cache_rw.read()
        && let Some((ref layouts, (cached_cols, cached_rows), last_query)) = *read_guard
        && cached_cols == cols
        && cached_rows == rows
        && last_query.elapsed() < Duration::from_secs(5)
    {
        return layouts.clone();
    }
    let mut cache = cache_rw.write().unwrap_or_else(|e| {
        idle_log::error!("mutex poisoned: {e}");
        std::process::abort()
    });
    if let Some((ref layouts, (cached_cols, cached_rows), last_query)) = *cache
        && cached_cols == cols
        && cached_rows == rows
        && last_query.elapsed() < Duration::from_secs(5)
    {
        return layouts.clone();
    }

    let mut computed_layouts = None;
    if let Some(xmonitors) = query_monitors_from_xrandr() {
        let min_x = xmonitors
            .iter()
            .map(|&(_, _, _, x, _)| x)
            .min()
            .unwrap_or(0);
        let max_x = xmonitors
            .iter()
            .map(|&(_, w, _, x, _)| x + w as i32)
            .max()
            .unwrap_or(0);
        let min_y = xmonitors
            .iter()
            .map(|&(_, _, _, _, y)| y)
            .min()
            .unwrap_or(0);
        let max_y = xmonitors
            .iter()
            .map(|&(_, _, h, _, y)| y + h as i32)
            .max()
            .unwrap_or(0);

        let total_width = (max_x - min_x) as usize;
        let total_height = (max_y - min_y) as usize;

        if total_width > 0 && total_height > 0 {
            let mut layouts = Vec::new();
            for (is_primary, w, h, x, y) in xmonitors {
                let rel_x1 = x - min_x;
                let rel_x2 = x + w as i32 - min_x;
                let rel_y1 = y - min_y;
                let rel_y2 = y + h as i32 - min_y;

                let start_col = (rel_x1 as usize * cols) / total_width;
                let end_col = (rel_x2 as usize * cols) / total_width;
                let start_row = (rel_y1 as usize * rows) / total_height;
                let end_row = (rel_y2 as usize * rows) / total_height;

                layouts.push(MonitorCellBounds {
                    start_col: start_col.clamp(0, cols),
                    end_col: end_col.clamp(0, cols),
                    start_row: start_row.clamp(0, rows),
                    end_row: end_row.clamp(0, rows),
                    is_primary,
                });
            }
            computed_layouts = Some(layouts);
        }
    }

    let result = computed_layouts.unwrap_or_else(|| {
        vec![MonitorCellBounds {
            start_col: 0,
            end_col: cols,
            start_row: 0,
            end_row: rows,
            is_primary: true,
        }]
    });

    *cache = Some((result.clone(), (cols, rows), Instant::now()));
    result
}

pub fn get_primary_monitor_bounds(cols: usize, rows: usize) -> MonitorCellBounds {
    if idle_api::MONITOR_BOUNDS_CALLBACK.get().is_some() {
        return idle_api::get_primary_monitor_bounds(cols, rows);
    }

    let layouts = get_monitor_layouts(cols, rows);
    layouts
        .into_iter()
        .find(|l| l.is_primary)
        .unwrap_or(MonitorCellBounds {
            start_col: 0,
            end_col: cols,
            start_row: 0,
            end_row: rows,
            is_primary: true,
        })
}

pub fn is_secondary_monitor() -> bool {
    idle_api::env_is_set(&["IDLE_SECONDARY_MONITOR"])
}

type XrandrMonitorInfo = (bool, u32, u32, i32, i32);

pub fn query_monitors_from_xrandr() -> Option<Vec<XrandrMonitorInfo>> {
    if let Ok(exe) = std::env::current_exe()
        && exe.to_string_lossy().contains("/deps/")
    {
        return None;
    }
    let output = std::process::Command::new("xrandr")
        .arg("--listmonitors")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut monitors = Vec::new();
    for line in stdout.lines() {
        if line.contains("Monitors:") || line.trim().is_empty() {
            continue;
        }
        let is_primary = line.contains('*');
        let mut geometry_token = None;
        for token in line.split_whitespace() {
            if token.contains('x') && token.contains('+') {
                geometry_token = Some(token);
                break;
            }
        }
        if let Some(token) = geometry_token {
            let parts: Vec<&str> = token.split('+').collect();
            if parts.len() >= 3 {
                let size_part = parts[0];
                let Ok(x_offset) = parts[1].parse::<i32>() else {
                    continue;
                };
                let Ok(y_offset) = parts[2].parse::<i32>() else {
                    continue;
                };

                let size_subparts: Vec<&str> = size_part.split('x').collect();
                if size_subparts.len() == 2 {
                    let w_part = size_subparts[0].split('/').next().unwrap_or("0");
                    let h_part = size_subparts[1].split('/').next().unwrap_or("0");
                    let Ok(width) = w_part.parse::<u32>() else {
                        continue;
                    };
                    let Ok(height) = h_part.parse::<u32>() else {
                        continue;
                    };

                    if width > 0 && height > 0 {
                        monitors.push((is_primary, width, height, x_offset, y_offset));
                    }
                }
            }
        }
    }
    if monitors.is_empty() {
        None
    } else {
        Some(monitors)
    }
}
