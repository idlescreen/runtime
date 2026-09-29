/// Environment keys for deterministic offline export (`render`).
pub const SEED_ENV_KEYS: &[&str] = &["RENDER_SEED", "IDLE_RENDER_SEED", "IDLE_SEED"];

/// Parse a seed from the process environment, if set and valid.
pub fn seed_from_env() -> Option<u64> {
    for key in SEED_ENV_KEYS {
        if let Ok(raw) = std::env::var(key) {
            let s = raw.trim();
            if s.is_empty() {
                continue;
            }
            if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                if let Ok(v) = u64::from_str_radix(hex, 16) {
                    return Some(v);
                }
            } else if let Ok(v) = s.parse::<u64>() {
                return Some(v);
            }
        }
    }
    None
}

/// Linear Congruential Generator. Deterministic, lock-free.
///
/// # Example
///
/// ```
/// use idle_api::LcgRng;
/// let mut rng = LcgRng::new(42);
/// let n = rng.next_range(0.0, 10.0);
/// assert!(n >= 0.0 && n <= 10.0);
/// ```
#[derive(Clone, Debug)]
pub struct LcgRng(u64);

impl LcgRng {
    pub fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    pub fn new_random() -> Self {
        // wasm32-unknown-unknown has no clock — a fixed seed keeps the
        // generator deterministic instead of panicking.
        #[cfg(target_arch = "wasm32")]
        return Self::new(0x9E37_79B9_7F4A_7C15);
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::time::SystemTime;
            let seed = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(1);
            Self::new(seed)
        }
    }

    /// Prefer `RENDER_SEED` / `IDLE_RENDER_SEED` / `IDLE_SEED` (decimal or 0x-hex); else random.
    pub fn from_env_or_random() -> Self {
        match seed_from_env() {
            Some(s) => Self::new(s),
            None => Self::new_random(),
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    pub fn next_f32(&mut self) -> f32 {
        let val = (self.next_u64() >> 40) as u32;
        (val as f32) * (1.0 / (1u32 << 24) as f32)
    }

    pub fn next_range(&mut self, min: f32, max: f32) -> f32 {
        min + self.next_f32() * (max - min)
    }

    pub fn next_usize(&mut self, max: usize) -> usize {
        if max == 0 {
            return 0;
        }
        (self.next_u64() % max as u64) as usize
    }

    pub fn next_bool(&mut self, prob: f32) -> bool {
        self.next_f32() < prob
    }
}

#[cfg(test)]
mod proptests;
#[cfg(test)]
mod seed_env_tests;
#[cfg(test)]
mod tests;
