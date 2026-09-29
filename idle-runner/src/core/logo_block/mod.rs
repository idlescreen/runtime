//! 5x5 block-letter logo renderer. Pure string-transformer with a static
//! cache — fits in `core` (no `interface` / `role` dependencies) so both
//! the console app effects (interface layer) and the r* screensaver effects
//! (role layer) can call it without violating the 4-layer taxonomy.

mod patterns;

type LogoCacheEntry = (String, Option<String>, Vec<String>);

/// Renders the live centered logo block (logo_text as big block letters
/// using the 5x5 font above + optional sub_text line underneath).
/// Perfect for retro console effects and dashboards.
pub fn render_logo_block(text: &str, sub_text: Option<&str>) -> Vec<String> {
    static CACHE: std::sync::Mutex<Option<LogoCacheEntry>> = std::sync::Mutex::new(None);
    let mut lock = CACHE.lock().unwrap_or_else(|e| {
        idle_log::error!("mutex poisoned: {e}");
        std::process::abort()
    });
    if let Some((cached_text, cached_sub, cached_val)) = &*lock
        && cached_text == text
        && cached_sub.as_deref() == sub_text
    {
        return cached_val.clone();
    }

    let chars: Vec<char> = text.chars().collect();
    let mut rows: Vec<String> = vec![String::new(); 5];
    for ch in &chars {
        let pattern = patterns::get_5x5_pattern(*ch).unwrap_or(["     "; 5]);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_uppercase_ascii() {
        let lines = render_logo_block("HELLO", None);
        assert_eq!(lines.len(), 5);
        assert!(lines[0].contains('\u{2588}'));
    }

    #[test]
    fn renders_sub_text() {
        let lines = render_logo_block("OS", Some("fedora 40"));
        assert_eq!(lines.len(), 7);
        assert_eq!(lines[6], "fedora 40");
    }

    #[test]
    fn caches_identical_input() {
        let a = render_logo_block("TEST", None);
        let b = render_logo_block("TEST", None);
        assert_eq!(a, b);
    }
}
