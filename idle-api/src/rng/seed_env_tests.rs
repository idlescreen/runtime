use super::*;
use std::sync::Mutex;

// Tests in this module mutate process-global env vars. The seed
// lookup tries RENDER_SEED first, so any leaked value (e.g. a
// parallel test setting it) wins over IDLE_SEED. We serialize the
// tests with a Mutex and clear all seed keys on entry to avoid
// the race that bit us when tests ran in parallel.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn clear_all_seed_env() {
    for key in SEED_ENV_KEYS {
        unsafe {
            std::env::remove_var(key);
        }
    }
}

#[test]
fn seed_from_env_decimal() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    clear_all_seed_env();
    unsafe {
        std::env::set_var("RENDER_SEED", "12345");
    }
    assert_eq!(seed_from_env(), Some(12345));
    clear_all_seed_env();
}

#[test]
fn seed_from_env_hex() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    clear_all_seed_env();
    unsafe {
        std::env::set_var("IDLE_SEED", "0x10");
    }
    assert_eq!(seed_from_env(), Some(16));
    clear_all_seed_env();
}
