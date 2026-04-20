use anyhow::{Context, Result};
use std::process::Command;

const SERVICE: &str = "homecmdr";

pub fn start() -> Result<()> {
    systemctl(&["start", SERVICE])
}

pub fn stop() -> Result<()> {
    systemctl(&["stop", SERVICE])
}

pub fn restart() -> Result<()> {
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
