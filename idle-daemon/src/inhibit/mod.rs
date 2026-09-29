// SPDX-License-Identifier: MIT

//! Idle inhibitors: IdleScreen cookies + external (logind idle / MPRIS).

mod external;
mod merge;
mod state;
mod zbus_helper;

pub use external::list_external;
pub use merge::merge_inhibitor_rows;
pub use state::{Inhibitor, InhibitorState};

#[cfg(test)]
mod tests_firefox;
#[cfg(test)]
mod tests_merge;
#[cfg(test)]
mod tests_state;
