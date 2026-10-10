// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Init system detection for process supervision.

use std::path::Path;

/// Supported init systems and process supervisors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InitSystem {
    Systemd,
    OpenRc,
    Runit,
    Dinit,
    S6,
    Standalone,
}

impl InitSystem {
    /// Return the canonical lower-case name.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Systemd => "systemd",
            Self::OpenRc => "openrc",
            Self::Runit => "runit",
            Self::Dinit => "dinit",
            Self::S6 => "s6",
            Self::Standalone => "standalone",
        }
    }

    /// Parse an init system from a string slice.
    #[must_use]
    pub fn parse_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "systemd" => Some(Self::Systemd),
            "openrc" => Some(Self::OpenRc),
            "runit" => Some(Self::Runit),
            "dinit" => Some(Self::Dinit),
            "s6" => Some(Self::S6),
            "standalone" => Some(Self::Standalone),
            _ => None,
        }
    }
}

impl std::fmt::Display for InitSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Abstract probe interface for init system markers.
pub(crate) trait InitProbe {
    fn path_exists(&self, path: &Path) -> bool;
    fn read_proc_comm(&self) -> Option<String>;
    fn env_var(&self, key: &str) -> Option<String>;
    fn command_exists(&self, cmd: &str) -> bool;
}

pub(crate) struct SystemInitProbe;

impl InitProbe for SystemInitProbe {
    fn path_exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn read_proc_comm(&self) -> Option<String> {
        std::fs::read_to_string("/proc/1/comm")
            .ok()
            .map(|s| s.trim().to_string())
    }

    fn env_var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    fn command_exists(&self, cmd: &str) -> bool {
        std::env::var_os("PATH")
            .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(cmd).is_file()))
    }
}

/// Detect the active init system in the current environment.
#[must_use]
pub fn detect_init_system() -> InitSystem {
    detect_init_system_from_probe(&SystemInitProbe)
}

pub(crate) fn detect_init_system_from_probe(probe: &dyn InitProbe) -> InitSystem {
    if let Some(forced) = probe.env_var("IDLE_INIT_SYSTEM")
        && let Some(init) = InitSystem::parse_str(&forced)
    {
        return init;
    }

    if is_systemd(probe) {
        return InitSystem::Systemd;
    }
    if is_dinit(probe) {
        return InitSystem::Dinit;
    }
    if is_openrc(probe) {
        return InitSystem::OpenRc;
    }
    if is_runit(probe) {
        return InitSystem::Runit;
    }
    if is_s6(probe) {
        return InitSystem::S6;
    }

    InitSystem::Standalone
}

fn is_systemd(probe: &dyn InitProbe) -> bool {
    (probe.path_exists(Path::new("/run/systemd/system"))
        || probe.read_proc_comm().as_deref() == Some("systemd"))
        && probe.command_exists("systemctl")
}

fn is_dinit(probe: &dyn InitProbe) -> bool {
    if probe.env_var("DINIT_CS_SOCKET").is_some() {
        return true;
    }
    if let Some(runtime) = probe.env_var("XDG_RUNTIME_DIR")
        && probe.path_exists(&Path::new(&runtime).join("dinitctl"))
    {
        return true;
    }
    probe.path_exists(Path::new("/run/dinit"))
        || (probe.read_proc_comm().as_deref() == Some("dinit") && probe.command_exists("dinitctl"))
}

fn is_openrc(probe: &dyn InitProbe) -> bool {
    probe.path_exists(Path::new("/run/openrc"))
        || probe.path_exists(Path::new("/run/openrc/started"))
        || probe.command_exists("rc-service")
}

fn is_runit(probe: &dyn InitProbe) -> bool {
    probe.path_exists(Path::new("/run/runit.stop"))
        || probe.path_exists(Path::new("/run/runsvdir"))
        || probe.read_proc_comm().as_deref() == Some("runit")
        || probe.read_proc_comm().as_deref() == Some("runsvdir")
        || probe.env_var("SVDIR").is_some()
}

fn is_s6(probe: &dyn InitProbe) -> bool {
    probe.path_exists(Path::new("/run/s6"))
        || probe.path_exists(Path::new("/run/service"))
        || probe.read_proc_comm().as_deref() == Some("s6-svscan")
        || probe.command_exists("s6-svc")
        || probe.command_exists("s6-rc")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    #[derive(Default)]
    struct MockInitProbe {
        paths: HashSet<PathBuf>,
        proc_comm: Option<String>,
        envs: HashMap<String, String>,
        commands: HashSet<String>,
    }

    impl InitProbe for MockInitProbe {
        fn path_exists(&self, path: &Path) -> bool {
            self.paths.contains(path)
        }
        fn read_proc_comm(&self) -> Option<String> {
            self.proc_comm.clone()
        }
        fn env_var(&self, key: &str) -> Option<String> {
            self.envs.get(key).cloned()
        }
        fn command_exists(&self, cmd: &str) -> bool {
            self.commands.contains(cmd)
        }
    }

    #[test]
    fn as_str_and_parse_roundtrip() {
        for init in [
            InitSystem::Systemd,
            InitSystem::OpenRc,
            InitSystem::Runit,
            InitSystem::Dinit,
            InitSystem::S6,
            InitSystem::Standalone,
        ] {
            assert_eq!(InitSystem::parse_str(init.as_str()), Some(init));
            assert_eq!(format!("{init}"), init.as_str());
        }
        assert_eq!(InitSystem::parse_str("unknown"), None);
    }

    #[test]
    fn detect_variants() {
        let mut p = MockInitProbe::default();
        assert_eq!(detect_init_system_from_probe(&p), InitSystem::Standalone);

        p.paths.insert(PathBuf::from("/run/systemd/system"));
        p.commands.insert("systemctl".to_string());
        assert_eq!(detect_init_system_from_probe(&p), InitSystem::Systemd);

        let mut p_openrc = MockInitProbe::default();
        p_openrc.commands.insert("rc-service".to_string());
        assert_eq!(detect_init_system_from_probe(&p_openrc), InitSystem::OpenRc);

        let mut p_runit = MockInitProbe::default();
        p_runit.paths.insert(PathBuf::from("/run/runsvdir"));
        assert_eq!(detect_init_system_from_probe(&p_runit), InitSystem::Runit);

        let mut p_dinit = MockInitProbe::default();
        p_dinit
            .envs
            .insert("DINIT_CS_SOCKET".to_string(), "1".to_string());
        assert_eq!(detect_init_system_from_probe(&p_dinit), InitSystem::Dinit);

        let mut p_s6 = MockInitProbe::default();
        p_s6.paths.insert(PathBuf::from("/run/s6"));
        assert_eq!(detect_init_system_from_probe(&p_s6), InitSystem::S6);
    }

    #[test]
    fn env_override_takes_precedence() {
        let mut probe = MockInitProbe::default();
        probe.paths.insert(PathBuf::from("/run/systemd/system"));
        probe.commands.insert("systemctl".to_string());
        probe
            .envs
            .insert("IDLE_INIT_SYSTEM".to_string(), "s6".to_string());
        assert_eq!(detect_init_system_from_probe(&probe), InitSystem::S6);
    }

    #[test]
    fn live_detection_returns_variant() {
        let init = detect_init_system();
        assert!(matches!(
            init,
            InitSystem::Systemd
                | InitSystem::OpenRc
                | InitSystem::Runit
                | InitSystem::Dinit
                | InitSystem::S6
                | InitSystem::Standalone
        ));
    }
}
