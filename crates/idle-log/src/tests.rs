use super::*;

// `init` mutates the global threshold AND reads RUST_LOG — every env-
// touching test here must hold the same lock so they can't interleave.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn level_filtering_follows_rust_log_and_default() {
    let _g = ENV_LOCK.lock().unwrap();
    // SAFETY: serialized by ENV_LOCK.
    unsafe { std::env::remove_var("RUST_LOG") };

    init("debug");
    assert!(enabled(Level::Error));
    assert!(enabled(Level::Debug));
    assert!(!enabled(Level::Trace));

    init("warn");
    assert!(enabled(Level::Warn));
    assert!(!enabled(Level::Info));

    init("off");
    assert!(!enabled(Level::Error));

    // target=level list: max enabled level wins, like EnvFilter.
    init("zbus=error,idle_daemon=trace");
    assert!(enabled(Level::Trace));

    init("bogus-garbage");
    assert!(!enabled(Level::Error), "unparseable spec mutes all");

    init("info");
}

#[test]
fn rust_log_env_overrides_default() {
    let _g = ENV_LOCK.lock().unwrap();
    // SAFETY: serialized by ENV_LOCK — this test and the one above both
    // mutate RUST_LOG and would race under parallel test threads.
    unsafe { std::env::set_var("RUST_LOG", "trace") };
    init("error");
    assert!(enabled(Level::Trace), "env must beat the default");
    unsafe { std::env::remove_var("RUST_LOG") };
    init("info");
}

#[test]
fn plain_format_message() {
    assert_eq!(__log_msg!("hello {}", 5), "hello 5");
    assert_eq!(__log_msg!("just text"), "just text");
}

#[test]
fn display_sigil_field() {
    let code = 42;
    assert_eq!(__log_msg!(x = %code, "done"), "x = 42, done");
}

#[test]
fn debug_sigil_field() {
    let name = "srv";
    assert_eq!(__log_msg!(name = ?name, "up"), "name = \"srv\", up");
}

#[test]
fn bare_field_uses_display() {
    let n = 7u32;
    assert_eq!(__log_msg!(count = n, "items"), "count = 7, items");
}

#[test]
fn shorthand_fields() {
    let port = 8080;
    assert_eq!(__log_msg!(port, "listening"), "port = 8080, listening");
    assert_eq!(__log_msg!(%port, "listening"), "port = 8080, listening");
    assert_eq!(__log_msg!(?port, "listening"), "port = 8080, listening");
}

#[test]
fn multiple_fields_then_message() {
    let path = "/tmp/x";
    assert_eq!(
        __log_msg!(plugin = %path, rules = 3, "loaded"),
        "plugin = /tmp/x, rules = 3, loaded"
    );
}

#[test]
fn target_override_is_dropped() {
    // tracing `target:` is accepted for compatibility but the emitted
    // record keeps module_path — only the message text survives.
    assert_eq!(__log_msg!(target: "custom::target", "hello"), "hello");
}
