use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

use crate::workspace::resolve_workspace_root;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(release: bool) -> Result<()> {
    let workspace = resolve_workspace_root()?;
    println!("Workspace: {}", workspace.display());

    run_cargo_build(&workspace, release)?;

    if release {
        // Stop the service before replacing the binary on disk — Linux refuses
        // to overwrite an executable that is currently mapped into a running
        // process ("Text file busy").
        let was_active = service_is_active();
        if was_active {
            println!("Stopping service before installing binary...");
            service_stop();
        }

        install_binary(&workspace)?;

        if was_active {
            println!("Starting service...");
            service_start();
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Shared build helper (called by init and adapter add too)
// ---------------------------------------------------------------------------

pub fn run_cargo_build(workspace: &Path, release: bool) -> Result<()> {
    let mode = if release { "release" } else { "debug" };
    println!(
        "Building {} binary (cargo build{})...",
        mode,
        if release {
            " --release -p api"
        } else {
            " -p api"
        }
    );

    let mut cmd = Command::new("cargo");
    cmd.arg("build");
    if release {
        cmd.arg("--release");
    }
    cmd.args(["-p", "api"]);
    cmd.current_dir(workspace);

    let status = cmd
        .status()
        .context("failed to launch cargo — is it installed and on PATH?")?;

    if !status.success() {
        bail!(
            "cargo build failed (exit code {})",
            status.code().unwrap_or(-1)
        );
    }

    println!("Build succeeded.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Binary installation
// ---------------------------------------------------------------------------

const INSTALL_PATH: &str = "/usr/local/bin/homecmdr";

fn install_binary(workspace: &Path) -> Result<()> {
    let src = workspace.join("target").join("release").join("api");
    if !src.exists() {
        bail!(
            "release binary not found at {}. Did the build succeed?",
            src.display()
        );
    }

    println!("Installing binary to {}...", INSTALL_PATH);

    // Try direct copy first (works if user is root or /usr/local/bin is writable)
    match std::fs::copy(&src, INSTALL_PATH) {
        Ok(_) => {
            println!("Installed to {}.", INSTALL_PATH);
            return Ok(());
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            // Fall through to sudo
        }
        Err(e) => return Err(e).context("failed to copy binary"),
    }

    // Re-try via sudo
    println!("  (requires sudo to write to {INSTALL_PATH})");
    let status = Command::new("sudo")
        .args(["cp", src.to_str().unwrap_or(""), INSTALL_PATH])
        .status()
        .context("failed to launch sudo")?;

    if !status.success() {
        bail!(
            "could not install binary. Run manually:\n  sudo cp {} {}",
            src.display(),
            INSTALL_PATH
        );
    }

    println!("Installed to {}.", INSTALL_PATH);
    Ok(())
}

// ---------------------------------------------------------------------------
// Service helpers
// ---------------------------------------------------------------------------

fn service_is_active() -> bool {
    Command::new("systemctl")
        .args(["is-active", "--quiet", "homecmdr"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn service_stop() {
    let status = Command::new("sudo")
        .args(["systemctl", "stop", "homecmdr"])
        .status();
    match status {
        Ok(s) if s.success() => println!("  Service stopped."),
        _ => println!("  warning: could not stop service automatically. Run: sudo systemctl stop homecmdr"),
    }
}

fn service_start() {
    let status = Command::new("sudo")
        .args(["systemctl", "start", "homecmdr"])
        .status();
    match status {
        Ok(s) if s.success() => println!("  Service started."),
        _ => println!("  warning: could not start service automatically. Run: sudo systemctl start homecmdr"),
    }
}
