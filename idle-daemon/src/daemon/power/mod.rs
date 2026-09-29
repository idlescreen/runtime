// SPDX-License-Identifier: MIT

//! Power monitoring, AC/battery detection, and udev watcher.

pub mod battery;
pub mod thread;
pub mod upower;
#[cfg(test)]
mod upower_tests;
pub mod watcher;
