use anyhow::{bail, Context, Result};
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::workspace::resolve_workspace_root;

const SERVICE_NAME: &str = "homecmdr";
const UNIT_PATH: &str = "/etc/systemd/system/homecmdr.service";
const SYSTEM_BIN: &str = "/usr/local/bin/homecmdr";
const CONFIG_DIR: &str = "/etc/homecmdr";
const DATA_DIR: &str = "/var/lib/homecmdr";

const SERVICE_UNIT: &str = r#"[Unit]
Description=HomeCmdr automation server
Documentation=https://github.com/homecmdr/homecmdr-api
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=homecmdr
Group=homecmdr

ExecStart=/usr/local/bin/homecmdr
Restart=on-failure
RestartSec=5s

# Configuration
Environment=HOMECMDR_CONFIG=/etc/homecmdr/default.toml
Environment=HOMECMDR_DATA_DIR=/var/lib/homecmdr

# Override the master key without touching the config file:
# EnvironmentFile=/etc/homecmdr/secrets.env
# (put HOMECMDR_MASTER_KEY=your-key-here in that file, chmod 600)

# Hardening
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/homecmdr
ReadOnlyPaths=/etc/homecmdr

[Install]
WantedBy=multi-user.target
"#;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run() -> Result<()> {
    // ── Preflight checks ──────────────────────────────────────────────────
    let workspace = resolve_workspace_root()?;

    let release_bin = workspace.join("target").join("release").join("api");
    if !release_bin.exists() {
        bail!(
            "release binary not found at {}.\n\
             Run 'homecmdr build --release' first.",
            release_bin.display()
        );
    }

    if Path::new(UNIT_PATH).exists() {
        bail!(
            "service unit already exists at {}.\n\
             Run 'homecmdr service uninstall' first if you want to reinstall.",
            UNIT_PATH
        );
    }

    println!("Installing HomeCmdr as a systemd service...");
    println!("(Several steps require sudo — you may be prompted for your password.)");
    println!();

    // ── 1. Install the release binary ─────────────────────────────────────
    println!("Step 1/6: Installing binary to {}...", SYSTEM_BIN);
    sudo_copy(&release_bin, SYSTEM_BIN)?;

    // ── 2. Create system user ─────────────────────────────────────────────
    println!("Step 2/6: Creating system user 'homecmdr'...");
    create_system_user()?;

    // ── 3. Create directories ─────────────────────────────────────────────
    println!("Step 3/6: Creating system directories...");
    sudo_run(&["mkdir", "-p", CONFIG_DIR, DATA_DIR])?;
    sudo_run(&["chown", "homecmdr:homecmdr", DATA_DIR])?;

    // ── 4. Install config files ───────────────────────────────────────────
    println!("Step 4/6: Installing config and asset files...");
    install_config(&workspace)?;

    // ── 5. Write systemd unit ─────────────────────────────────────────────
    println!("Step 5/6: Writing systemd unit to {}...", UNIT_PATH);
    write_unit_file()?;

    // ── 6. Enable and start the service ───────────────────────────────────
    println!("Step 6/6: Enabling and starting the service...");
    sudo_run(&["systemctl", "daemon-reload"])?;
    sudo_run(&["systemctl", "enable", "--now", SERVICE_NAME])?;

    println!();
    println!("HomeCmdr service installed and started.");
    println!();
    println!("Useful commands:");
    println!("  homecmdr service status   — check status");
    println!("  homecmdr service logs     — follow logs");
    println!("  homecmdr service restart  — restart after config changes");

    Ok(())
}

