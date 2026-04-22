use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

const SERVICE: &str = "homecmdr";
const UNIT_PATH: &str = "/etc/systemd/system/homecmdr.service";
const SYSTEM_BIN: &str = "/usr/local/bin/homecmdr-server";

/// Verify that `homecmdr service install` has been run before trying to
/// start/stop/restart the service.  Gives a clear, actionable error instead of
/// a cryptic systemctl failure.
fn require_installed() -> Result<()> {
    if !Path::new(UNIT_PATH).exists() {
        bail!(
            "HomeCmdr service is not installed.\n\
             Run 'homecmdr service install' first."
        );
    }
    if !Path::new(SYSTEM_BIN).exists() {
        bail!(
            "Service unit exists at {} but the server binary is missing \
             from {}.\n\
             Run 'homecmdr service uninstall' then 'homecmdr service install' \
             to repair the installation.",
            UNIT_PATH, SYSTEM_BIN,
        );
    }
    Ok(())
}

pub fn start() -> Result<()> {
    require_installed()?;
    systemctl(&["start", SERVICE])
}

pub fn stop() -> Result<()> {
    require_installed()?;
    systemctl(&["stop", SERVICE])
}

pub fn restart() -> Result<()> {
    require_installed()?;
    systemctl(&["restart", SERVICE])
}

pub fn status() -> Result<()> {
    // systemctl status exits non-zero when the service is stopped — that's
    // fine; we still want to show the output.
    let _ = Command::new("systemctl").args(["status", SERVICE]).status();
    Ok(())
}

pub fn logs() -> Result<()> {
    // Follow journal output — this blocks until the user presses Ctrl-C.
    Command::new("journalctl")
        .args(["-u", SERVICE, "-f", "--no-pager"])
        .status()
        .context("failed to launch journalctl — is systemd available?")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared helper
// ---------------------------------------------------------------------------

fn systemctl(args: &[&str]) -> Result<()> {
    let status = Command::new("sudo")
        .arg("systemctl")
        .args(args)
        .status()
        .with_context(|| format!("failed to run: sudo systemctl {}", args.join(" ")))?;

    if !status.success() {
        // Don't hard-error; systemctl prints its own diagnostics.
        eprintln!(
            "warning: 'sudo systemctl {}' exited with code {}",
            args.join(" "),
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}
