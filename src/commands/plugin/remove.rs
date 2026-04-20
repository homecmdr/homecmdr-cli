use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::Path;

use crate::workspace::resolve_workspace_root;

use super::add::{canonical_name, short_name};

// ---------------------------------------------------------------------------
// Minimal plugin.toml types (only need the block name for removal)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct PluginManifest {
    config: PluginConfigMeta,
}

#[derive(Deserialize)]
struct PluginConfigMeta {
    block: String,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(name: &str) -> Result<()> {
    let canonical = canonical_name(name);
    let workspace = resolve_workspace_root()?;
    println!("Workspace: {}", workspace.display());

    let dest = workspace.join("crates").join(&canonical);
    if !dest.exists() {
        bail!(
            "plugin '{}' is not installed (no directory at {}).",
            short_name(&canonical),
            dest.display()
        );
    }

    // Resolve the config block name from plugin.toml before we delete the
    // directory.  Fall back to deriving it from the adapter name for plugins
    // installed before the manifest requirement was introduced.
    let block_name = read_block_name(&dest, &canonical);

    // Unpatch workspace Cargo.toml
    let workspace_toml = workspace.join("Cargo.toml");
    unpatch_workspace_toml(&workspace_toml, &canonical)
        .context("failed to unpatch workspace Cargo.toml")?;

    // Unpatch crates/adapters/Cargo.toml
    let adapters_toml = workspace.join("crates").join("adapters").join("Cargo.toml");
    if adapters_toml.exists() {
        unpatch_adapters_toml(&adapters_toml, &canonical)
            .context("failed to unpatch crates/adapters/Cargo.toml")?;
    }

    // Unpatch crates/adapters/src/lib.rs
    let lib_rs = workspace
        .join("crates")
        .join("adapters")
        .join("src")
        .join("lib.rs");
    if lib_rs.exists() {
        unpatch_adapters_lib_rs(&lib_rs, &canonical)
            .context("failed to unpatch crates/adapters/src/lib.rs")?;
    }

    // Remove the config block from config/default.toml
    let config_path = workspace.join("config").join("default.toml");
    remove_config_block(&config_path, &block_name)
        .context("failed to remove plugin config block from config/default.toml")?;

    // Remove the crate directory
    fs::remove_dir_all(&dest).with_context(|| format!("failed to remove {}", dest.display()))?;
    println!("  Removed {}.", dest.display());

    println!();
    println!("Plugin '{}' removed.", short_name(&canonical));
    println!();

    // Rebuild
    crate::commands::build::run_cargo_build(&workspace, false)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Config block name resolution
// ---------------------------------------------------------------------------

/// Read the `config.block` value from `plugin.toml`.  If the file is absent
/// or unreadable (e.g. plugin was installed before the manifest requirement),
/// derive the block name from the adapter name instead.
fn read_block_name(crate_dir: &Path, canonical: &str) -> String {
    let manifest_path = crate_dir.join("plugin.toml");
    if let Ok(src) = fs::read_to_string(&manifest_path) {
        if let Ok(manifest) = toml::from_str::<PluginManifest>(&src) {
            return manifest.config.block;
        }
    }
    // Fallback: adapter-roku-tv → adapters.roku_tv
    format!("adapters.{}", short_name(canonical).replace('-', "_"))
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

    // Find the section header line
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

    // Build the result: everything before the section…
    let mut result: Vec<&str> = lines[..start].to_vec();

    // …strip any trailing blank lines that preceded the removed section…
    while result
        .last()
        .map(|l: &&str| l.trim().is_empty())
        .unwrap_or(false)
    {
        result.pop();
    }

    // …then, if there is content after the block, add one blank separator line
    // and the remaining lines.
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
// Unpatch helpers
// ---------------------------------------------------------------------------

fn unpatch_workspace_toml(toml_path: &Path, adapter_name: &str) -> Result<()> {
    let content = fs::read_to_string(toml_path)?;
    let member = format!("    \"crates/{}\",\n", adapter_name);

    if !content.contains(&member) {
        let member_bare = format!("\"crates/{}\"", adapter_name);
        if !content.contains(&member_bare) {
            println!(
                "  {} not found in workspace Cargo.toml members — skipping.",
                adapter_name
            );
            return Ok(());
        }
    }

    let patched = content.replace(&member, "");
    fs::write(toml_path, patched)?;
    println!("  Unpatched workspace Cargo.toml.");
    Ok(())
}

fn unpatch_adapters_toml(toml_path: &Path, adapter_name: &str) -> Result<()> {
    let content = fs::read_to_string(toml_path)?;
    let dep_line = format!("{} = {{ path = \"../{}\" }}\n", adapter_name, adapter_name);

    if !content.contains(&dep_line) {
        println!(
            "  {} not found in crates/adapters/Cargo.toml — skipping.",
            adapter_name
        );
        return Ok(());
    }

    let patched = content.replace(&dep_line, "");
    fs::write(toml_path, patched)?;
    println!("  Unpatched crates/adapters/Cargo.toml.");
    Ok(())
}

fn unpatch_adapters_lib_rs(lib_rs_path: &Path, adapter_name: &str) -> Result<()> {
    let crate_name = adapter_name.replace('-', "_");
    let use_stmt = format!("use {} as _;\n", crate_name);
    let use_stmt_bare = format!("use {} as _;", crate_name);

    let content = fs::read_to_string(lib_rs_path)?;

    if !content.contains(&use_stmt_bare) {
        println!(
            "  {} not found in crates/adapters/src/lib.rs — skipping.",
            adapter_name
        );
        return Ok(());
    }

    let patched = content.replace(&use_stmt, "").replace(&use_stmt_bare, "");
    fs::write(lib_rs_path, patched)?;
    println!("  Unpatched crates/adapters/src/lib.rs.");
    Ok(())
}
