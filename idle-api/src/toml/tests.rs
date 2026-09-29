use super::*;

#[test]
fn parses_manifest_shape() {
    let text = r#"
# comment
schema_version = 1
plugin_id      = "io.idlescreen.saver.Aurora"
plugin_version = "2.4.0"
api_version    = 1

[entry]
runtime = "native"
library = "libscreensaver_aurora.so"

[capabilities]
network          = false
filesystem_read  = []
filesystem_write = ["/tmp/x", "/opt/y"]

[dependencies]
native = ["libc6"]
wasm   = []

[headless_render]
default_fps        = 60
deterministic_seed = true
gpu_optional       = true
"#;
    let doc = parse(text).unwrap();
    assert_eq!(doc.get("schema_version").unwrap().as_int(), Some(1));
    let entry = doc.get("entry").unwrap();
    assert_eq!(entry.get("runtime").unwrap().as_str(), Some("native"));
    let caps = doc.get("capabilities").unwrap();
    assert_eq!(caps.get("network").unwrap().as_bool(), Some(false));
    let fw = caps.get("filesystem_write").unwrap().as_array().unwrap();
    assert_eq!(fw.len(), 2);
    assert_eq!(fw[0].as_str(), Some("/tmp/x"));
}

#[test]
fn rejects_junk() {
    assert!(parse("key = ").is_err());
    assert!(parse("[unclosed").is_err());
    assert!(parse("a = 1\na = 2").is_err());
}

#[test]
fn string_escapes_decode() {
    let doc = parse("a = \"x\\ny\\t\\\"z\\\" \\\\ \\u0041\"").unwrap();
    assert_eq!(doc.get("a").unwrap().as_str(), Some("x\ny\t\"z\" \\ A"));
}

#[test]
fn string_rejects_bad_escape_and_control_chars() {
    assert!(parse("a = \"x\\q\"").is_err(), "invalid escape");
    assert!(parse("a = \"x\\uD800\"").is_err(), "surrogate codepoint");
    assert!(parse("a = \"x\u{0001}y\"").is_err(), "control char");
    assert!(parse("a = \"unterminated").is_err());
}

#[test]
fn literal_strings_take_chars_verbatim() {
    let doc = parse("a = 'C:\\new\\path'").unwrap();
    assert_eq!(doc.get("a").unwrap().as_str(), Some("C:\\new\\path"));
    assert!(parse("a = 'unterminated").is_err());
}

#[test]
fn arrays_multiline_comments_and_trailing_comma() {
    let doc = parse("a = [\n  \"x\", # first\n  \"y\",\n]\nb = [1, 2,]\nc = []\n").unwrap();
    let a = doc.get("a").unwrap().as_array().unwrap();
    assert_eq!(a.len(), 2);
    assert_eq!(a[1].as_str(), Some("y"));
    assert_eq!(doc.get("b").unwrap().as_array().unwrap().len(), 2);
    assert_eq!(doc.get("c").unwrap().as_array().unwrap().len(), 0);
}

#[test]
fn array_missing_comma_is_error() {
    assert!(parse("a = [\"x\" \"y\"]").is_err());
    assert!(parse("a = [\"x\"").is_err(), "unterminated");
}

#[test]
fn integers_signs_underscores_and_bounds() {
    let doc = parse("a = +42\nb = -7\nc = 1_000_000\nd = 0").unwrap();
    assert_eq!(doc.get("a").unwrap().as_int(), Some(42));
    assert_eq!(doc.get("b").unwrap().as_int(), Some(-7));
    assert_eq!(doc.get("c").unwrap().as_int(), Some(1_000_000));
    assert!(parse("a = 99999999999999999999").is_err(), "i64 overflow");
    assert!(parse("a = 0x10").is_err(), "hex rejected");
    assert!(parse("a = 1.5").is_err(), "floats rejected");
}

#[test]
fn tables_can_interleave_root_keys() {
    // Root keys after a [table] belong to the table, not root.
    let doc = parse("root1 = 1\n[t]\nk = 2\nroot2 = 3\n").unwrap();
    assert_eq!(doc.get("root1").unwrap().as_int(), Some(1));
    assert!(doc.get("root2").is_none(), "goes to [t], not root");
    assert_eq!(
        doc.get("t").unwrap().get("root2").unwrap().as_int(),
        Some(3)
    );
}

#[test]
fn duplicate_table_and_key_rejected() {
    assert!(parse("[t]\na=1\n[t]\nb=2").is_err(), "dup table");
    assert!(parse("[t]\na=1\na=2").is_err(), "dup key in table");
    let err = parse("a = 1\n[t]\nb = 2\n[t]\nc = 3").unwrap_err();
    assert!(format!("{err}").contains("duplicate table"));
}

#[test]
fn dotted_keys_rejected() {
    // `a.b` is a nested-table key in real TOML — outside this subset,
    // so it's an error rather than silently stored as a flat name.
    let err = parse("a.b = 1").unwrap_err();
    assert!(format!("{err}").contains("dotted"), "{err}");
    assert!(parse("a.b.c = 1").is_err());
}

#[test]
fn malformed_inputs_never_panic() {
    // Every prefix of a valid manifest — Err or Ok, never panic.
    let good =
        "[entry]\nlibrary = \"x.so\"\n[caps]\nnet = false\nlist = [\"a\", \"b\"]\nn = -4_2\n";
    for i in 0..=good.len() {
        let _ = parse(&good[..i]);
    }
    for bad in [
        "",
        "[",
        "]",
        "[a",
        "a",
        "=",
        "a =",
        "a = [",
        "a = [1",
        "\"",
        "'",
        "a = \"\\",
        "a = \"\\u",
        "a = \"\\uZZZZ\"",
        "[[]]",
        "[a]",
        "[[a]]",
        "a = {",
        "a = 0x",
        "a = 18446744073709551616",
        "a = +",
        "a = -",
        "\u{feff}a = 1",
        "a = '''x",
        "a = t",
        "a = truex",
        "a = \"x\"\ny",
        ".a = 1",
        "a. = 1",
        "- = 1",
        "_ = 1",
    ] {
        let _ = parse(bad);
    }
}

#[test]
fn inline_tables_and_dates_rejected() {
    assert!(parse("a = {x = 1}").is_err(), "inline table");
    assert!(parse("a = 2024-01-01").is_err(), "date");
}

#[test]
fn error_reports_line_and_column() {
    let err = parse("a = 1\nbad!! = 2").unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("line 2"), "got: {msg}");
    let err2 = parse("ok = 1\n\n\nx = {").unwrap_err();
    assert!(format!("{err2}").contains("line 4"));
}

#[test]
fn crlf_line_endings_accepted() {
    let doc = parse("a = 1\r\nb = \"x\"\r\n").unwrap();
    assert_eq!(doc.get("a").unwrap().as_int(), Some(1));
}

#[test]
fn value_accessors_return_none_on_wrong_type() {
    let doc = parse("s = \"x\"\ni = 1\nb = true\na = [1]").unwrap();
    assert!(doc.get("s").unwrap().as_int().is_none());
    assert!(doc.get("i").unwrap().as_str().is_none());
    assert!(doc.get("b").unwrap().as_array().is_none());
    assert!(doc.get("a").unwrap().as_bool().is_none());
    assert!(doc.get("missing").is_none());
}

#[test]
fn comment_only_and_empty_documents() {
    assert_eq!(parse("").unwrap().as_table().unwrap().len(), 0);
    assert_eq!(
        parse("# just a comment\n\n# another\n")
            .unwrap()
            .as_table()
            .unwrap()
            .len(),
        0
    );
}
