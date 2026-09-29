// SPDX-License-Identifier: MIT

//! Fuzz tests for the validation layer behind the D-Bus control methods.
//! `set_saver`/`preview` both route names through `sanitize_saver_name` —
//! every byte pattern a hostile caller can send must either be rejected or
//! emerge charset-clean. `set_timeout`/`set_render_scale` guards are pinned
//! by the config-parser fuzz tests (same 1..=240 / finite contract).

use idle_api::LcgRng;
use idle_runner::launcher::sanitize_saver_name;

/// Charset contract: sanitized names contain only [a-zA-Z0-9-].
fn assert_charset_clean(name: &str) {
    assert!(
        name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
        "sanitized name contains disallowed chars: {name:?}"
    );
}

#[test]
fn sanitize_saver_name_hostile_inputs() {
    // Targeted hostile strings: traversal, control bytes, unicode, prefixes.
    let hostile = [
        "",
        "..",
        "../..",
        "../../etc/passwd",
        "/etc/passwd",
        "a/b",
        "a\\b",
        "name\u{0}evil",
        "na\u{7f}me",
        "🔥screensaver",
        "säver",
        "libscreensaver_beams",
        "libscreensaver_",
        "lib",
        "idle-saver-storm",
        "idle-saver-",
        "screensaver-gnats",
        "screensaver-",
        "-",
        "--",
        "-x",
        "x-",
        "a b",
        "a.b",
        ".hidden",
    ];
    let big = ["x".repeat(10_000), "-".repeat(4096), "../".repeat(512)];
    for input in hostile
        .iter()
        .map(|s| s.as_ref())
        .chain(big.iter().map(|s| s.as_str()))
    {
        if let Some(name) = sanitize_saver_name(input) {
            assert!(!name.is_empty(), "empty sanitized name for {input:?}");
            assert_charset_clean(&name);
        }
    }
}

#[test]
fn sanitize_saver_name_seeded_byte_soup() {
    let mut rng = LcgRng::new(0xF00D);
    for _ in 0..20_000 {
        // Random strings over a hostile alphabet: path chars, dots, control.
        const ALPHABET: &[u8] = b"abz-./\\\0\x7f~:_\xF0\x9F\x94\xA5".as_slice();
        let len = rng.next_usize(40);
        let mut s = String::with_capacity(len);
        for _ in 0..len {
            let b = ALPHABET[rng.next_usize(ALPHABET.len())];
            s.push(b as char);
        }
        if let Some(name) = sanitize_saver_name(&s) {
            assert!(!name.is_empty());
            assert_charset_clean(&name);
        }
    }
}
