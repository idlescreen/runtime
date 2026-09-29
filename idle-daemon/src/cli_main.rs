// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Shared binary entrypoint for the `idle-daemon` and `idlescreen-daemon` bins.
//! Both names exec the same dispatch; keeping one body prevents drift.

use crate::{daemon, ipc_runner};

/// argv must contain: prog, "run-ipc-runner", saver, socket, shm, cols, rows, scale.
/// Keep in sync with the spawn site in `presentation::ipc_init`.
const IPC_RUNNER_MIN_ARGC: usize = 8;

/// Process entry: tracing setup, plugin callbacks, subcommand dispatch.
pub fn run() -> idle_err::Result<()> {
    // Mark multi-monitor span presentation for plugins/layout helpers.
    idle_api::set_env("IDLE_SPAN_MODE", "1");

    // RUST_LOG-filtered stderr logging; under systemd also mirror to journald.
    idle_log::init("info");
    if std::env::var("JOURNAL_STREAM").is_ok() {
        idle_log::enable_journald("idle-daemon");
    }

    // Register visual theme and system query callbacks for dynamically loaded screensaver plugins
    let _ = idle_api::SYSTEM_INFO_CALLBACK.set(idle_runner::toolkit::sys_info::get_system_info);
    let _ = idle_api::PALETTE_CALLBACK.set(idle_runner::toolkit::sys_info::query_current_palette);
    let _ = idle_api::MONITOR_BOUNDS_CALLBACK
        .set(idle_runner::toolkit::sys_info::get_primary_monitor_bounds);
    let _ = idle_api::IS_SECONDARY_MONITOR_CALLBACK
        .set(idle_runner::toolkit::sys_info::is_secondary_monitor);

    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 {
        match args[1].as_str() {
            "run-plugin" => run_plugin_subcmd(&args),
            "run-ipc-runner" => run_ipc_runner_subcmd(&args),
            "daemon" | "--daemon" => daemon::run_daemon(),
            "--help" | "-h" => {
                println!(
                    "idle-daemon — background idle monitoring service for IdleScreen

usage:
  idle-daemon                     run the background idle daemon (default)
  idle-daemon daemon | --daemon   run the background idle daemon
  idle-daemon run-plugin <saver>  run a trusted screensaver plugin fullscreen
  idle-daemon --help | -h         show this help message"
                );
                Ok(())
            }
            other => idle_err::bail!("unknown argument: {}\ntry --help", other),
        }
    } else {
        // Run the daemon by default
        daemon::run_daemon()
    }
}

fn run_plugin_subcmd(args: &[String]) -> idle_err::Result<()> {
    idle_err::ensure!(
        args.len() >= 3,
        "missing saver name.\nusage: idle-daemon run-plugin <saver>"
    );
    let name = &args[2];
    idle_err::ensure!(
        !name.contains('/') && !name.contains('\\'),
        "saver name must not be a path"
    );
    let path = idle_runner::launcher::resolve_saver_binary(
        name,
        &idle_runner::launcher::LaunchMode::Preview,
    )?;
    // run_plugin_fullscreen replaces this process image with the plugin;
    // exit the host with the plugin's status code on return.
    let code = idle_runner::idle_runner::run_plugin_fullscreen(path.to_string_lossy().as_ref())
        .map_err(|e| idle_err::anyhow!("{e}"))?;
    std::process::exit(code as i32);
}

fn run_ipc_runner_subcmd(args: &[String]) -> idle_err::Result<()> {
    // IPC children must never inherit ambient sandbox/dev escapes from the session.
    idle_runner::sandbox::clear_sandbox_escape_env();
    idle_err::ensure!(
        args.len() >= IPC_RUNNER_MIN_ARGC,
        "missing arguments.\nusage: idle-daemon run-ipc-runner <saver> <socket_path> <shm_name> <cols> <rows> <render_scale>"
    );
    let saver = &args[2];
    let socket_path = &args[3];
    let shm_name = &args[4];
    let cols: usize = args[5].parse().unwrap_or(80);
    let rows: usize = args[6].parse().unwrap_or(24);
    let render_scale: Option<f32> = if args[7] == "none" {
        None
    } else {
        args[7].parse().ok()
    };
    ipc_runner::run_ipc_runner(saver, socket_path, shm_name, cols, rows, render_scale)
        .map_err(|e| idle_err::anyhow!("{e}"))
}

#[cfg(test)]
mod tests {
    use super::IPC_RUNNER_MIN_ARGC;

    /// The spawn site in presentation::ipc_init passes exactly
    /// saver/socket/shm/cols/rows/scale after the subcommand name — argv len 8.
    /// If params are added or removed, this contract must move with them.
    #[test]
    fn ipc_runner_argc_matches_spawn_contract() {
        let argv = [
            "prog",
            "run-ipc-runner",
            "beams",
            "/tmp/idle-uds-1-0.sock",
            "/idle-shm-1-0",
            "80",
            "24",
            "1.000000",
        ];
        assert!(argv.len() >= IPC_RUNNER_MIN_ARGC);
    }
}
