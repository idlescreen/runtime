//! Environment helpers for IdleScreen host and plugins.
//!
//! Protocol keys are `IDLE_*` only (hard cut; no `TRANCE_*` dual-read).

/// First non-empty value among `keys` (left to right).
pub fn env_var_first(keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Ok(v) = std::env::var(key)
            && !v.is_empty()
        {
            return Some(v);
        }
    }
    None
}

/// True if any of `keys` is set (including empty string).
pub fn env_is_set(keys: &[&str]) -> bool {
    keys.iter().any(|k| std::env::var_os(k).is_some())
}

/// True if any key equals `"1"`, `"true"`, or `"TRUE"` (case-sensitive for true).
pub fn env_truthy(keys: &[&str]) -> bool {
    matches!(
        env_var_first(keys).as_deref(),
        Some("1") | Some("true") | Some("TRUE")
    )
}

/// Set a single environment key (plugin spawn / host setup).
///
/// # Safety
/// Same constraints as [`std::env::set_var`]: not concurrent with other
/// env access from other threads in undefined ways; used at plugin spawn.
pub fn set_env(key: &str, value: impl AsRef<std::ffi::OsStr>) {
    let v = value.as_ref();
    // SAFETY: host single-threaded spawn path / export setup.
    unsafe {
        std::env::set_var(key, v);
    }
}

/// Env prefix for `[saver]` config params delivered to the runner.
pub const SAVER_PARAM_ENV_PREFIX: &str = "IDLE_SAVER_PARAM_";

/// Env var name for a saver param key (`fire.size` → `IDLE_SAVER_PARAM_FIRE_SIZE`).
/// Returns `None` when the key contains no alphanumeric characters.
pub fn saver_param_env_key(key: &str) -> Option<String> {
    let mut name = String::with_capacity(SAVER_PARAM_ENV_PREFIX.len() + key.len());
    name.push_str(SAVER_PARAM_ENV_PREFIX);
    let mut has_alnum = false;
    for ch in key.chars() {
        if ch.is_ascii_alphanumeric() {
            has_alnum = true;
            name.push(ch.to_ascii_uppercase());
        } else {
            name.push('_');
        }
    }
    has_alnum.then_some(name)
}

/// A `[saver]` config param, or `None` when unset. Keys like `fire.size`
/// read `IDLE_SAVER_PARAM_FIRE_SIZE` from the environment.
pub fn param(key: &str) -> Option<String> {
    let name = saver_param_env_key(key)?;
    std::env::var(&name).ok().filter(|v| !v.is_empty())
}

/// A `[saver]` config param parsed as `f32`, or `None` when unset/malformed.
pub fn param_f32(key: &str) -> Option<f32> {
    param(key)?.parse::<f32>().ok().filter(|v| v.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Env is process-global — serialize every mutating test in this file.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn first_prefers_left() {
        let _g = ENV_LOCK.lock().unwrap();
        // SAFETY: serialized by ENV_LOCK; keys are test-only.
        unsafe {
            std::env::set_var("IDLE_TEST_A", "new");
            std::env::set_var("IDLE_TEST_B", "old");
        }
        assert_eq!(
            env_var_first(&["IDLE_TEST_A", "IDLE_TEST_B"]).as_deref(),
            Some("new")
        );
        unsafe {
            std::env::remove_var("IDLE_TEST_A");
            std::env::remove_var("IDLE_TEST_B");
        }
    }

    #[test]
    fn param_env_key_sanitizes() {
        assert_eq!(
            saver_param_env_key("hearth.fire_size").as_deref(),
            Some("IDLE_SAVER_PARAM_HEARTH_FIRE_SIZE")
        );
        assert_eq!(
            saver_param_env_key("glow").as_deref(),
            Some("IDLE_SAVER_PARAM_GLOW")
        );
        // No alphanumeric chars → no usable env name.
        assert_eq!(saver_param_env_key("..."), None);
    }

    #[test]
    fn param_round_trips_and_parses() {
        let _g = ENV_LOCK.lock().unwrap();
        // SAFETY: serialized by ENV_LOCK; key is test-only.
        unsafe { std::env::set_var("IDLE_SAVER_PARAM_HEARTH_FIRE_SIZE", "1.5") };
        assert_eq!(param("hearth.fire_size").as_deref(), Some("1.5"));
        assert_eq!(param_f32("hearth.fire_size"), Some(1.5));
        unsafe { std::env::remove_var("IDLE_SAVER_PARAM_HEARTH_FIRE_SIZE") };
        assert_eq!(param("hearth.fire_size"), None);
        assert_eq!(param_f32("hearth.fire_size"), None);
    }

    #[test]
    fn param_f32_rejects_garbage() {
        let _g = ENV_LOCK.lock().unwrap();
        // SAFETY: serialized by ENV_LOCK; key is test-only.
        unsafe { std::env::set_var("IDLE_SAVER_PARAM_X", "not_a_float") };
        assert_eq!(param_f32("x"), None);
        unsafe { std::env::set_var("IDLE_SAVER_PARAM_X", "inf") };
        assert_eq!(param_f32("x"), None); // non-finite rejected
        unsafe { std::env::remove_var("IDLE_SAVER_PARAM_X") };
    }
}
