// SPDX-License-Identifier: MIT

//! Buffer import helpers via `zwp_linux_dmabuf_v1`.

use std::os::fd::BorrowedFd;

use wayland_client::protocol::wl_buffer;
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_buffer_params_v1, zwp_linux_dmabuf_v1,
};

use crate::overlay::state::SessionState;

/// Import a Linux DMA-BUF memory descriptor into a Wayland `wl_buffer`.
#[allow(dead_code)]
pub fn import_dmabuf_buffer(
    dmabuf: &zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
    fd: BorrowedFd<'_>,
    width: u32,
    height: u32,
    stride: u32,
    drm_format: u32,
    modifier: u64,
    queue: &QueueHandle<SessionState>,
) -> Option<wl_buffer::WlBuffer> {
    let params = dmabuf.create_params(queue, ());
    params.add(
        fd,
        0,
        0,
        stride,
        (modifier >> 32) as u32,
        (modifier & 0xFFFF_FFFF) as u32,
    );

    let buffer = params.create_immed(
        width as i32,
        height as i32,
        drm_format,
        zwp_linux_buffer_params_v1::Flags::empty(),
        queue,
        (),
    );
    params.destroy();
    Some(buffer)
}

impl Dispatch<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1, ()> for SessionState {
    fn event(
        _: &mut Self,
        _: &zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
        _: zwp_linux_dmabuf_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1, ()> for SessionState {
    fn event(
        _: &mut Self,
        _: &zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
        _: zwp_linux_buffer_params_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
