// SPDX-License-Identifier: MIT

use std::sync::{Arc, Mutex};

/// A configured Wayland output ready for frame submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputLayout {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub refresh_rate_hz: u32,
    pub x: i32,
    pub y: i32,
    pub scale: i32,
}

#[derive(Clone, Default)]
pub struct OutputRegistry(Arc<Mutex<Vec<OutputLayout>>>);

impl OutputRegistry {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(Vec::new())))
    }

    pub fn upsert(&self, layout: OutputLayout) {
        if let Ok(mut guard) = self.0.lock() {
            if let Some(existing) = guard.iter_mut().find(|entry| entry.id == layout.id) {
                *existing = layout;
            } else {
                guard.push(layout);
            }
        }
    }

    pub fn remove(&self, id: u32) {
        if let Ok(mut guard) = self.0.lock() {
            guard.retain(|entry| entry.id != id);
        }
    }

    pub fn clear(&self) {
        if let Ok(mut guard) = self.0.lock() {
            guard.clear();
        }
    }

    pub fn layouts(&self) -> Vec<OutputLayout> {
        self.0.lock().map(|guard| guard.clone()).unwrap_or_default()
    }
}
