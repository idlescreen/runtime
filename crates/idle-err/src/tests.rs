use super::*;

#[test]
fn msg_displays_text() {
    let e = Error::msg("boom");
    assert_eq!(format!("{e}"), "boom");
    assert_eq!(format!("{e:#}"), "boom");
}

#[test]
fn context_chains_like_anyhow() {
    // anyhow: `{}` shows only the outermost context; `{:#}` walks the chain.
    let leaf = std::io::Error::new(std::io::ErrorKind::NotFound, "no file");
    let e = Error::new(leaf)
        .context("reading config")
        .context("loading daemon settings");
    assert_eq!(format!("{e}"), "loading daemon settings");
    assert_eq!(
        format!("{e:#}"),
        "loading daemon settings: reading config: no file"
    );
}

#[test]
fn downcast_finds_source_in_chain() {
    let leaf = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
    let e = Error::new(leaf).context("outer").context("outermost");
    let io_err = e
        .downcast_ref::<std::io::Error>()
        .expect("io::Error in chain");
    assert_eq!(io_err.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(e.downcast_ref::<std::fmt::Error>().is_none());
}

#[test]
fn context_trait_on_std_result() {
    let r: std::result::Result<(), std::io::Error> = Err(std::io::Error::other("low"));
    let e = r.context("high").unwrap_err();
    assert_eq!(format!("{e:#}"), "high: low");
}

#[test]
fn with_context_is_lazy() {
    let ok: std::result::Result<u32, std::io::Error> = Ok(7);
    let mut called = false;
    let v = ok
        .with_context(|| {
            called = true;
            "should not run".to_string()
        })
        .unwrap();
    assert_eq!(v, 7);
    assert!(!called, "context closure must not run on Ok");
}

#[test]
fn context_trait_on_option() {
    let none: Option<u32> = None;
    let e = none.context("was empty").unwrap_err();
    assert_eq!(format!("{e}"), "was empty");

    let some: Option<u32> = Some(3);
    assert_eq!(some.context("unused").unwrap(), 3);
}

#[test]
fn context_trait_on_own_result_rewraps() {
    let r: Result<()> = Err(Error::msg("base"));
    let e = r.context("wrapped").unwrap_err();
    assert_eq!(format!("{e:#}"), "wrapped: base");
}

#[test]
fn anyhow_macro_formats() {
    let n = 41;
    let e = anyhow!("value {n} is off by one");
    assert_eq!(format!("{e}"), "value 41 is off by one");
}

#[test]
fn bail_early_returns() {
    fn f() -> Result<()> {
        bail!("stop {0}", 3);
    }
    assert_eq!(format!("{}", f().unwrap_err()), "stop 3");
}

#[test]
fn ensure_both_forms() {
    fn check(x: u32) -> Result<u32> {
        ensure!(x > 0);
        ensure!(x < 100, "x too big: {x}");
        Ok(x)
    }
    assert_eq!(check(5).unwrap(), 5);
    assert_eq!(
        format!("{}", check(0).unwrap_err()),
        "condition failed: `x > 0`"
    );
    assert_eq!(format!("{}", check(200).unwrap_err()), "x too big: 200");
}

#[test]
fn from_any_std_error() {
    let e: Error = std::io::Error::new(std::io::ErrorKind::TimedOut, "slow").into();
    assert_eq!(format!("{e}"), "slow");
}
