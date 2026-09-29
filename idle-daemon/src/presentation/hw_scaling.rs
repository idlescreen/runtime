// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Policy for optional Wayland `wp_viewporter` hardware scaling.
//!
//! Pure helpers — unit tested. On COSMIC, enabling viewporter during preview
//! historically disconnected the Wayland client and (before 2.5.5) killed the
//! daemon process.

/// Env var that must be set to allow viewporter destination scaling.
pub const HW_VIEWPORT_ENV: &str = "IDLE_HW_VIEWPORT";

/// Whether frame loop should use compositor viewporter scaling.
///
/// Default is **off** for stability. Requires opt-in env + compositor support
/// + software (non-GPU) upscale path.
pub fn should_use_hw_viewport(
    env_force: bool,
    compositor_supports_viewporter: bool,
    using_gpu_upscale: bool,
) -> bool {
    env_force && compositor_supports_viewporter && !using_gpu_upscale
}

/// Read force flag from process environment.
pub fn hw_viewport_env_force() -> bool {
    std::env::var_os(HW_VIEWPORT_ENV).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_off_even_when_supported() {
        // Regression: viewporter was on by default and crashed preview on COSMIC.
        assert!(!should_use_hw_viewport(false, true, false));
    }

    #[test]
    fn on_when_env_and_supported_and_cpu_upscale() {
        assert!(should_use_hw_viewport(true, true, false));
    }

    #[test]
    fn off_when_env_but_no_viewporter() {
        assert!(!should_use_hw_viewport(true, false, false));
    }

    #[test]
    fn off_when_gpu_upscale_active() {
        assert!(!should_use_hw_viewport(true, true, true));
    }

    #[test]
    fn env_name_stable() {
        assert_eq!(HW_VIEWPORT_ENV, "IDLE_HW_VIEWPORT");
    }
}
