// SPDX-License-Identifier: MIT

//! Pure policy: which ScreenSaver D-Bus interfaces we may *sniff*.
//!
//! We implement `org.freedesktop.ScreenSaver` ourselves. Sniffing those
//! method calls double-counts Inhibit (real cookie + phantom 10000+ cookie).
//! Firefox only UnInhibits the real cookie → stale "Playing video" forever.
//!
//! Regression filters: `freeness_screensaver_must_not_be_sniffed`,
//! `firefox_double_count_uninhibit_leaves_phantom`, `firefox_prune_after_exit`.

/// Return true only if bus method-call sniffing is allowed for this interface.
pub fn may_sniff_screensaver_interface(interface: &str) -> bool {
    match interface {
        // Optional legacy path some GNOME clients still use.
        "org.gnome.ScreenSaver" => true,
        _ => false,
    }
}

/// Interfaces the external sniffer is allowed to subscribe to.
pub fn sniffable_screensaver_interfaces() -> &'static [&'static str] {
    &["org.gnome.ScreenSaver"]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freeness_screensaver_must_not_be_sniffed() {
        // Regression: double-count with ScreenSaverService caused 100+ phantom
        // Firefox holds after browser exit (cookies 10000+).
        assert!(
            !may_sniff_screensaver_interface("org.freedesktop.ScreenSaver"),
            "owning freeness.ScreenSaver forbids bus sniffing"
        );
    }

    #[test]
    fn gnome_screensaver_may_be_sniffed() {
        assert!(may_sniff_screensaver_interface("org.gnome.ScreenSaver"));
    }

    #[test]
    fn unknown_interface_not_sniffed() {
        assert!(!may_sniff_screensaver_interface("org.example.ScreenSaver"));
        assert!(!may_sniff_screensaver_interface(""));
    }

    #[test]
    fn sniffable_list_never_includes_freeness() {
        for iface in sniffable_screensaver_interfaces() {
            assert_ne!(*iface, "org.freedesktop.ScreenSaver");
            assert!(may_sniff_screensaver_interface(iface));
        }
        assert!(
            !sniffable_screensaver_interfaces().contains(&"org.freedesktop.ScreenSaver"),
            "sniffable list must never reintroduce freeness.ScreenSaver"
        );
    }
}
