use anyhow::{bail, Context, Result};
use std::fs;
use std::path::Path;
use std::process::Command;

pub const CONFIG_DIR: &str = "/etc/homecmdr";
pub const SYSTEM_CONFIG: &str = "/etc/homecmdr/default.toml";

// ---------------------------------------------------------------------------
// Path patching
// ---------------------------------------------------------------------------

/// Replace workspace-relative asset paths with their system equivalents so
/// the config written to /etc/homecmdr/default.toml is self-contained.
pub fn patch_config_paths(raw: &str) -> String {
    raw.replace(
        "directory = \"config/plugins\"",
        "directory = \"/etc/homecmdr/plugins\"",
    )
    .replace(
        "directory = \"config/scenes\"",
        "directory = \"/etc/homecmdr/scenes\"",
    )
    .replace(
        "directory = \"config/automations\"",
        "directory = \"/etc/homecmdr/automations\"",
    )
    .replace(
        "directory = \"config/scripts\"",
        "directory = \"/etc/homecmdr/scripts\"",
    )
}

// ---------------------------------------------------------------------------
// System config sync
// ---------------------------------------------------------------------------

/// Read `config/default.toml` from the workspace, apply system path patches,
/// and write the result to `/etc/homecmdr/default.toml` (via sudo if needed).
/// Sets ownership `root:homecmdr` and mode `640` on the target.
///
/// Only call this when the service is installed (i.e. CONFIG_DIR exists).
pub fn sync_workspace_config_to_system(workspace: &Path) -> Result<()> {
    let workspace_config = workspace.join("config").join("default.toml");
    if !workspace_config.exists() {
        bail!(
            "workspace config not found at {}. Run 'homecmdr init' first.",
            workspace_config.display()
        );
    }

    let raw = fs::read_to_string(&workspace_config).with_context(|| {
        format!(
            "failed to read workspace config at {}",
            workspace_config.display()
        )
    })?;

    let patched = patch_config_paths(&raw);

    write_via_sudo_tee(&patched, SYSTEM_CONFIG)
        .context("failed to write config to /etc/homecmdr/")?;
    sudo_run(&["chmod", "640", SYSTEM_CONFIG])?;
    sudo_run(&["chown", "root:homecmdr", SYSTEM_CONFIG])?;

    println!("  Synced config to {}.", SYSTEM_CONFIG);
    Ok(())
}

// ---------------------------------------------------------------------------
// Low-level helpers (shared with service/install.rs)
// ---------------------------------------------------------------------------

/// Write `content` to `path` as root using `sudo tee`.
pub fn write_via_sudo_tee(content: &str, path: &str) -> Result<()> {
    let mut child = Command::new("sudo")
        .args(["tee", path])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .context("failed to spawn sudo tee")?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(content.as_bytes())
            .context("failed to pipe content to sudo tee")?;
    }

    let status = child.wait().context("failed to wait for sudo tee")?;
    if !status.success() {
        bail!("sudo tee failed writing to {}", path);
    }
    Ok(())
}

pub fn sudo_run(args: &[&str]) -> Result<()> {
    let (prog, prog_args) = args.split_first().expect("args must not be empty");
    let mut cmd = Command::new("sudo");
    cmd.arg(prog).args(prog_args);
    let status = cmd
        .status()
        .with_context(|| format!("failed to run: sudo {}", args.join(" ")))?;
    if !status.success() {
        bail!(
            "command failed (exit {}): sudo {}",
            status.code().unwrap_or(-1),
            args.join(" ")
        );
    }
    Ok(())
}
