// SPDX-License-Identifier: MIT

use super::external;

/// Merge local cookies with external blocks (pure; unit-tested).
pub fn merge_inhibitor_rows(
    local: Vec<(u32, String, String)>,
    external: &[external::ExternalInhibitor],
) -> Vec<(u32, String, String)> {
    let mut out = local;
    for ext in external {
        if ext.source == "logind" && external::ignore_logind_idle_hold(&ext.who, &ext.why) {
            continue;
        }
        out.push((0, format!("{}:{}", ext.source, ext.who), ext.why.clone()));
    }
    out
}
