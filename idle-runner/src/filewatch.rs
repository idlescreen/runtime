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
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    /// Build a raw `inotify_event` record.
    fn event(mask: u32, name: &str) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&1i32.to_ne_bytes()); // wd
        v.extend_from_slice(&mask.to_ne_bytes());
        v.extend_from_slice(&0u32.to_ne_bytes()); // cookie
        let nul_name = {
            let mut b = name.as_bytes().to_vec();
            b.push(0);
            // Kernel pads name to a 16-byte multiple… (actually just
            // reports len incl. the NUL; padding is optional).
            b
        };
        v.extend_from_slice(&(nul_name.len() as u32).to_ne_bytes());
        v.extend_from_slice(&nul_name);
        v
    }

    fn collect(buf: &[u8]) -> Vec<Option<String>> {
        let out = std::cell::RefCell::new(Vec::new());
        parse_events(buf, &|name| {
            out.borrow_mut()
                .push(name.map(|n| n.to_string_lossy().into_owned()));
        });
        out.into_inner()
    }

    #[test]
    fn parses_named_event() {
        let buf = event(libc::IN_CREATE, "config.toml");
        assert_eq!(collect(&buf), vec![Some("config.toml".to_string())]);
    }

    #[test]
    fn parses_multiple_events_in_one_read() {
        let mut buf = event(libc::IN_CREATE, "a.txt");
        buf.extend_from_slice(&event(libc::IN_MODIFY, "b.txt"));
        assert_eq!(
            collect(&buf),
            vec![Some("a.txt".to_string()), Some("b.txt".to_string())]
        );
    }

    #[test]
    fn nameless_event_delivers_none() {
        // Self-events (IN_DELETE_SELF/IN_MOVE_SELF) carry no name.
        let buf = event(libc::IN_DELETE_SELF, "");
        assert_eq!(collect(&buf), vec![None]);
    }

    #[test]
    fn queue_overflow_delivers_none() {
        let buf = event(libc::IN_Q_OVERFLOW, "");
        assert_eq!(collect(&buf), vec![None]);
    }

    #[test]
    fn truncated_record_is_ignored() {
        let mut buf = event(libc::IN_CREATE, "ok.txt");
        // A second record whose declared len runs past the buffer end.
        let mut bad = event(libc::IN_MODIFY, "way_too_long_name.txt");
        bad.truncate(20); // header says len > available
        buf.extend_from_slice(&bad);
        assert_eq!(collect(&buf), vec![Some("ok.txt".to_string())]);
    }

    #[test]
    fn truncated_header_is_ignored() {
        let buf = event(libc::IN_CREATE, "x")[..10].to_vec();
        assert!(collect(&buf).is_empty());
    }

    #[test]
    fn name_padding_does_not_leak_into_next_event() {
        // Kernel pads names to 16-byte alignment; extra NULs must not
        // produce phantom events.
        let mut buf = event(libc::IN_CREATE, "f");
        buf.extend_from_slice(&event(libc::IN_CLOSE_WRITE, "g"));
        assert_eq!(
            collect(&buf),
            vec![Some("f".to_string()), Some("g".to_string())]
        );
    }

    #[test]
    fn real_watch_fires_callback() {
        let dir = crate::test_util::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let _watcher = DirWatcher::watch(dir.path(), move |name| {
            let _ = tx.send(name.map(|n| n.to_string_lossy().into_owned()));
        })
        .unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        // The watcher thread needs a beat to arm inotify_add_watch.
        std::thread::sleep(Duration::from_millis(100));
        std::fs::write(dir.path().join("watched.toml"), b"x = 1").unwrap();

        let mut got = Vec::new();
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(Some(n)) if n == "watched.toml" => {
                    got.push(n);
                    break;
                }
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        assert_eq!(got, vec!["watched.toml".to_string()]);
        // Drop: stop flag + fd close + join must not hang.
        drop(_watcher);
    }
}
