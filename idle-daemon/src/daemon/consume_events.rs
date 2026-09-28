// SPDX-License-Identifier: MIT

// perf: T2 · bench: draw_frame · on-demand only; not gated
//! Parse an inotify read buffer into a count of whole records.

/// Parse an inotify read buffer. Currently only consumes whole records —
/// we don't care *which* file changed, only *that something* did. Returns
/// the number of events seen so the caller can decide whether to refresh
/// the cache.
///
/// `pub` (not `pub(crate)`) solely so `daemon/mod.rs::bench_exports` can
/// re-export it to the `draw_frame` bench target. See RULES.md §5.
pub fn consume_events(buf: &[u8]) -> usize {
    let mut off = 0usize;
    let mut n = 0usize;
    while off + 16 <= buf.len() {
        let len = u32::from_ne_bytes([buf[off + 12], buf[off + 13], buf[off + 14], buf[off + 15]])
            as usize;
        let name_end = off + 16 + len;
        if name_end > buf.len() {
            break;
        }
        n += 1;
        let _ = &buf[off..off + 16]; // header read
        off = name_end;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::consume_events;

    /// Build a synthetic inotify record (header + name + padding) so
    /// the parser sees a well-formed entry.
    fn synth_record(name: &[u8]) -> Vec<u8> {
        let mut buf = vec![0u8; 16 + name.len()];
        // wd (u32 LE), mask (u32 LE), cookie (u32 LE), len (u32 LE)
        buf[8..12].copy_from_slice(&(name.len() as u32).to_le_bytes());
        buf[16..16 + name.len()].copy_from_slice(name);
        buf
    }

    #[test]
    fn empty_buffer_yields_zero_events() {
        assert_eq!(consume_events(&[]), 0);
    }

    #[test]
    fn short_buffer_yields_zero_events() {
        // Header is 16 bytes; a 15-byte buffer is "no records".
        assert_eq!(consume_events(&[0u8; 15]), 0);
    }

    #[test]
    fn one_complete_record_yields_one_event() {
        let rec = synth_record(b"AC0");
        assert_eq!(consume_events(&rec), 1);
    }

    #[test]
    fn two_concatenated_records_yield_two_events() {
        let mut buf = synth_record(b"AC0");
        buf.extend(synth_record(b"BAT0"));
        assert_eq!(consume_events(&buf), 2);
    }

    #[test]
    fn truncated_trailing_record_does_not_overcount() {
        // First record is complete; the second is truncated (header
        // present but name length exceeds buffer).
        let mut buf = synth_record(b"AC0");
        // Append a 16-byte header that claims 32 bytes of name.
        buf.extend_from_slice(&[0u8; 16]);
        // The len field sits at bytes 12..16 of the trailing header,
        // which is buf[trailer_start..trailer_start + 16]. After the
        // first synth_record(b"AC0") + 16 zero bytes, the trailing
        // header begins at offset 19 — len field at buf[31..35].
        buf[31..35].copy_from_slice(&32u32.to_le_bytes());
        assert_eq!(consume_events(&buf), 1);
    }
}
