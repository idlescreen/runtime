// SPDX-License-Identifier: MIT

use super::*;
use zbus::names::UniqueName;

fn client(name: &str) -> UniqueName<'static> {
    UniqueName::try_from(name.to_string()).unwrap()
}

#[test]
fn inhibitor_state_starts_uninhibited() {
    let s = InhibitorState::new();
    assert!(!s.is_inhibited());
}

#[test]
fn add_inhibitor_marks_inhibited() {
    let s = InhibitorState::new();
    let c = client(":test.app.Inhibitor");
    let cookie = s
        .add("app".to_string(), "reason".to_string(), c.clone())
        .unwrap();
    assert!(cookie > 0);
    assert!(s.is_inhibited());
}

#[test]
fn remove_for_client_clears_inhibition() {
    let s = InhibitorState::new();
    let c = client(":test.app.Inhibitor");
    let cookie = s
        .add("app".to_string(), "reason".to_string(), c.clone())
        .unwrap();
    assert!(s.remove_for_client(cookie, &c));
    assert!(!s.is_inhibited());
}

#[test]
fn remove_for_client_wrong_cookie_returns_false() {
    let s = InhibitorState::new();
    let c = client(":test.app.Inhibitor");
    let _ = s.add("app".to_string(), "reason".to_string(), c.clone());
    assert!(!s.remove_for_client(9999, &c));
    assert!(s.is_inhibited());
}

#[test]
fn remove_for_client_wrong_client_returns_false() {
    let s = InhibitorState::new();
    let c1 = client(":test.one.Client");
    let c2 = client(":test.two.Client");
    let cookie = s
        .add("app".to_string(), "reason".to_string(), c1.clone())
        .unwrap();
    assert!(!s.remove_for_client(cookie, &c2));
    assert!(s.is_inhibited());
}

#[test]
fn remove_client_clears_all_for_that_client() {
    let s = InhibitorState::new();
    let c1 = client(":test.one.Client");
    let c2 = client(":test.two.Client");
    let _ = s
        .add("app".to_string(), "reason1".to_string(), c1.clone())
        .unwrap();
    let _ = s
        .add("app".to_string(), "reason2".to_string(), c1.clone())
        .unwrap();
    let _ = s
        .add("app".to_string(), "reason3".to_string(), c2.clone())
        .unwrap();
    s.remove_client(&c1);
    assert!(s.is_inhibited()); // c2 still holds an inhibitor
    s.remove_client(&c2);
    assert!(!s.is_inhibited());
}

#[test]
fn cookies_are_unique_and_increasing_for_distinct_reasons() {
    let s = InhibitorState::new();
    let c = client(":test.app.Cookie");
    let k1 = s.add("a".to_string(), "r1".to_string(), c.clone()).unwrap();
    let k2 = s.add("a".to_string(), "r2".to_string(), c.clone()).unwrap();
    let k3 = s.add("a".to_string(), "r3".to_string(), c.clone()).unwrap();
    assert!(k1 < k2);
    assert!(k2 < k3);
}

#[test]
fn add_coalesces_same_client_app_reason() {
    let s = InhibitorState::new();
    let c = client(":test.app.Coalesce");
    let k1 = s
        .add("firefox".into(), "Playing video".into(), c.clone())
        .unwrap();
    let k2 = s
        .add("firefox".into(), "Playing video".into(), c.clone())
        .unwrap();
    assert_eq!(k1, k2, "duplicate Inhibit must reuse cookie");
    assert_eq!(s.len(), 1);
}

#[test]
fn prune_not_in_live_set_drops_dead_peers() {
    let s = InhibitorState::new();
    let live = client(":1.100");
    let dead = client(":1.999");
    let _ = s
        .add("firefox".into(), "Playing video".into(), live.clone())
        .unwrap();
    let _ = s
        .add("firefox".into(), "Playing video".into(), dead.clone())
        .unwrap();
    assert_eq!(s.len(), 2);
    let mut set = std::collections::HashSet::new();
    set.insert(":1.100".to_string());
    let n = s.prune_not_in_live_set(&set);
    assert_eq!(n, 1);
    assert_eq!(s.len(), 1);
    assert!(s.is_inhibited());
    set.clear();
    let n = s.prune_not_in_live_set(&set);
    assert_eq!(n, 1);
    assert!(!s.is_inhibited());
}

#[test]
fn add_rejects_when_at_capacity_for_one_client() {
    let s = InhibitorState::new();
    let c = client(":test.app.Capacity");
    for i in 0..32 {
        assert!(
            s.add("a".to_string(), format!("r{i}"), c.clone()).is_ok(),
            "expected add {i} to succeed"
        );
    }
    // 33rd should be rejected (per-cap of 32)
    assert!(s.add("a".to_string(), "r".to_string(), c.clone()).is_err());
}

#[test]
fn list_all_includes_local_cookies() {
    let s = InhibitorState::new();
    let c = client(":test.app.List");
    let cookie = s.add("myapp".into(), "fullscreen".into(), c).expect("add");
    let rows = s.list_all();
    assert!(
        rows.iter()
            .any(|(k, app, why)| *k == cookie && app == "myapp" && why == "fullscreen"),
        "local cookie missing from list_all: {rows:?}"
    );
}

#[test]
fn add_rejects_excessively_long_strings_negative_selection() {
    let s = InhibitorState::new();
    let c = client(":test.app.Length");
    let long_string = "a".repeat(2000);
    assert!(
        s.add(long_string.clone(), "reason".to_string(), c.clone())
            .is_err(),
        "must fail if application_name is too long"
    );
    assert!(
        s.add("app".to_string(), long_string, c).is_err(),
        "must fail if reason is too long"
    );
}

#[test]
fn test_immune_rail_dbus_inhibitor_limits() {
    let state = InhibitorState::new();
    let client1 = UniqueName::try_from(":1.100").unwrap();
    let client2 = UniqueName::try_from(":1.200").unwrap();

    let long_app = "A".repeat(2000);
    let res = state.add(long_app, "valid_reason".into(), client1.clone());
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("maximum length"));

    let long_reason = "R".repeat(65536);
    let res = state.add("valid_app".into(), long_reason, client1.clone());
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("maximum length"));

    for i in 0..32 {
        assert!(
            state
                .add("app".into(), format!("reason_{i}"), client1.clone())
                .is_ok()
        );
    }
    let overflow = state.add("app".into(), "reason_33".into(), client1.clone());
    assert!(overflow.is_err());
    assert!(
        overflow
            .unwrap_err()
            .contains("too many concurrent inhibitors")
    );

    assert!(state.add("app".into(), "reason_1".into(), client2).is_ok());
}
