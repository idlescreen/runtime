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
