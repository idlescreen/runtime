// SPDX-License-Identifier: MIT

use std::thread;
use std::time::{Duration, Instant};

use idle_api::{OutputLayout, OverlaySurface};

/// Presentation FPS target refresh rate.
///
/// Multi-monitor span uses the **primary** display refresh (e.g. 144 Hz on display 1),
/// not the lowest. One frame loop drives all outputs; the secondary (e.g. 60 Hz) may
/// skip or hold frames, which is fine for spillover content. Physics tick rate stays
/// independent (`IDLE_TICK_HZ`, default 60).
///
/// Override sync policy with `IDLE_PRESENT_SYNC=min|primary|max` (default: primary).
pub fn presentation_refresh_hz(layouts: &[OutputLayout], primary: OutputLayout) -> u32 {
    if layouts.len() <= 1 {
        return layouts
            .first()
            .map(|layout| layout.refresh_mhz)
            .unwrap_or(60)
            .max(60);
    }

    let min_hz = layouts
        .iter()
        .map(|layout| layout.refresh_mhz)
        .min()
        .unwrap_or(60)
        .max(60);
    let max_hz = layouts
        .iter()
        .map(|layout| layout.refresh_mhz)
        .max()
        .unwrap_or(60)
        .max(60);
    let primary_hz = primary.refresh_mhz.max(60);

    match idle_api::env_var_first(&["IDLE_PRESENT_SYNC"]).as_deref() {
        Some("min") => min_hz,
        Some("max") => max_hz,
        _ => primary_hz,
    }
}

pub fn wait_for_output_layouts(
    presenter: &dyn OverlaySurface,
    timeout: Duration,
) -> Result<Vec<OutputLayout>, String> {
    let deadline = Instant::now() + timeout;
    let mut best = Vec::new();
    let mut layouts_seen_at = None::<Instant>;

    while Instant::now() < deadline {
        let layouts = presenter.output_layouts();
        if !layouts.is_empty() {
            best = layouts;
            layouts_seen_at.get_or_insert_with(Instant::now);
            if layouts_seen_at.is_some_and(|seen| seen.elapsed() >= Duration::from_millis(500)) {
                return Ok(best);
            }
        }
        thread::sleep(Duration::from_millis(50));
    }

    if best.is_empty() {
        best = presenter.output_layouts();
    }
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(id: u32, hz: u32) -> OutputLayout {
        OutputLayout {
            id,
            width: 1920,
            height: 1080,
            refresh_mhz: hz,
            x: 0,
            y: 0,
            scale: 1,
        }
    }

    #[test]
    fn single_output_uses_own_refresh_floored_at_60() {
        let layouts = vec![layout(1, 144)];
        assert_eq!(presentation_refresh_hz(&layouts, layouts[0]), 144);
        let low = vec![layout(1, 30)];
        assert_eq!(presentation_refresh_hz(&low, low[0]), 60);
    }

    #[test]
    fn multi_output_sync_policy_min_max_primary() {
        let _g = crate::TEST_ENV_LOCK.lock().unwrap();
        let layouts = vec![layout(1, 60), layout(2, 144)];
        let primary = layouts[1];
        let prior = std::env::var("IDLE_PRESENT_SYNC").ok();

        unsafe {
            std::env::set_var("IDLE_PRESENT_SYNC", "min");
        }
        assert_eq!(presentation_refresh_hz(&layouts, primary), 60);

        unsafe {
            std::env::set_var("IDLE_PRESENT_SYNC", "max");
        }
        assert_eq!(presentation_refresh_hz(&layouts, primary), 144);

        unsafe {
            std::env::set_var("IDLE_PRESENT_SYNC", "primary");
        }
        assert_eq!(presentation_refresh_hz(&layouts, primary), 144);

        // Unknown / empty → primary path.
        unsafe {
            std::env::set_var("IDLE_PRESENT_SYNC", "other");
        }
        assert_eq!(presentation_refresh_hz(&layouts, layouts[0]), 60);

        match prior {
            Some(v) => unsafe {
                std::env::set_var("IDLE_PRESENT_SYNC", v);
            },
            None => unsafe {
                std::env::remove_var("IDLE_PRESENT_SYNC");
            },
        }
    }

    #[test]
    fn empty_layouts_default_to_60() {
        assert_eq!(presentation_refresh_hz(&[], layout(0, 0)), 60);
    }
}
