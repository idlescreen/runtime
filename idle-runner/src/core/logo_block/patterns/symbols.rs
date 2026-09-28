// SPDX-License-Identifier: MIT
// perf: T3 · metric: bounded single-pass work; no syscalls, no locks, no allocation on the steady path · check: review

pub fn pattern(ch: char) -> Option<[&'static str; 5]> {
    match ch {
        '_' => Some([
            "     ",
            "     ",
            "     ",
            "     ",
            "\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}",
        ]),
        '!' => Some([
            "  \u{2588}  ",
            "  \u{2588}  ",
            "  \u{2588}  ",
            "     ",
            "  \u{2588}  ",
        ]),
        ' ' => Some(["     ", "     ", "     ", "     ", "     "]),
        '.' => Some(["     ", "     ", "     ", "     ", "  \u{2588}  "]),
        '-' => Some([
            "     ",
            "     ",
            " \u{2588}\u{2588}\u{2588} ",
            "     ",
            "     ",
        ]),
        _ => Some([
            " \u{2588}\u{2588}\u{2588} ",
            "\u{2588}   \u{2588}",
            "\u{2588}   \u{2588}",
            "\u{2588}   \u{2588}",
            " \u{2588}\u{2588}\u{2588} ",
        ]),
    }
}
