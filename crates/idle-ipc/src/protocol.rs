// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum IpcCommand {
    Init { cols: u32, rows: u32 },
    TickAndDraw { dt_micros: u64 },
    SetSimulationRate { hz: f32 },
    Stop,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum IpcResponse {
    Ready,
    FrameReady { scanlines: bool, dirty: bool },
    Ack,
}

impl IpcCommand {
    pub fn write_to<W: std::io::Write>(&self, mut writer: W) -> std::io::Result<()> {
        match self {
            IpcCommand::Init { cols, rows } => {
                writer.write_all(&[0])?;
                writer.write_all(&cols.to_le_bytes())?;
                writer.write_all(&rows.to_le_bytes())?;
            }
            IpcCommand::TickAndDraw { dt_micros } => {
                writer.write_all(&[1])?;
                writer.write_all(&dt_micros.to_le_bytes())?;
            }
            IpcCommand::SetSimulationRate { hz } => {
                writer.write_all(&[2])?;
                writer.write_all(&hz.to_le_bytes())?;
            }
            IpcCommand::Stop => {
                writer.write_all(&[3])?;
            }
        }
        Ok(())
    }

    pub fn read_from<R: std::io::Read>(mut reader: R) -> std::io::Result<Self> {
        let mut tag = [0u8; 1];
        reader.read_exact(&mut tag)?;
        match tag[0] {
            0 => {
                let mut cols_bytes = [0u8; 4];
                let mut rows_bytes = [0u8; 4];
                reader.read_exact(&mut cols_bytes)?;
                reader.read_exact(&mut rows_bytes)?;
                let cols = u32::from_le_bytes(cols_bytes);
                let rows = u32::from_le_bytes(rows_bytes);
                crate::ffi_cell::validate_grid_dims(cols as usize, rows as usize)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
                Ok(IpcCommand::Init { cols, rows })
            }
            1 => {
                let mut dt_bytes = [0u8; 8];
                reader.read_exact(&mut dt_bytes)?;
                Ok(IpcCommand::TickAndDraw {
                    dt_micros: u64::from_le_bytes(dt_bytes),
                })
            }
            2 => {
                let mut hz_bytes = [0u8; 4];
                reader.read_exact(&mut hz_bytes)?;
                Ok(IpcCommand::SetSimulationRate {
                    hz: f32::from_le_bytes(hz_bytes),
                })
            }
            3 => Ok(IpcCommand::Stop),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid command tag",
            )),
        }
    }
}

impl IpcResponse {
    pub fn write_to<W: std::io::Write>(&self, mut writer: W) -> std::io::Result<()> {
        match self {
            IpcResponse::Ready => {
                writer.write_all(&[0])?;
            }
            IpcResponse::FrameReady { scanlines, dirty } => {
                writer.write_all(&[
                    1,
                    if *scanlines { 1 } else { 0 },
                    if *dirty { 1 } else { 0 },
                ])?;
            }
            IpcResponse::Ack => {
                writer.write_all(&[2])?;
            }
        }
        Ok(())
    }

    pub fn read_from<R: std::io::Read>(mut reader: R) -> std::io::Result<Self> {
        let mut tag = [0u8; 1];
        reader.read_exact(&mut tag)?;
        match tag[0] {
            0 => Ok(IpcResponse::Ready),
            1 => {
                let mut data = [0u8; 2];
                reader.read_exact(&mut data)?;
                Ok(IpcResponse::FrameReady {
                    scanlines: data[0] != 0,
                    dirty: data[1] != 0,
                })
            }
            2 => Ok(IpcResponse::Ack),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid response tag",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipc_response_frame_ready_roundtrip() {
        let resp = IpcResponse::FrameReady {
            scanlines: true,
            dirty: false,
        };
        let mut buf = Vec::new();
        resp.write_to(&mut buf).unwrap();
        assert_eq!(buf, vec![1, 1, 0]);
        let read_resp = IpcResponse::read_from(&buf[..]).unwrap();
        assert_eq!(resp, read_resp);
    }
}
