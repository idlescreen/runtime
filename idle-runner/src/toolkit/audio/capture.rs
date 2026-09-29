// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Host audio capture sampling PulseAudio or PipeWire monitor stream.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::bands_calc::{NUM_AUDIO_BANDS, compute_audio_bands};

/// Check if PulseAudio or PipeWire audio socket is available.
pub fn is_audio_socket_available() -> bool {
    let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") else {
        return false;
    };
    let rt = Path::new(&runtime_dir);
    rt.join("pulse/native").exists() || rt.join("pipewire-0").exists()
}

/// Locate capture utility (`parec` or `pw-record`) on system.
fn find_capture_binary() -> Option<PathBuf> {
    for candidate in &["/usr/bin/parec", "/usr/bin/pw-record", "/bin/parec"] {
        let p = PathBuf::from(candidate);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Host audio capture manager.
pub struct AudioCapture {
    stop_signal: Arc<AtomicBool>,
    bands: Arc<Mutex<[f32; NUM_AUDIO_BANDS]>>,
    _worker: Option<JoinHandle<()>>,
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.stop_signal.store(true, Ordering::Release);
    }
}

impl AudioCapture {
    /// Create and start audio capture if backend is present, otherwise returns silent fallback.
    pub fn start() -> Self {
        let stop_signal = Arc::new(AtomicBool::new(false));
        let bands = Arc::new(Mutex::new([0.0f32; NUM_AUDIO_BANDS]));

        if !is_audio_socket_available() {
            return Self {
                stop_signal,
                bands,
                _worker: None,
            };
        }

        let Some(bin) = find_capture_binary() else {
            return Self {
                stop_signal,
                bands,
                _worker: None,
            };
        };

        let stop = Arc::clone(&stop_signal);
        let shared_bands = Arc::clone(&bands);

        let handle = thread::Builder::new()
            .name("idle-audio-capture".into())
            .spawn(move || {
                run_capture_loop(bin, stop, shared_bands);
            })
            .ok();

        Self {
            stop_signal,
            bands,
            _worker: handle,
        }
    }

    /// Read current snapshot of audio frequency bands.
    pub fn current_bands(&self) -> [f32; NUM_AUDIO_BANDS] {
        match self.bands.lock() {
            Ok(g) => *g,
            Err(poisoned) => *poisoned.into_inner(),
        }
    }
}

/// Child process RAII killer.
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run_capture_loop(
    bin: PathBuf,
    stop: Arc<AtomicBool>,
    bands: Arc<Mutex<[f32; NUM_AUDIO_BANDS]>>,
) {
    let mut child = match Command::new(&bin)
        .arg("--raw")
        .arg("--channels=1")
        .arg("--rate=8000")
        .arg("--format=s16le")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => ChildGuard(c),
        Err(_) => return,
    };

    let Some(mut stdout) = child.0.stdout.take() else {
        return;
    };

    // Buffer: 128 samples = 256 bytes (~16ms at 8000 Hz)
    let mut raw_buf = [0u8; 256];
    let mut sample_buf = [0i16; 128];

    while !stop.load(Ordering::Relaxed) {
        match stdout.read_exact(&mut raw_buf) {
            Ok(()) => {
                for (i, chunk) in raw_buf.as_chunks::<2>().0.iter().enumerate() {
                    sample_buf[i] = i16::from_le_bytes([chunk[0], chunk[1]]);
                }
                let computed = compute_audio_bands(&sample_buf);
                if let Ok(mut g) = bands.lock() {
                    // Exponential moving average smoothing
                    for b in 0..NUM_AUDIO_BANDS {
                        g[b] = g[b] * 0.4 + computed[b] * 0.6;
                    }
                }
            }
            Err(_) => {
                // Read error or EOF; sleep briefly to avoid spin
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}
