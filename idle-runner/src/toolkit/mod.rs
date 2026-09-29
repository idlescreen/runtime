//! Toolkit module: platform-specific helpers. Vendored from `runner::toolkit`.
//! Slim version: only the queries that screensaver-security actually uses are
//! preserved. The full library has many more (monitors, GPU, IPC, eBPF,
//! registry, etc.) that this project does not touch.
//!
//! ## Submodules
//!
//! - [`platform`] — portable structs for disks, power, and BIOS metadata
//! - [`sys_info`] — live host queries with short-lived caches
//! - [`theme_query`] — COSMIC / GTK accent and dark-mode discovery
//!
//! Screensaver plugins receive populated [`idle_api::SYSTEM_INFO_CALLBACK`] and
//! [`idle_api::PALETTE_CALLBACK`] hooks from the daemon; this module implements
//! the Linux-side data sources those callbacks read. Monitor layout helpers in
//! [`sys_info`] also back [`idle_api::MONITOR_BOUNDS_CALLBACK`] during span mode.
//!
//! Theme data is best-effort: when COSMIC settings are unavailable, GTK settings
//! and sane defaults are used before plugins query [`query_current_palette`].

pub mod platform;
pub mod sys_info;
pub mod theme_query;

// Linux-specific procfs and xrandr helpers live under sys_info submodules.
// Non-Linux builds return conservative defaults from platform.rs.
// Caches in sys_info expire after a few seconds to limit procfs churn.