pub fn run_uninstall() -> Result<()> {
    println!("Uninstalling HomeCmdr service...");

    // Stop and disable
    let _ = sudo_run(&["systemctl", "stop", SERVICE_NAME]);
    let _ = sudo_run(&["systemctl", "disable", SERVICE_NAME]);

    // Remove unit file
    if Path::new(UNIT_PATH).exists() {
        sudo_run(&["rm", "-f", UNIT_PATH])?;
        sudo_run(&["systemctl", "daemon-reload"])?;
        println!("  Removed systemd unit.");
    } else {
        println!("  No systemd unit found at {} — skipping.", UNIT_PATH);
    }

    println!();
    println!("Service uninstalled.");
    println!("Note: /etc/homecmdr/ and /var/lib/homecmdr/ were NOT removed to preserve your data.");
    println!(
        "      Remove them manually with 'sudo rm -rf /etc/homecmdr /var/lib/homecmdr' if needed."
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sudo_copy(src: &Path, dst: &str) -> Result<()> {
    // Try direct first
    match fs::copy(src, dst) {
        Ok(_) => return Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {}
        Err(e) => return Err(e).context("failed to copy file"),
    }
    sudo_run(&["cp", src.to_str().unwrap_or(""), dst])
}

fn sudo_run(args: &[&str]) -> Result<()> {
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

fn create_system_user() -> Result<()> {
    // Check if user already exists
    let exists = Command::new("id")
        .arg("-u")
        .arg("homecmdr")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if exists {
        println!("  System user 'homecmdr' already exists — skipping.");
        return Ok(());
    }

    sudo_run(&[
        "useradd",
        "--system",
        "--no-create-home",
        "--shell",
        "/sbin/nologin",
        "homecmdr",
    ])
}

fn install_config(workspace: &Path) -> Result<()> {
    let workspace_config = workspace.join("config").join("default.toml");
    if !workspace_config.exists() {
        bail!(
            "workspace config not found at {}. Run 'homecmdr init' first.",
            workspace_config.display()
        );
    }

    // Copy main config
    let dest = format!("{}/default.toml", CONFIG_DIR);
    sudo_copy(&workspace_config, &dest)?;
    sudo_run(&["chmod", "640", &dest])?;
    sudo_run(&["chown", "root:homecmdr", &dest])?;
    println!("  Copied config to {}.", dest);

    // Copy Lua asset directories
    for dir_name in &["scenes", "automations", "scripts"] {
        let src = workspace.join("config").join(dir_name);
        if src.exists() {
            let dst = format!("{}/{}", CONFIG_DIR, dir_name);
            sudo_run(&["cp", "-r", src.to_str().unwrap_or(""), &dst])?;
            sudo_run(&["chown", "-R", "homecmdr:homecmdr", &dst])?;
            println!("  Copied {} to {}.", dir_name, dst);
        }
    }

    // Update directory paths in the installed config to use absolute paths
    patch_installed_config(&format!("{}/default.toml", CONFIG_DIR))?;

    Ok(())
}

/// After copying the config to /etc/homecmdr/, update the relative directory
/// paths to absolute /etc/homecmdr/<dir> paths so the service finds them.
fn patch_installed_config(config_path: &str) -> Result<()> {
    // Read the installed config (need sudo-readable path — we own it at this point
    // since we just copied it as root, but we might not be root yet in this process).
    // Simplest approach: use sudo tee to write the patched content.
    let content = match fs::read_to_string(config_path) {
        Ok(c) => c,
        Err(_) => {
            // Can't read it without root — skip; user can edit manually
            println!(
                "  (could not read {} to update directory paths — update them manually if needed)",
                config_path
            );
            return Ok(());
        }
    };

    let patched = content
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
        );

    if patched == content {
        return Ok(()); // nothing to change
    }

    // Write via sudo tee
    let mut child = Command::new("sudo")
        .args(["tee", config_path])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .context("failed to spawn sudo tee")?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(patched.as_bytes())
            .context("failed to write patched config")?;
    }

    let status = child.wait().context("failed to wait for sudo tee")?;
    if !status.success() {
        bail!("failed to write updated config to {}", config_path);
    }

    println!("  Updated directory paths in {}.", config_path);
    Ok(())
}

fn write_unit_file() -> Result<()> {
    // Write via sudo tee
    let mut child = Command::new("sudo")
        .args(["tee", UNIT_PATH])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .context("failed to spawn sudo tee")?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(SERVICE_UNIT.as_bytes())
            .context("failed to write unit file content")?;
    }

    let status = child.wait().context("failed to wait for sudo tee")?;
    if !status.success() {
        bail!("failed to write systemd unit to {}", UNIT_PATH);
    }

    Ok(())
}
