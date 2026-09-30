// SPDX-License-Identifier: MIT

//! Multi-buffer pool for DMA-BUF recycling via `wl_buffer.release`.

use wayland_client::protocol::wl_buffer;

#[allow(dead_code)]
pub struct DmaBufSlot {
    pub id: usize,
    pub wl_buffer: Option<wl_buffer::WlBuffer>,
    pub in_use: bool,
    pub width: u32,
    pub height: u32,
}

pub struct DmaBufPool {
    slots: Vec<DmaBufSlot>,
    next_id: usize,
    max_slots: usize,
}

#[allow(dead_code)]
impl DmaBufPool {
    pub fn new(max_slots: usize) -> Self {
        Self {
            slots: Vec::with_capacity(max_slots),
            next_id: 0,
            max_slots,
        }
    }

    pub fn acquire_slot(&mut self, width: u32, height: u32) -> Option<usize> {
        if let Some(pos) = self
            .slots
            .iter()
            .position(|s| !s.in_use && s.width == width && s.height == height)
        {
            self.slots[pos].in_use = true;
            return Some(pos);
        }

        if let Some(pos) = self.slots.iter().position(|s| !s.in_use) {
            if let Some(buf) = self.slots[pos].wl_buffer.take() {
                buf.destroy();
            }
            self.slots[pos].in_use = true;
            self.slots[pos].width = width;
            self.slots[pos].height = height;
            return Some(pos);
        }

        if self.slots.len() < self.max_slots {
            let id = self.next_id;
            self.next_id = self.next_id.saturating_add(1);
            let slot = DmaBufSlot {
                id,
                wl_buffer: None,
                in_use: true,
                width,
                height,
            };
            self.slots.push(slot);
            Some(self.slots.len() - 1)
        } else {
            None
        }
    }

    pub fn release_slot_by_buffer(&mut self, buffer: &wl_buffer::WlBuffer) -> bool {
        for slot in &mut self.slots {
            if let Some(ref b) = slot.wl_buffer
                && b == buffer
            {
                slot.in_use = false;
                return true;
            }
        }
        false
    }

    pub fn release_slot_by_index(&mut self, idx: usize) {
        if idx < self.slots.len() {
            self.slots[idx].in_use = false;
        }
    }

    pub fn set_buffer(&mut self, idx: usize, buffer: wl_buffer::WlBuffer) {
        if idx < self.slots.len() {
            if let Some(old) = self.slots[idx].wl_buffer.take() {
                old.destroy();
            }
            self.slots[idx].wl_buffer = Some(buffer);
        }
    }

    pub fn get_slot(&self, idx: usize) -> Option<&DmaBufSlot> {
        self.slots.get(idx)
    }

    pub fn clear(&mut self) {
        for slot in &mut self.slots {
            if let Some(buf) = slot.wl_buffer.take() {
                buf.destroy();
            }
            slot.in_use = false;
        }
        self.slots.clear();
    }
}

impl Drop for DmaBufPool {
    fn drop(&mut self) {
        self.clear();
    }
}
