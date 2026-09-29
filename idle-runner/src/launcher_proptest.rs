//! Property tests for plugin name sanitization and allowlist policy.

use super::{ALLOWED_SAVERS, is_allowed_saver, sanitize_saver_name};

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
}

/// Random byte string over a hostile charset (path separators, dots,
/// control chars, UTF-8 edges) — the old `.*` proptest strategy.
fn arb_any_string(r: &mut Rng) -> String {
    const CHARSET: &[char] = &[
        'a', 'b', 'z', 'A', 'Z', '0', '9', '-', '_', '.', '/', '\\', ' ', '\t', '\n', '\0', ':',
        ';', '\'', '"', '$', '~', 'é', '中', '🚀', '\u{1}',
    ];
    let len = (r.next() % 33) as usize;
    (0..len)
        .map(|_| CHARSET[(r.next() as usize) % CHARSET.len()])
        .collect()
}

/// Random lowercase `[a-z]{1,8}` name — the old alpha strategy.
fn arb_alpha(r: &mut Rng) -> String {
    let len = 1 + (r.next() % 8) as usize;
    (0..len)
        .map(|_| (b'a' + (r.next() % 26) as u8) as char)
        .collect()
}

#[test]
fn sanitize_never_returns_path_separators() {
    let mut rng = Rng(0x5A17_1E51);
    for s in (0..64).map(|_| arb_any_string(&mut rng)) {
        if let Some(clean) = sanitize_saver_name(&s) {
            assert!(!clean.contains('/'));
            assert!(!clean.contains('\\'));
            assert!(
                clean.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
                "sanitized name {clean:?} must be [a-zA-Z0-9-]+"
            );
        }
    }
}

#[test]
fn allowlist_members_are_allowed() {
    let mut rng = Rng(0x5A17_1E52);
    for _ in 0..64 {
        let name = ALLOWED_SAVERS[(rng.next() as usize) % ALLOWED_SAVERS.len()];
        assert!(is_allowed_saver(name));
        let cleaned = sanitize_saver_name(name);
        assert_eq!(cleaned.as_deref(), Some(name));
    }
}

#[test]
fn path_like_names_not_allowed() {
    let mut rng = Rng(0x5A17_1E53);
    for _ in 0..64 {
        let s = arb_alpha(&mut rng);
        let parent = format!("../{s}");
        let abs = format!("/tmp/{s}");
        assert!(!is_allowed_saver(&parent));
        assert!(!is_allowed_saver(&abs));
    }
}

#[test]
fn libscreensaver_prefix_strips() {
    let mut rng = Rng(0x5A17_1E54);
    let pool = ["beams", "storm", "radar", "hearth"];
    for _ in 0..64 {
        let s = pool[(rng.next() as usize) % pool.len()];
        let raw = format!("libscreensaver_{s}.so");
        let cleaned = sanitize_saver_name(&raw);
        assert_eq!(cleaned.as_deref(), Some(s));
    }
}
