use anyhow::{bail, Context, Result};
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::workspace::resolve_workspace_root;

use super::add::{adapter_name, canonical_name, short_name};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(name: &str) -> Result<()> {
    let canonical = canonical_name(name);
    let workspace = resolve_workspace_root()?;
    println!("Workspace: {}", workspace.display());

    let adapter = adapter_name(&canonical);
    let plugins_dir = workspace.join("config").join("plugins");

    let wasm_path = plugins_dir.join(format!("{}.wasm", adapter));
    let manifest_path = plugins_dir.join(format!("{}.plugin.toml", adapter));

    if !wasm_path.exists() && !manifest_path.exists() {
        bail!(
            "plugin '{}' is not installed (no files found in {}).",
            short_name(&canonical),
            plugins_dir.display(),
        );
    }

    // Remove .wasm binary
    if wasm_path.exists() {
        fs::remove_file(&wasm_path)
            .with_context(|| format!("failed to remove {}", wasm_path.display()))?;
        println!("  Removed {}.", wasm_path.display());
    } else {
        println!("  {} not found — skipping.", wasm_path.display());
    }

    // Remove .plugin.toml manifest
    if manifest_path.exists() {
        fs::remove_file(&manifest_path)
            .with_context(|| format!("failed to remove {}", manifest_path.display()))?;
        println!("  Removed {}.", manifest_path.display());
    } else {
        println!("  {} not found — skipping.", manifest_path.display());
    }

    // Remove [adapters.<name>] block from config/default.toml
    let config_path = workspace.join("config").join("default.toml");
    let block_name = format!("adapters.{}", adapter);
    remove_config_block(&config_path, &block_name)
        .context("failed to remove plugin config block from config/default.toml")?;

    // If the service is deployed, sync updated config and remove the plugin
    // files from the system plugin directory so the factory is no longer
    // loaded on the next start.
    if std::path::Path::new(crate::commands::config_sync::SYSTEM_CONFIG).exists() {
        println!("  Syncing config to system (/etc/homecmdr/default.toml)...");
        crate::commands::config_sync::sync_workspace_config_to_system(&workspace)
            .context("failed to sync config to /etc/homecmdr/default.toml")?;

        let system_plugins_dir = "/etc/homecmdr/plugins";
        let system_wasm = format!("{}/{}.wasm", system_plugins_dir, adapter);
        let system_toml = format!("{}/{}.plugin.toml", system_plugins_dir, adapter);
        println!("  Removing plugin files from {}...", system_plugins_dir);
        // Ignore errors — files may not be present if they were never synced.
        let _ = crate::commands::config_sync::sudo_run(&["rm", "-f", &system_wasm, &system_toml]);
    }

    println!();
    println!("Plugin '{}' removed.", short_name(&canonical));
    println!();

    // Restart service if running — no rebuild needed.
    if is_service_active() {
        println!("Service is running — restarting to unload the plugin...");
        restart_service();
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Config block removal
// ---------------------------------------------------------------------------

/// Remove the `[block_name]` section and all of its key-value lines from the
/// config file.  Stops at the next TOML section header (`[`) or EOF.
/// Cleans up surrounding blank lines so the file stays tidy.
fn remove_config_block(config_path: &Path, block_name: &str) -> Result<()> {
    if !config_path.exists() {
        println!("  config/default.toml not found — skipping config cleanup.");
        return Ok(());
    }

    let content = fs::read_to_string(config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;

    let header = format!("[{}]", block_name);
    let lines: Vec<&str> = content.lines().collect();

    let start = match lines.iter().position(|l| l.trim() == header) {
        Some(i) => i,
        None => {
            println!(
                "  [{}] block not found in config/default.toml — skipping.",
                block_name
            );
            return Ok(());
        }
    };

    // Find where the section ends: the next line that starts a new header, or EOF
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .map(|i| start + 1 + i)
        .unwrap_or(lines.len());

    // Everything before the section, with trailing blank lines stripped
    let mut result: Vec<&str> = lines[..start].to_vec();
    while result
        .last()
        .map(|l: &&str| l.trim().is_empty())
        .unwrap_or(false)
    {
        result.pop();
    }

    if end < lines.len() {
        result.push("");
        result.extend_from_slice(&lines[end..]);
    }

    let mut patched = result.join("\n");
    if content.ends_with('\n') {
        patched.push('\n');
    }

    fs::write(config_path, &patched)
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    println!("  Removed [{}] block from config/default.toml.", block_name);
    Ok(())
}

// ---------------------------------------------------------------------------
// Service helpers
// ---------------------------------------------------------------------------

fn is_service_active() -> bool {
    Command::new("systemctl")
        .args(["is-active", "--quiet", "homecmdr"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn restart_service() {
    let status = Command::new("sudo")
        .args(["systemctl", "restart", "homecmdr"])
        .status();
    match status {
        Ok(s) if s.success() => println!("  Service restarted."),
        _ => println!(
            "  warning: could not restart service automatically.\n\
             Run: sudo systemctl restart homecmdr"
        ),
    }
}
