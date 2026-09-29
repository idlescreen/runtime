// SPDX-License-Identifier: MIT

//! Minimal inotify-based directory watcher — replaces `notify` for the two
//! watch patterns used here: "tell me when file X in directory Y is created
//! or modified". Linux-only, same as the rest of the runner.
//!
//! Dropping the watcher stops the thread; events are delivered on a
//! dedicated OS thread via the callback.

use std::ffi::OsStr;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const WATCH_MASK: u32 = libc::IN_MODIFY
    | libc::IN_CREATE
    | libc::IN_MOVED_TO
    | libc::IN_CLOSE_WRITE
    | libc::IN_ATTRIB
    | libc::IN_DELETE_SELF
    | libc::IN_MOVE_SELF;

/// Watches a directory; `callback` fires with the file name for each
/// create/modify/move event (self-events pass `None`).
pub struct DirWatcher {
    fd: libc::c_int,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl DirWatcher {
    /// Start watching `dir` (non-recursive). The callback runs on a
    /// background thread and must not block.
    pub fn watch<F>(dir: &Path, callback: F) -> io::Result<Self>
    where
        F: Fn(Option<&OsStr>) + Send + 'static,
    {
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let c_dir = std::ffi::CString::new(dir.as_os_str().as_bytes())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let wd = unsafe { libc::inotify_add_watch(fd, c_dir.as_ptr(), WATCH_MASK) };
        if wd < 0 {
            let err = io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(err);
        }

        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("idle-filewatch".into())
            .spawn(move || run_loop(fd, stop_thread, callback))?;
        Ok(Self {
            fd,
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for DirWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Closing the fd wakes the poll/read loop so the thread exits.
        unsafe { libc::close(self.fd) };
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run_loop(fd: libc::c_int, stop: Arc<AtomicBool>, callback: impl Fn(Option<&OsStr>)) {
    let mut buf = [0u8; 8192];
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let mut fds = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // 250ms cap so `stop` is observed promptly without a self-pipe.
        let ready = unsafe { libc::poll(&mut fds, 1, 250) };
        if ready <= 0 {
            continue; // timeout or EINTR — loop re-checks `stop`
        }
        let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n <= 0 {
            if n < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::Interrupted || e.kind() == io::ErrorKind::WouldBlock {
                    continue;
                }
            }
            return; // fd closed or hard error
        }
        parse_events(&buf[..n as usize], &callback);
    }
}

/// Parse one read-buffer's worth of inotify_event records:
/// `{wd, mask, cookie, len, name[len]}`. Malformed/truncated tails are
/// ignored; self-events and queue overflow deliver `None`.
fn parse_events(buf: &[u8], callback: &dyn Fn(Option<&OsStr>)) {
    let mut off = 0usize;
    while off + 16 <= buf.len() {
        let mask = u32::from_ne_bytes([buf[off + 4], buf[off + 5], buf[off + 6], buf[off + 7]]);
        let len = u32::from_ne_bytes([buf[off + 12], buf[off + 13], buf[off + 14], buf[off + 15]])
            as usize;
        let name_start = off + 16;
        let name_end = name_start + len;
        if name_end > buf.len() {
            break;
        }
        if mask & libc::IN_Q_OVERFLOW != 0 {
            // Queue overflow: surface as a catch-all event.
            callback(None);
        } else {
            let raw = &buf[name_start..name_end];
            let nul = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            if nul > 0 {
                callback(Some(OsStr::from_bytes(&raw[..nul])));
            } else {
                callback(None); // self-referential event (IN_DELETE_SELF etc.)
            }
        }
        // struct inotify_event is 16 header bytes + `len` name bytes.
        off += 16 + len;
    }
}

#[cfg(test)]
mod tests;
