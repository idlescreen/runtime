// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Atomic rolling state log file (`~/.local/state/idlescreen/idle-daemon.log`).

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const DEFAULT_MAX_BYTES: u64 = 5 * 1024 * 1024; // 5 MB

pub struct RollingLog {
    path: PathBuf,
    max_bytes: u64,
    file: Option<File>,
    written_bytes: u64,
}

static ROLLING_LOG: Mutex<Option<RollingLog>> = Mutex::new(None);

impl RollingLog {
    pub fn new(path: PathBuf, max_bytes: u64) -> Self {
        Self {
            path,
            max_bytes,
            file: None,
            written_bytes: 0,
        }
    }

    pub fn canonical_path() -> PathBuf {
        if let Ok(override_path) = std::env::var("IDLE_LOG_FILE") {
            return PathBuf::from(override_path);
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("idlescreen")
            .join("idle-daemon.log")
    }

    pub fn append(&mut self, line: &str) -> std::io::Result<()> {
        let line_len = line.len() as u64 + 1;
        if self.file.is_none() {
            if let Some(parent) = self.path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let f = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            self.written_bytes = f.metadata().map(|m| m.len()).unwrap_or(0);
            self.file = Some(f);
        }

        if self.written_bytes + line_len >= self.max_bytes {
            self.file = None;
            let backup = self.path.with_extension("log.1");
            let _ = std::fs::rename(&self.path, backup);
            let f = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            self.file = Some(f);
            self.written_bytes = 0;
        }

        if let Some(ref mut file) = self.file {
            writeln!(file, "{line}")?;
            let _ = file.flush();
            self.written_bytes += line_len;
        }
        Ok(())
    }
}

pub fn init_file(custom_path: Option<&Path>) {
    let path = custom_path
        .map(PathBuf::from)
        .unwrap_or_else(RollingLog::canonical_path);
    let max_bytes = std::env::var("IDLE_LOG_MAX_BYTES")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_MAX_BYTES);
    let mut lock = ROLLING_LOG.lock().unwrap_or_else(|e| e.into_inner());
    *lock = Some(RollingLog::new(path, max_bytes));
}

pub fn append(line: &str) {
    let mut lock = ROLLING_LOG.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(ref mut logger) = *lock {
        let _ = logger.append(line);
    }
}
