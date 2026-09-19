// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen
//
// Split out of `lib.rs` to keep that file under the 256-line cap (F-016/PROBE F-006).
// Referenced from `lib.rs` via `#[path = "ipc_proptests.rs"]`.

use super::*;

/// xorshift64* deterministic stand-in for proptest's generators.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo + 1)
    }
}

fn arb_command(r: &mut Rng) -> IpcCommand {
    match r.range(0, 3) {
        0 => IpcCommand::Init {
            cols: r.range(1, 512) as u32,
            rows: r.range(1, 512) as u32,
        },
        1 => IpcCommand::TickAndDraw {
            dt_micros: r.next(),
        },
        2 => {
            let hz = f32::from_bits(r.next() as u32);
            if hz.is_finite() {
                IpcCommand::SetSimulationRate { hz }
            } else {
                IpcCommand::SetSimulationRate { hz: 60.0 }
            }
        }
        _ => IpcCommand::Stop,
    }
}

fn arb_response(r: &mut Rng) -> IpcResponse {
    match r.range(0, 2) {
        0 => IpcResponse::Ready,
        1 => IpcResponse::FrameReady {
            scanlines: r.next() & 1 != 0,
            dirty: r.next() & 1 != 0,
        },
        _ => IpcResponse::Ack,
    }
}

/// Every command encodes and decodes to an equal value.
#[test]
fn command_roundtrip() {
    let mut rng = Rng(0x19C0_0001);
    for _ in 0..256 {
        let cmd = arb_command(&mut rng);
        let mut buf = Vec::new();
        cmd.write_to(&mut buf).expect("write");
        let decoded = IpcCommand::read_from(&buf[..]).expect("read");
        assert_eq!(cmd, decoded);
    }
}

/// Every response encodes and decodes to an equal value.
#[test]
fn response_roundtrip() {
    let mut rng = Rng(0x19C0_0002);
    for _ in 0..256 {
        let resp = arb_response(&mut rng);
        let mut buf = Vec::new();
        resp.write_to(&mut buf).expect("write");
        let decoded = IpcResponse::read_from(&buf[..]).expect("read");
        assert_eq!(resp, decoded);
    }
}

/// SHM size is at least the header and grows linearly with cells.
#[test]
fn shm_size_monotonic() {
    let mut rng = Rng(0x19C0_0003);
    for _ in 0..256 {
        let cols = (rng.next() % 512) as usize;
        let rows = (rng.next() % 512) as usize;
        let size = compute_shm_size(cols, rows).expect("no overflow in range");
        let header = std::mem::size_of::<SharedMemoryHeader>();
        let cell = std::mem::size_of::<FfiTerminalCell>();
        assert!(size >= header);
        assert_eq!(size, header + cols * rows * cell);
        if cols > 0 && rows > 0 {
            let smaller = compute_shm_size(cols - 1, rows).expect("smaller");
            assert!(smaller < size || cols == 1);
        }
    }
}

/// Adversarial dims either validate cleanly or are rejected; size never panics.
#[test]
fn adversarial_dims_never_panic() {
    let mut rng = Rng(0x19C0_0004);
    for _ in 0..256 {
        let c = rng.next() as u32 as usize;
        let r = rng.next() as u32 as usize;
        let _ = validate_grid_dims(c, r);
        let _ = compute_shm_size(c, r);
    }
}

/// Unknown command tags are rejected.
#[test]
fn invalid_command_tags_fail() {
    for tag in 4u8..=255 {
        assert!(IpcCommand::read_from(&[tag][..]).is_err());
    }
}

/// Unknown response tags are rejected.
#[test]
fn invalid_response_tags_fail() {
    for tag in 3u8..=255 {
        assert!(IpcResponse::read_from(&[tag][..]).is_err());
    }
}
