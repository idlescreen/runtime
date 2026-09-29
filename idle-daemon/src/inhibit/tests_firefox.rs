// SPDX-License-Identifier: MIT

use super::*;
use zbus::names::UniqueName;

fn client(name: &str) -> UniqueName<'static> {
    UniqueName::try_from(name.to_string()).unwrap()
}

#[test]
fn firefox_playing_video_coalesce_under_spam() {
    // Firefox often calls Inhibit many times for the same reason.
    let s = InhibitorState::new();
    let c = client(":1.145");
    let mut cookies = Vec::new();
    for _ in 0..50 {
        cookies.push(
            s.add(
                "org.mozilla.firefox".into(),
                "Playing video".into(),
                c.clone(),
            )
            .unwrap(),
        );
    }
    assert_eq!(s.len(), 1, "spam Inhibit must not stack holds");
    assert!(cookies.iter().all(|k| *k == cookies[0]));
    assert!(s.is_inhibited());
    assert!(s.remove_for_client(cookies[0], &c));
    assert!(!s.is_inhibited());
}

#[test]
fn firefox_prune_after_exit_clears_all_holds() {
    let s = InhibitorState::new();
    let ff = client(":1.200");
    let other = client(":1.50");
    for reason in ["Playing video", "Playing audio"] {
        let _ = s
            .add("org.mozilla.firefox".into(), reason.into(), ff.clone())
            .unwrap();
    }
    let _ = s
        .add("vlc".into(), "fullscreen".into(), other.clone())
        .unwrap();
    assert_eq!(s.len(), 3);
    // Firefox gone; VLC still live.
    let mut live = std::collections::HashSet::new();
    live.insert(":1.50".to_string());
    let n = s.prune_not_in_live_set(&live);
    assert_eq!(n, 2);
    assert_eq!(s.len(), 1);
    assert!(s.is_inhibited());
    // VLC also gone.
    let n = s.prune_not_in_live_set(&std::collections::HashSet::new());
    assert_eq!(n, 1);
    assert!(!s.is_inhibited());
}

#[test]
fn firefox_uninhibit_real_cookie_clears_when_no_phantom() {
    // Correct single-path (service only): UnInhibit is enough.
    let s = InhibitorState::new();
    let c = client(":1.145");
    let cookie = s
        .add(
            "org.mozilla.firefox".into(),
            "Playing video".into(),
            c.clone(),
        )
        .unwrap();
    assert!(
        cookie < 10000,
        "service cookies start at 1, not sniffer range"
    );
    assert!(s.remove_for_client(cookie, &c));
    assert_eq!(s.len(), 0);
    assert!(!s.is_inhibited());
}

#[test]
fn remove_client_by_unique_name_string_eq() {
    // NameOwnerChanged path: UniqueName compare via as_str.
    let s = InhibitorState::new();
    let c = client(":1.145");
    let _ = s
        .add(
            "org.mozilla.firefox".into(),
            "Playing video".into(),
            c.clone(),
        )
        .unwrap();
    s.remove_client(&client(":1.145"));
    assert!(!s.is_inhibited());
}
