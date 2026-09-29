// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen
//
// Split out of `lib.rs` to keep that file under the 256-line cap (F-016/PROBE F-006).
// Referenced from `lib.rs` via `#[path = "ipc_tests.rs"]`.

use super::*;

#[test]
fn test_ipc_commands() {
    let cmds = vec![
        IpcCommand::Init {
            cols: 120,
            rows: 40,
        },
        IpcCommand::TickAndDraw { dt_micros: 16666 },
        IpcCommand::SetSimulationRate { hz: 60.0 },
        IpcCommand::Stop,
    ];

    for cmd in cmds {
        let mut buf = Vec::new();
        cmd.write_to(&mut buf).expect("encode command");
        let decoded = IpcCommand::read_from(&buf[..]).expect("decode command");
        assert_eq!(cmd, decoded);
    }
}

#[test]
fn test_ipc_responses() {
    let resps = vec![
        IpcResponse::Ready,
        IpcResponse::FrameReady {
            scanlines: true,
            dirty: true,
        },
        IpcResponse::FrameReady {
            scanlines: false,
            dirty: false,
        },
        IpcResponse::Ack,
    ];

    for resp in resps {
        let mut buf = Vec::new();
        resp.write_to(&mut buf).expect("encode response");
        let decoded = IpcResponse::read_from(&buf[..]).expect("decode response");
        assert_eq!(resp, decoded);
    }
}

#[test]
fn test_shm_size() {
    let size = compute_shm_size(80, 24).expect("size");
    let header_sz = std::mem::size_of::<SharedMemoryHeader>();
    let cell_sz = std::mem::size_of::<FfiTerminalCell>();
    assert_eq!(size, header_sz + 80 * 24 * cell_sz);
}

#[test]
fn test_compute_shm_size_zero_dimensions() {
    let size = compute_shm_size(0, 0).expect("zero grid is just header");
    assert_eq!(size, std::mem::size_of::<SharedMemoryHeader>());
}

#[test]
fn test_compute_shm_size_overflow_is_none() {
    assert!(compute_shm_size(usize::MAX, 2).is_none());
    assert!(compute_shm_size(usize::MAX / 2, usize::MAX / 2).is_none());
}

#[test]
fn test_validate_grid_dims() {
    assert!(validate_grid_dims(80, 24).is_ok());
    assert!(validate_grid_dims(0, 24).is_err());
    assert!(validate_grid_dims(80, 0).is_err());
    assert!(validate_grid_dims(0, 0).is_err());
    assert!(validate_grid_dims(MAX_GRID_DIM + 1, 1).is_err());
    assert!(validate_grid_dims(1, MAX_GRID_DIM + 1).is_err());
    assert!(validate_grid_dims(MAX_GRID_CELLS, 2).is_err());
}

#[test]
fn test_validate_grid_dims_boundaries() {
    // Exact axis max is allowed (`>` must not become `>=`).
    assert!(validate_grid_dims(MAX_GRID_DIM, 1).is_ok());
    assert!(validate_grid_dims(1, MAX_GRID_DIM).is_ok());
    // Exact cell-count max is allowed (`<=` must not become `<` or always-true).
    assert!(validate_grid_dims(512, 512).is_ok());
    assert_eq!(512 * 512, MAX_GRID_CELLS);
    // One cell over the cap with both axes still under MAX_GRID_DIM.
    assert!(validate_grid_dims(512, 513).is_err());
    assert!(validate_grid_dims(513, 512).is_err());
    // Both axes max → product far over MAX_GRID_CELLS.
    assert!(validate_grid_dims(MAX_GRID_DIM, MAX_GRID_DIM).is_err());
}

#[test]
fn test_ffi_terminal_cell_conversion() {
    use idle_api::TerminalCell;
    let cell = TerminalCell {
        ch: '★',
        fg: (255, 128, 64),
        bg: (10, 20, 30),
        bold: true,
    };
    let ffi = FfiTerminalCell::from(cell);
    assert_eq!(ffi.ch, '★' as u32);
    assert_eq!(ffi.fg_r, 255);
    assert_eq!(ffi.fg_g, 128);
    assert_eq!(ffi.fg_b, 64);
    assert_eq!(ffi.bold, 1);

    let roundtrip = TerminalCell::from(ffi);
    assert_eq!(cell, roundtrip);
}

#[test]
fn test_invalid_ipc_command_tag() {
    let bad_bytes = [99u8];
    assert!(IpcCommand::read_from(&bad_bytes[..]).is_err());
}

#[test]
fn test_invalid_ipc_response_tag() {
    let bad_bytes = [255u8];
    assert!(IpcResponse::read_from(&bad_bytes[..]).is_err());
}

#[test]
fn test_truncated_command_read() {
    let truncated = [0u8, 120]; // Tag 0 requires 8 bytes payload (cols:4, rows:4)
    assert!(IpcCommand::read_from(&truncated[..]).is_err());
}

#[test]
fn test_immune_rail_ipc_protocol_malformations() {
    use std::io::ErrorKind;

    let mut buf1 = vec![0u8];
    buf1.extend_from_slice(&u32::MAX.to_le_bytes());
    buf1.extend_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        IpcCommand::read_from(&buf1[..]).unwrap_err().kind(),
        ErrorKind::InvalidData
    );

    let mut buf2 = vec![0u8];
    buf2.extend_from_slice(&4097u32.to_le_bytes());
    buf2.extend_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        IpcCommand::read_from(&buf2[..]).unwrap_err().kind(),
        ErrorKind::InvalidData
    );

    assert_eq!(
        IpcCommand::read_from(&[0x05u8, 0, 0, 0][..])
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidData
    );
    assert_eq!(
        IpcCommand::read_from(&[0x00u8, 0x50, 0x00][..])
            .unwrap_err()
            .kind(),
        ErrorKind::UnexpectedEof
    );
}
