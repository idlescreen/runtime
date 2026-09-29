use super::*;
use std::collections::HashMap;
use zbus::zvariant::Value;

#[test]
fn read_bool_handles_valid_input() {
    let v = Value::Bool(true).try_to_owned().unwrap();
    let mut map = HashMap::new();
    map.insert("flag".to_string(), v);
    assert!(read_bool(&map, "flag"));
}

#[test]
fn read_bool_defaults_false_on_missing_key() {
    let map: HashMap<String, zbus::zvariant::OwnedValue> = HashMap::new();
    assert!(!read_bool(&map, "missing"));
}

#[test]
fn read_bool_defaults_false_on_wrong_type() {
    let v = Value::U32(42).try_to_owned().unwrap();
    let mut map = HashMap::new();
    map.insert("flag".to_string(), v);
    assert!(!read_bool(&map, "flag"));
}

#[test]
fn read_u32_handles_valid_input() {
    let v = Value::U32(42).try_to_owned().unwrap();
    let mut map = HashMap::new();
    map.insert("count".to_string(), v);
    assert_eq!(read_u32(&map, "count"), 42);
}

#[test]
fn read_u32_defaults_zero_on_missing() {
    let map: HashMap<String, zbus::zvariant::OwnedValue> = HashMap::new();
    assert_eq!(read_u32(&map, "missing"), 0);
}

#[test]
fn read_string_handles_valid_input() {
    let v = Value::Str("hello".into()).try_to_owned().unwrap();
    let mut map = HashMap::new();
    map.insert("name".to_string(), v);
    assert_eq!(read_string(&map, "name"), "hello");
}

#[test]
fn read_string_defaults_empty_on_missing() {
    let map: HashMap<String, zbus::zvariant::OwnedValue> = HashMap::new();
    assert_eq!(read_string(&map, "missing"), "");
}

#[test]
fn parse_status_handles_missing_keys() {
    let map: HashMap<String, zbus::zvariant::OwnedValue> = HashMap::new();
    let status = parse_status(map).unwrap();
    assert!(!status.running);
    assert_eq!(status.idle_timeout_mins, 0);
    assert_eq!(status.active_saver, "");
    assert_eq!(status.render_scale, "");
}

#[test]
fn parse_status_reads_known_fields() {
    let mut map = HashMap::new();
    map.insert(
        "running".to_string(),
        Value::Bool(true).try_to_owned().unwrap(),
    );
    map.insert(
        "idle_timeout_mins".to_string(),
        Value::U32(15).try_to_owned().unwrap(),
    );
    map.insert(
        "active_saver".to_string(),
        Value::Str("cosmos".into()).try_to_owned().unwrap(),
    );
    let status = parse_status(map).unwrap();
    assert!(status.running);
    assert_eq!(status.idle_timeout_mins, 15);
    assert_eq!(status.active_saver, "cosmos");
}

#[test]
fn parse_status_full_roundtrip() {
    let mut map = HashMap::new();
    map.insert(
        "running".to_string(),
        Value::Bool(true).try_to_owned().unwrap(),
    );
    map.insert(
        "idle_enabled".to_string(),
        Value::Bool(true).try_to_owned().unwrap(),
    );
    map.insert(
        "idle_timeout_mins".to_string(),
        Value::U32(10).try_to_owned().unwrap(),
    );
    map.insert(
        "active_saver".to_string(),
        Value::Str("beams".into()).try_to_owned().unwrap(),
    );
    map.insert(
        "presentation_active".to_string(),
        Value::Bool(false).try_to_owned().unwrap(),
    );
    map.insert(
        "preview_active".to_string(),
        Value::Bool(true).try_to_owned().unwrap(),
    );
    map.insert(
        "system_idle".to_string(),
        Value::Bool(true).try_to_owned().unwrap(),
    );
    map.insert(
        "session_locked".to_string(),
        Value::Bool(false).try_to_owned().unwrap(),
    );
    map.insert(
        "inhibited".to_string(),
        Value::Bool(false).try_to_owned().unwrap(),
    );
    map.insert(
        "current_saver".to_string(),
        Value::Str("beams".into()).try_to_owned().unwrap(),
    );
    map.insert(
        "gpu_enabled".to_string(),
        Value::Bool(true).try_to_owned().unwrap(),
    );
    map.insert(
        "show_fps_overlay".to_string(),
        Value::Bool(true).try_to_owned().unwrap(),
    );
    map.insert(
        "render_scale".to_string(),
        Value::Str("0.75".into()).try_to_owned().unwrap(),
    );

    let status = parse_status(map).unwrap();
    assert!(status.running);
    assert!(status.idle_enabled);
    assert_eq!(status.idle_timeout_mins, 10);
    assert_eq!(status.active_saver, "beams");
    assert!(!status.presentation_active);
    assert!(status.preview_active);
    assert!(status.system_idle);
    assert!(!status.session_locked);
    assert!(!status.inhibited);
    assert_eq!(status.current_saver, "beams");
    assert!(status.show_fps_overlay);
    assert_eq!(status.render_scale, "0.75");
}
