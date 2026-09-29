#!/usr/bin/env cargo
//! ```cargo
//! [dependencies]
//! ```

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

const CRATES: &[&str] = &["idle-daemon"];

fn run_cmd(cmd: &mut Command) -> Result<(), String> {
    let status = cmd.status().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("Command failed with exit status: {}", status));
    }
    Ok(())
}

fn get_version() -> Result<String, String> {
    let content = fs::read_to_string("idle-daemon/Cargo.toml").map_err(|e| e.to_string())?;
    for line in content.lines() {
        if line.starts_with("version =") {
            let parts: Vec<&str> = line.split('"').collect();
            if parts.len() >= 2 {
                return Ok(parts[1].to_string());
            }
        }
    }
    Err("Could not find version in idle-daemon/Cargo.toml".to_string())
}

fn skip_tests() -> bool {
    matches!(
        std::env::var("SKIP_TESTS").as_deref(),
        Ok("1") | Ok("true") | Ok("yes") | Ok("TRUE") | Ok("YES")
    )
}

fn dry_run() -> bool {
    matches!(
        std::env::var("IDLE_PACKAGE_DRY_RUN").as_deref(),
        Ok("1") | Ok("true") | Ok("yes") | Ok("TRUE") | Ok("YES")
    )
}


/// Headless package gate — must pass before shipping packages.
/// See `scripts/qa_package_gate.sh` and `docs/QA_REGRESSION.md`.
fn run_qa_unit_gate() -> Result<(), String> {
    let script = Path::new("scripts/qa_package_gate.sh");
    if script.is_file() {
        println!("QA package gate (headless): scripts/qa_package_gate.sh");
        run_cmd(Command::new("bash").arg(script))?;
        return Ok(());
    }
    // Fallback if script missing (partial checkout).
    println!("QA gate fallback: core host crates");
    run_cmd(Command::new("cargo").args([
        "test",
        "-p",
        "idle-daemon",
        "-p",
        "idle-ipc",
        "-p",
        "idle-dbus",
        "-p",
        "idle-upscaler",
        "-p",
        "idle-runner",
        "-p",
        "idle-api",
        "-p",
        "wayland-present",
        "-p",
        "wayland-idle",
    ]))?;
    println!("QA unit gate passed.");
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("==========================================");
    println!("Building All IdleScreen Packages via Rust...");
    println!("==========================================");

    // Clean stale packaging directories to avoid copying old versions
    if dry_run() {
        println!("[dry-run] would clean: target/debian, target/generate-rpm");
    } else {
        let _ = fs::remove_dir_all("target/debian");
        let _ = fs::remove_dir_all("target/generate-rpm");
    }

    // Ensure path to cargo bin
    let home = std::env::var("HOME")?;
    let path = std::env::var("PATH")?;
    let cargo_bin = format!("{}/.cargo/bin", home);
    if !path.contains(&cargo_bin) {
        // SAFETY: single-threaded QA-gate setup before any other threads exist.
        unsafe {
            std::env::set_var("PATH", format!("{}:{}", cargo_bin, path));
        }
    }

    if skip_tests() {
        println!("SKIP_TESTS set — skipping QA unit gate (not for release cuts).");
    } else {
        run_qa_unit_gate()?;
    }

    println!("Compiling release binaries...");
    run_cmd(Command::new("cargo").args(["build", "--release"]))?;

    let apt_pool = Path::new("../packages/apt/pool/main");
    let rpm_pool = Path::new("../packages/rpm/pool");

    for crate_name in CRATES {
        println!("Packaging: {}", crate_name);

        println!("Building Debian package...");
        run_cmd(Command::new("cargo").args(["deb", "--no-build", "-p", crate_name]))?;

        // Deb/RPM metadata package names (may differ from crate name).
        let pkg_name = crate_name;

        // Find built .deb file
        let deb_dir = Path::new("target/debian");
        let prefix = format!("{}_", pkg_name);
        let mut deb_file_path = None;

        if let Ok(entries) = fs::read_dir(deb_dir) {
            for entry in entries {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    if let Some(filename) = path.file_name().and_then(|s| s.to_str()) {
                        if filename.starts_with(&prefix) && filename.ends_with(".deb") {
                            deb_file_path = Some(path);
                            break;
                        }
                    }
                }
            }
        }

        if let Some(src_path) = deb_file_path {
            if let Some(fname) = src_path.file_name() {
                println!("Built: {:?}", fname);
                let dest_path = apt_pool.join(fname);
                fs::copy(&src_path, &dest_path)?;
                println!("Copied to apt repository packages/apt/pool/main/");
            }
        } else {
            return Err(format!(
                "Debian package not found for {crate_name} (searched for: {pkg_name}). Refusing partial release."
            )
            .into());
        }

        println!("Building RPM package...");
        run_cmd(Command::new("cargo").args(["generate-rpm", "-p", crate_name]))?;

        // Find built .rpm file
        let rpm_dir = Path::new("target/generate-rpm");
        let rpm_prefix = format!("{}-", pkg_name);
        let mut rpm_file_path = None;

        if let Ok(entries) = fs::read_dir(rpm_dir) {
            for entry in entries {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    if let Some(filename) = path.file_name().and_then(|s| s.to_str()) {
                        if filename.starts_with(&rpm_prefix) && filename.ends_with(".rpm") {
                            rpm_file_path = Some(path);
                            break;
                        }
                    }
                }
            }
        }

        if let Some(src_path) = rpm_file_path {
            if let Some(fname) = src_path.file_name() {
                println!("Built RPM: {:?}", fname);
                let dest_path = rpm_pool.join(fname);
                fs::copy(&src_path, &dest_path)?;
                println!("Copied to rpm repository packages/rpm/pool/");
            }
        } else {
            return Err(format!(
                "RPM package not found for {crate_name} (searched for: {pkg_name}). Refusing partial release."
            )
            .into());
        }
    }

    println!("==========================================");
    println!("Updating packages index...");
    println!("==========================================");
    
    // Execute the unified update script at the packages root
    run_cmd(Command::new("./update.sh").current_dir("../packages"))?;

    println!("==========================================");
    println!("IdleScreen build and package sync complete!");
    println!("==========================================");

    print!("\nDo you want to commit and push these package updates to GitHub? (y/n): ");
    io::stdout().flush()?;
    let mut response = String::new();
    io::stdin().read_line(&mut response)?;
    let response = response.trim().to_lowercase();

    if response == "y" || response == "yes" {
        let version = get_version()?;
        // Tag/commit/push live in scripts/package_push_packages.sh (line budget).
        run_cmd(
            Command::new("bash")
                .arg("../scripts/package_push_packages.sh")
                .arg(&version),
        )?;
    }

    Ok(())
}
