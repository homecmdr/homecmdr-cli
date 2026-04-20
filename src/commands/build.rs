use anyhow::{bail, Context, Result};
use std::io::Read;
use std::path::Path;
use std::process::Command;

use crate::workspace::resolve_workspace_root;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run a build.  When `release` is true and `force_source` is false (the
/// default), the function first attempts to download a pre-built binary from
/// the latest GitHub release for this platform.  If the download succeeds the
/// cargo build step is skipped entirely.  If the download fails or no binary
/// is available for this platform, the function falls back to a local
/// `cargo build` automatically — no action is required from the caller.
///
/// Pass `force_source = true` when the workspace has been modified (e.g. a
/// plugin was just added) and the binary must be rebuilt from source.
pub fn run(release: bool, force_source: bool) -> Result<()> {
    let workspace = resolve_workspace_root()?;
    println!("Workspace: {}", workspace.display());

    if release && !force_source {
        match try_download_prebuilt(&workspace) {
            Ok(true) => {
                // Pre-built binary is now at target/release/api — fall through
                // to the install step below.
            }
            Ok(false) => {
                println!("No pre-built binary available for this platform. Building from source...");
                run_cargo_build(&workspace, true)?;
            }
            Err(e) => {
                println!("Pre-built download failed ({e}). Falling back to source build...");
                run_cargo_build(&workspace, true)?;
            }
        }
    } else {
        run_cargo_build(&workspace, release)?;
    }

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
// Shared build helper (called by init and plugin add/remove too)
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
// Pre-built binary download
// ---------------------------------------------------------------------------

const API_RELEASES_URL: &str =
    "https://api.github.com/repos/homecmdr/homecmdr-api/releases/latest";

/// Maps the current host triple to the asset name published in GitHub
/// Releases.  Returns `None` for platforms that are not yet covered by CI.
fn current_target_triple() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        // std::env::consts::ARCH returns "arm" for all 32-bit ARM targets
        ("linux", "arm") => Some("armv7-unknown-linux-gnueabihf"),
        _ => None,
    }
}

#[derive(serde::Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(serde::Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

/// Attempt to download the latest pre-built release binary for this platform
/// and write it to `workspace/target/release/api`.
///
/// Returns:
/// - `Ok(true)`  — binary downloaded and ready; skip `cargo build`
/// - `Ok(false)` — no binary for this platform; caller should fall back
/// - `Err(_)`    — download was attempted and failed; caller should fall back
fn try_download_prebuilt(workspace: &Path) -> Result<bool> {
    let Some(triple) = current_target_triple() else {
        // Unsupported platform — silent fallback to source build.
        return Ok(false);
    };

    println!("Checking GitHub releases for a pre-built binary ({triple})...");

    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!(
            "homecmdr-cli/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .context("failed to build HTTP client")?;

    let release_json = client
        .get(API_RELEASES_URL)
        .send()
        .context("failed to reach GitHub releases API")?
        .error_for_status()
        .context("GitHub releases API returned an error status")?
        .text()
        .context("failed to read GitHub releases response")?;

    let release: GithubRelease =
        serde_json::from_str(&release_json).context("failed to parse GitHub releases response")?;

    let asset_name = format!("homecmdr-api-{triple}");
    let Some(asset) = release.assets.iter().find(|a| a.name == asset_name) else {
        println!(
            "  No pre-built binary found for {triple} in release {}.",
            release.tag_name
        );
        return Ok(false);
    };

    println!(
        "  Downloading {} ({})...",
        asset_name, release.tag_name
    );

    let mut response = client
        .get(&asset.browser_download_url)
        .send()
        .context("failed to start binary download")?
        .error_for_status()
        .context("binary download returned an error status")?;

    let mut bytes = Vec::new();
    response
        .read_to_end(&mut bytes)
        .context("failed to read binary download")?;

    // Write to workspace/target/release/api so install_binary() can find it.
    let out_dir = workspace.join("target").join("release");
    std::fs::create_dir_all(&out_dir).context("failed to create target/release directory")?;
    let out_path = out_dir.join("api");
    std::fs::write(&out_path, &bytes).context("failed to write pre-built binary to disk")?;

    // Make executable (Unix only — the CI only publishes Linux binaries).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&out_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&out_path, perms)?;
    }

    println!(
        "  Downloaded {} ({:.1} MiB).",
        asset_name,
        bytes.len() as f64 / (1024.0 * 1024.0)
    );
    Ok(true)
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
        _ => println!(
            "  warning: could not stop service automatically. Run: sudo systemctl stop homecmdr"
        ),
    }
}

fn service_start() {
    let status = Command::new("sudo")
        .args(["systemctl", "start", "homecmdr"])
        .status();
    match status {
        Ok(s) if s.success() => println!("  Service started."),
        _ => println!(
            "  warning: could not start service automatically. Run: sudo systemctl start homecmdr"
        ),
    }
}
