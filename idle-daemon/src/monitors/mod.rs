// SPDX-License-Identifier: MIT

//! External event and lock monitors.

pub mod lock;
pub mod locks;
pub mod sleep;

pub use lock::*;
pub use locks::*;
pub use sleep::*;
