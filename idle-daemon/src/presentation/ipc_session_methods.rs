// SPDX-License-Identifier: MIT

use idle_api::TerminalCell;
use idle_ipc::IpcResponse;

use super::ipc_raster::raster_viewport_into;
use super::ipc_session::IpcPluginSession;
use super::timeout::is_timeout;

impl IpcPluginSession {
    pub fn draw_frame(&mut self, grid_cols: usize, grid_rows: usize) -> (bool, bool) {
        if let Some(ref mut socket) = self.socket {
            match IpcResponse::read_from(&mut *socket) {
                Ok(IpcResponse::FrameReady { scanlines, dirty }) => {
                    if let Some(ref shm) = self.shm {
                        // SAFETY: SHM mapped for session lifetime; dims set at init.
                        match unsafe { shm.cells_mut() } {
                            Ok(cells) => {
                                if let Some(need) = grid_cols.checked_mul(grid_rows) {
                                    if self.grid.len() != need {
                                        self.grid = vec![TerminalCell::default(); need];
                                    }
                                } else {
                                    idle_log::error!(
                                        "grid resize overflow: {grid_cols}x{grid_rows}"
                                    );
                                    return (false, false);
                                }
                                if dirty {
                                    // Zip avoids bounds checks on the destination grid.
                                    for (dst, src) in self.grid.iter_mut().zip(cells.iter()) {
                                        *dst = TerminalCell::from(*src);
                                    }
                                }
                            }
                            Err(e) => {
                                idle_log::error!("shm cells view rejected: {e}");
                            }
                        }
                    }
                    return (scanlines, dirty);
                }
                Ok(resp) => {
                    idle_log::error!("unexpected response to TickAndDraw: {:?}", resp);
                    self.socket = None;
                }
                Err(e) => {
                    if is_timeout(&e) {
                        idle_log::error!(
                            saver = %self.saver_name,
                            "IPC read timed out — saver hung inside TickAndDraw; killing child"
                        );
                        self.kill_child();
                        self.socket = None;
                        return (false, false);
                    }
                    idle_log::error!("failed to read response to TickAndDraw: {}", e);
                    self.socket = None;
                }
            }
        }
        (false, false)
    }

    pub fn raster_viewport(
        &mut self,
        col_start: usize,
        row_start: usize,
        cols: usize,
        rows: usize,
        grid_cols: usize,
        _grid_rows: usize,
        width: u32,
        height: u32,
        scanlines: bool,
        pixel_buf: &mut Vec<u8>,
    ) {
        let using_gpu = self.using_gpu_upscale();
        let hardware_scaling = self.hardware_scaling;
        raster_viewport_into(
            &mut self.renderer,
            &mut self.upscaler,
            &self.grid,
            hardware_scaling,
            using_gpu,
            &mut self.content_buf,
            pixel_buf,
            col_start,
            row_start,
            cols,
            rows,
            grid_cols,
            width,
            height,
            scanlines,
        );
    }
}
