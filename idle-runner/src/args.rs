//! Screensaver CLI arg parsing and usage banner.

/// CLI mode detected from the screensaver args.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// `/s` — run the screensaver fullscreen
    Run,
    /// `/c` — open the configuration dialog
    Configure,
    /// `/p <HWND>` — render into the given preview HWND
    Preview,
    /// No args (or `-h` / `--help`) — print usage
    ShowUsage,
}

/// Parse the standard screensaver CLI args. On Windows the full
/// `HWND` is also classified (for `/p:<hwnd>` and `/c:<hwnd>`).
pub fn parse_args() -> Mode {
    parse_args_from(std::env::args().skip(1))
}

/// Arg-iterator form of `parse_args` so tests can pass synthetic argv.
pub fn parse_args_from(args: impl IntoIterator<Item = String>) -> Mode {
    // Filter out our OpenRGB arguments so they don't trigger the usage/error parser
    let filtered_args: Vec<String> = args
        .into_iter()
        .filter(|arg| arg != "--enable-openrgb" && arg != "/rgb")
        .collect();

    if filtered_args.is_empty() {
        return Mode::Run;
    }
    for arg in &filtered_args {
        let lower = arg.to_lowercase();
        if lower == "/s" || lower == "-s" {
            return Mode::Run;
        }
        if lower.starts_with("/p") || lower.starts_with("-p") {
            return Mode::Preview;
        }
        if lower.starts_with("/c") || lower.starts_with("-c") {
            return Mode::Configure;
        }
        if lower == "/?" || lower == "-h" || lower == "--help" {
            return Mode::ShowUsage;
        }
        if arg.starts_with('/') || arg.starts_with('-') {
            return Mode::ShowUsage;
        }
    }
    Mode::ShowUsage
}

/// Print the standard screensaver usage banner for the given name.
pub fn print_usage(name: &str) {
    eprintln!("{name} — retro screensaver (screensaver_runtime)");
    eprintln!();
    eprintln!("Usage (Windows):");
    eprintln!("  {name}.scr /s           — run fullscreen");
    eprintln!("  {name}.scr /c           — open configuration dialog");
    eprintln!("  {name}.scr /p <HWND>    — preview inside the given parent HWND");
    eprintln!();
    eprintln!("Usage (Linux / other):");
    eprintln!("  {name}                  — run fullscreen in the current terminal");
    eprintln!("  {name} -h | --help      — this help");
    eprintln!();
    eprintln!("Options:");
    eprintln!(
        "  --enable-openrgb | /rgb — enable keyboard/device RGB lighting controls via OpenRGB"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(args: &[&str]) -> Mode {
        parse_args_from(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn no_args_runs_fullscreen() {
        assert_eq!(m(&[]), Mode::Run);
    }

    #[test]
    fn run_flags() {
        assert_eq!(m(&["/s"]), Mode::Run);
        assert_eq!(m(&["-s"]), Mode::Run);
        assert_eq!(m(&["/S"]), Mode::Run, "case-insensitive");
    }

    #[test]
    fn preview_flags() {
        assert_eq!(m(&["/p"]), Mode::Preview);
        assert_eq!(m(&["-p"]), Mode::Preview);
        assert_eq!(m(&["/p:1234"]), Mode::Preview, "HWND suffix accepted");
        assert_eq!(m(&["/P", "5678"]), Mode::Preview);
    }

    #[test]
    fn configure_flags() {
        assert_eq!(m(&["/c"]), Mode::Configure);
        assert_eq!(m(&["-c"]), Mode::Configure);
        assert_eq!(m(&["/c:99"]), Mode::Configure);
    }

    #[test]
    fn usage_flags() {
        assert_eq!(m(&["-h"]), Mode::ShowUsage);
        assert_eq!(m(&["--help"]), Mode::ShowUsage);
        assert_eq!(m(&["/?"]), Mode::ShowUsage);
    }

    #[test]
    fn unknown_flag_shows_usage() {
        assert_eq!(m(&["--bogus"]), Mode::ShowUsage);
        assert_eq!(m(&["/xyz"]), Mode::ShowUsage);
    }

    #[test]
    fn positional_only_shows_usage() {
        // Non-flag args fall through the loop → usage.
        assert_eq!(m(&["something"]), Mode::ShowUsage);
    }

    #[test]
    fn openrgb_args_filtered_before_classification() {
        // OpenRGB flag alone must not trip the usage path.
        assert_eq!(m(&["--enable-openrgb"]), Mode::Run);
        assert_eq!(m(&["/rgb"]), Mode::Run);
        // Combined with a real mode flag.
        assert_eq!(m(&["--enable-openrgb", "/s"]), Mode::Run);
        assert_eq!(m(&["/rgb", "-h"]), Mode::ShowUsage);
    }

    #[test]
    fn first_matching_arg_wins() {
        assert_eq!(m(&["/c", "/s"]), Mode::Configure);
        assert_eq!(m(&["/s", "/c"]), Mode::Run);
    }
}
