default: verify

# Bootstrap a clean clone: rustup toolchain + cargo-audit/cargo-deny +
# sibling idle/ symlink + apt dep sanity. Idempotent.
bootstrap:
    ./scripts/bootstrap.sh

# Build a release binary
build:
    cargo build --release --workspace

# Build a release binary (with hard-coded debug info for crash reports)
build-debug:
    cargo build --workspace --profile=release-with-debug

# Run the full test suite
test:
    cargo test --workspace --all-features

# Run doc tests
test-doc:
    cargo test --workspace --doc

# Format check
fmt-check:
    cargo fmt --all -- --check

# Format (apply changes)
fmt:
    cargo fmt --all

# Clippy (deny warnings)
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Documentation build
doc:
    cargo doc --workspace --no-deps --open

# Security audit
audit:
    cargo audit

# License + advisory check
deny:
    cargo deny check

# Find unused dependencies
udeps:
    cargo +nightly udeps --workspace --all-features

# Code coverage (HTML report)
coverage:
    cargo llvm-cov --workspace --all-features --html

# Build distribution packages (deb, rpm).
# Default: headless package gate first (same as package.rs). Skip with SKIP_TESTS=1.
package: qa-package-gate
    ./package.rs

# Verify formatting + lint + tests all pass
verify: fmt-check clippy test test-doc
    @echo "All checks passed."

# Quick CI mirror: lint + test only
ci: fmt-check clippy test
    @echo "CI checks passed."

# Headless package gate (same as package.rs — no Wayland required)
qa-package-gate:
    ./scripts/qa_package_gate.sh

# Host/preview regression units (no display; subset of package gate)
qa-unit:
    cargo test -p idle-daemon -p idle-ipc -p idle-dbus -p wayland-present
    @echo "QA unit regression suite passed."

# Named filters covering morning+preview+fullscreen regressions (docs/QA_REGRESSION.md)
qa-unit-named:
    cargo test -p idle-daemon -p idle-ipc -p idle-dbus -p wayland-present -- \
        doctor_rules inhibitors_fmt ignore_logind merge_drops merge_includes \
        recovery_plan present_cooldown thrash hold_idle exit_process \
        preview_starts idle_decision path_safety hw_scaling \
        frame_geometry layer_not would_block eagain exclusive_zone \
        panel_expand fullscreen_expands geom_tests battery_should \
        format_status status_text status_json
    @echo "QA named regression filters passed."

# Live preview smoke: NRestarts must not rise (needs active idle-daemon + Wayland)
# NOT part of packaging — run after install on a real session.
qa-smoke saver="beams":
    ./scripts/qa_preview_smoke.sh {{saver}}

# Full product closed-loop (install/bus/preview/fault) — needs Wayland session
qa-product:
    ./scripts/qa_product_loop.sh

# Package gate + lighter live smoke
qa: qa-package-gate qa-smoke
    @echo "QA package gate + live smoke passed."

# Package gate + full product closed-loop (release machine)
qa-all: qa-package-gate qa-product
    @echo "QA package gate + product loop passed."

