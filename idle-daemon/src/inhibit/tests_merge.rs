// SPDX-License-Identifier: MIT

use super::external::ExternalInhibitor;
use super::*;

#[test]
fn merge_includes_real_logind_external() {
    let local = vec![(1u32, "app".into(), "reason".into())];
    let external = vec![ExternalInhibitor {
        source: "logind".into(),
        who: "vlc (block)".into(),
        why: "playing video".into(),
    }];
    let rows = merge_inhibitor_rows(local, &external);
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter().any(|(k, app, why)| {
            *k == 0 && app == "logind:vlc (block)" && why == "playing video"
        }),
        "vlc logind row missing: {rows:?}"
    );
}

#[test]
fn merge_drops_grok_agent_turn_logind() {
    let rows = merge_inhibitor_rows(
        vec![],
        &[
            ExternalInhibitor {
                source: "logind".into(),
                who: "grok (block)".into(),
                why: "agent turn in progress".into(),
            },
            ExternalInhibitor {
                source: "mpris".into(),
                who: "spotify".into(),
                why: "PlaybackStatus=Playing".into(),
            },
        ],
    );
    assert_eq!(rows.len(), 1, "only mpris should remain: {rows:?}");
    assert!(rows[0].1.starts_with("mpris:"));
    assert!(!rows.iter().any(|(_, app, _)| app.contains("grok")));
}

#[test]
fn merge_external_only_not_empty() {
    let rows = merge_inhibitor_rows(
        vec![],
        &[ExternalInhibitor {
            source: "mpris".into(),
            who: "firefox".into(),
            why: "PlaybackStatus=Playing".into(),
        }],
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, 0);
    assert!(rows[0].1.starts_with("mpris:"));
}

#[test]
fn merge_preserves_local_then_external_order() {
    let rows = merge_inhibitor_rows(
        vec![
            (2u32, "app-a".into(), "r1".into()),
            (5u32, "app-b".into(), "r2".into()),
        ],
        &[
            ExternalInhibitor {
                source: "logind".into(),
                who: "agent".into(),
                why: "busy".into(),
            },
            ExternalInhibitor {
                source: "mpris".into(),
                who: "player".into(),
                why: "Playing".into(),
            },
        ],
    );
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].0, 2);
    assert_eq!(rows[1].0, 5);
    assert_eq!(rows[2].1, "logind:agent");
    assert_eq!(rows[3].1, "mpris:player");
}

#[test]
fn merge_empty_local_and_external_is_empty() {
    let rows = merge_inhibitor_rows(vec![], &[]);
    assert!(rows.is_empty());
}
