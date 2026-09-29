//! Cross-platform screensaver runtime host.
//! Vendored from `runner::idle_runner`.

use crate::core::screensaver::Screensaver;
use std::sync::atomic::{AtomicBool, Ordering};

#[path = "args.rs"]
mod args;
#[path = "idle_runner_fullscreen.rs"]
mod idle_runner_fullscreen;
#[path = "platform_helpers.rs"]
mod platform_helpers;
#[path = "renderer.rs"]
mod renderer;
#[path = "terminal_guard.rs"]
mod terminal_guard;

pub use args::{Mode, parse_args, print_usage};

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_signal(_sig: libc::c_int) {
    SHUTDOWN.store(true, Ordering::Relaxed);
}

fn init_tracing() {
    idle_log::init("info");
}

/// Run the screensaver with the given effect.
pub fn run_main<S: Screensaver + 'static>(mut saver: S, name: &str) {
    init_tracing();
    let mode = parse_args();
    match mode {
        Mode::Run => {
            let code = match run_fullscreen(&mut saver) {
                Ok(()) => 0,
                Err(_) => 1,
            };
            std::process::exit(code as i32);
        }
        Mode::Configure => {
            idle_log::warn!("({name}) configuration dialog: not yet implemented.");
            std::process::exit(0);
        }
        Mode::Preview => {
            #[cfg(target_os = "windows")]
            {
                let code = run_preview_stub(&mut saver);
                std::process::exit(code as i32);
            }
            #[cfg(not(target_os = "windows"))]
            {
                let code = match run_fullscreen(&mut saver) {
                    Ok(()) => 0,
                    Err(_) => 1,
                };
                std::process::exit(code as i32);
            }
        }
        Mode::ShowUsage => {
            print_usage(name);
            std::process::exit(0);
        }
    }
}

#[cfg(target_os = "windows")]
fn run_preview_stub(_saver: &mut dyn Screensaver) -> isize {
    idle_log::warn!("Windows preview mode is not supported in console mode.");
    0
}

/// Loads a screensaver plugin dynamic library and runs it fullscreen.
///
/// Routed through [`crate::plugin_session::PluginSession::load_path_with_options`]
/// so the **manifest gate** applies to the `idle-daemon run-plugin <saver>` CLI
/// subcommand, the TUI preview fallback, and the COSMIC preview fallback (all
/// three of which call this function directly). The loader runs, in order:
///
/// 1. read the sibling `.idleplugin.toml` manifest,
/// 2. validate it (`schema_version`, `plugin_id`, `[entry]`, …),
/// 3. capability-check it (refuse unmediated network/audio unless opted in),
/// 4. enforce Landlock using the manifest's `sandbox.profile` + capability
///    trees (falling back to the `minimal` profile when no manifest is
///    present),
/// 5. `dlopen` the `.so` (ELF constructors only fire once the sandbox is
///    already on, so plugin code cannot escape it),
/// 6. ABI-version-negotiate,
/// 7. assert `manifest.entry.library` matches the resolved path, and
/// 8. resolve the `create_screensaver` / `destroy_screensaver` symbols and
///    instantiate the screensaver.
///
/// A bare `.so` with no manifest is refused by default. The operator escape
/// hatch is `IDLE_ALLOW_UNSIGNED_PLUGINS=1` (see
/// `idle_api::plugin_manifest::host::ALLOW_UNSIGNED_ENV`). Every gate is
/// fail-closed: any error from the loader propagates here and the plugin is
/// not run.
pub fn run_plugin_fullscreen(plugin_path: &str) -> Result<isize, Box<dyn std::error::Error>> {
    // Manifest gate entry point: see the doc comment above. Routing through
    // `PluginSession` here closes the wave-3 reviewer hole where `run-plugin`
    // paths bypassed the gate that the IPC child path already enforced.
    let path = std::path::Path::new(plugin_path);
    let mut session = crate::plugin_session::PluginSession::load_path_with_options(path, None)
        .map_err(|e| -> Box<dyn std::error::Error> { e.to_string().into() })?;

    // `load_path_with_options` populates `session.plugin = Some(guard)` on the
    // `Ok` arm; the `Option` only exists for the hot-reload swap in
    // `PluginSession::reload`. Treat the (unreachable) `None` arm as a
    // load failure rather than panicking, to keep this fail-closed.
    let Some(guard) = session.plugin.as_mut() else {
        return Err("plugin loader returned Ok but no plugin guard".into());
    };
    let exit_code = match run_fullscreen(guard.saver_mut()) {
        Ok(()) => 0,
        Err(_) => 1,
    };
    Ok(exit_code)
}

// ---------------------------------------------------------------------------
// Common Fullscreen Animation Loop
// ---------------------------------------------------------------------------

fn run_fullscreen(saver: &mut dyn Screensaver) -> Result<(), Box<dyn std::error::Error>> {
    let terminal = idle_runner_fullscreen::setup_terminal()?;
    let result = idle_runner_fullscreen::drive_plugin_loop(saver);
    idle_runner_fullscreen::teardown_terminal(terminal);
    result
}
