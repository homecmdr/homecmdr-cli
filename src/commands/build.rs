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
        install_binary(&workspace)?;
        maybe_restart_service();
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
// Optional service restart
// ---------------------------------------------------------------------------

fn maybe_restart_service() {
    // Check if the service is currently active; if so, offer to restart.
    let is_active = Command::new("systemctl")
        .args(["is-active", "--quiet", "homecmdr"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !is_active {
        return;
    }

    println!("The homecmdr service is running. Restarting it to pick up the new binary...");
    let status = Command::new("sudo")
        .args(["systemctl", "restart", "homecmdr"])
        .status();

    match status {
        Ok(s) if s.success() => println!("Service restarted."),
        _ => println!("Could not restart automatically. Run: sudo systemctl restart homecmdr"),
    }
}
