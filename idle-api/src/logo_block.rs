//! 5x5 block-letter logo renderer. Pure string-transformer with a static cache.

fn get_5x5_pattern(ch: char) -> Option<[&'static str; 5]> {
    let u = ch.to_ascii_uppercase();
    match u {
        'A' => Some([" █▀▀█ ", "█   █", "█████", "█   █", "█   █"]),
        'B' => Some(["████  ", "█   █ ", "████  ", "█   █ ", "████  "]),
        'C' => Some([" ████", "█    ", "█    ", "█    ", " ████"]),
        'D' => Some(["████  ", "█   █ ", "█   █ ", "█   █ ", "████  "]),
        'E' => Some(["█████", "█    ", "████ ", "█    ", "█████"]),
        'F' => Some(["█████", "█    ", "████ ", "█    ", "█    "]),
        'G' => Some([" ████", "█    ", "█ ███", "█   █", " ████"]),
        'H' => Some(["█   █", "█   █", "█████", "█   █", "█   █"]),
        'I' => Some(["█████", "  █  ", "  █  ", "  █  ", "█████"]),
        'J' => Some(["    █", "    █", "    █", "█   █", " ███ "]),
        'K' => Some(["█   █", "█  █ ", "███  ", "█  █ ", "█   █"]),
        'L' => Some(["█    ", "█    ", "█    ", "█    ", "█████"]),
        'M' => Some(["█   █", "██ ██", "█ █ █", "█   █", "█   █"]),
        'N' => Some(["█   █", "██  █", "█ █ █", "█  ██", "█   █"]),
        'O' => Some([" ███ ", "█   █", "█   █", "█   █", " ███ "]),
        'P' => Some(["████ ", "█   █", "████ ", "█    ", "█    "]),
        'Q' => Some([" ███ ", "█   █", "█ █ █", "█  █ ", " ████"]),
        'R' => Some(["████ ", "█   █", "████ ", "█  █ ", "█   █"]),
        'S' => Some([" ████", "█    ", " ███ ", "    █", "████ "]),
        'T' => Some(["█████", "  █  ", "  █  ", "  █  ", "  █  "]),
        'U' => Some(["█   █", "█   █", "█   █", "█   █", " ███ "]),
        'V' => Some(["█   █", "█   █", " █ █ ", " █ █ ", "  █  "]),
        'W' => Some(["█   █", "█   █", "█ █ █", "██ ██", "█   █"]),
        'X' => Some(["█   █", " █ █ ", "  █  ", " █ █ ", "█   █"]),
        'Y' => Some(["█   █", " █ █ ", "  █  ", "  █  ", "  █  "]),
        'Z' => Some(["█████", "   █ ", "  █  ", " █   ", "█████"]),
        '0' => Some([" ███ ", "█  ██", "█ █ █", "██  █", " ███ "]),
        '1' => Some(["  █  ", " ██  ", "  █  ", "  █  ", "█████"]),
        '2' => Some([" ███ ", "█   █", "   █ ", " █   ", "█████"]),
        '3' => Some(["████ ", "    █", " ███ ", "    █", "████ "]),
        '4' => Some(["█   █", "█   █", "█████", "    █", "    █"]),
        '5' => Some(["█████", "█    ", "████ ", "    █", "████"]),
        '6' => Some([" ███ ", "█    ", "████ ", "█   █", " ███ "]),
        '7' => Some(["█████", "    █", "   █ ", "  █  ", "  █  "]),
        '8' => Some([" ███ ", "█   █", " ███ ", "█   █", " ███ "]),
        '9' => Some([" ███ ", "█   █", " ████", "    █", " ███ "]),
        '_' => Some(["     ", "     ", "     ", "     ", "█████"]),
        '!' => Some(["  █  ", "  █  ", "  █  ", "     ", "  █  "]),
        ' ' => Some(["     ", "     ", "     ", "     ", "     "]),
        '.' => Some(["     ", "     ", "     ", "     ", "  █  "]),
        '-' => Some(["     ", "     ", " ███ ", "     ", "     "]),
        _ => Some([" ███ ", "█   █", "█   █", "█   █", " ███ "]),
    }
}

type LogoCacheEntry = (String, Option<String>, Vec<String>);

/// Renders the live centered logo block.
pub fn render_logo_block(text: &str, sub_text: Option<&str>) -> Vec<String> {
    static CACHE: std::sync::RwLock<Option<LogoCacheEntry>> = std::sync::RwLock::new(None);
    if let Ok(read_guard) = CACHE.read()
        && let Some(entry) = read_guard.as_ref()
        && entry.0 == text
        && entry.1.as_deref() == sub_text
    {
        return entry.2.clone();
    }
    let mut lock = CACHE.write().unwrap_or_else(|p| p.into_inner());
    if let Some(entry) = lock.as_ref()
        && entry.0 == text
        && entry.1.as_deref() == sub_text
    {
        return entry.2.clone();
    }

    let chars: Vec<char> = text.chars().collect();
    let mut rows: Vec<String> = vec![String::new(); 5];
    for ch in &chars {
        let pattern = get_5x5_pattern(*ch).unwrap_or(["     "; 5]);
        for (i, line) in pattern.iter().enumerate() {
            rows[i].push_str(line);
            rows[i].push(' ');
        }
    }

    if let Some(sub) = sub_text {
        rows.push(String::new());
        rows.push(sub.to_string());
    }

    let out = rows;
    *lock = Some((text.to_string(), sub_text.map(String::from), out.clone()));
    out
}
