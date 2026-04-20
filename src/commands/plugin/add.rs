use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::Command;

use crate::workspace::resolve_workspace_root;

pub const REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/homecmdr/adapters/main/adapters.toml";
pub const ADAPTERS_ARCHIVE_URL: &str =
    "https://github.com/homecmdr/adapters/archive/refs/heads/main.zip";

// ---------------------------------------------------------------------------
// Registry types (shared with list.rs)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct Registry {
    pub adapters: Vec<AdapterEntry>,
}

#[derive(Deserialize)]
pub struct AdapterEntry {
    pub name: String,
    pub path: String,
    pub display_name: String,
    pub description: String,
    pub version: String,
}

// ---------------------------------------------------------------------------
// plugin.toml manifest types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct PluginManifest {
    config: PluginConfig,
}

#[derive(Deserialize)]
struct PluginConfig {
    block: String,
    fields: Vec<PluginField>,
}

#[derive(Deserialize)]
struct PluginField {
    key: String,
    #[serde(rename = "type")]
    field_type: String,
    description: String,
    /// Pre-filled default — user can press Enter to accept.
    default: Option<String>,
    /// Must be provided; no default allowed.
    #[serde(default)]
    required: bool,
    /// May be left blank; key is omitted from the config block if empty.
    #[serde(default)]
    optional: bool,
    /// Hint: value is sensitive (password). Stored plaintext for now.
    #[serde(default)]
    secret: bool,
}

// ---------------------------------------------------------------------------
// Name normalisation
// ---------------------------------------------------------------------------

/// Accept either the full registry name (`adapter-elgato-lights`) or the
/// short form without the prefix (`elgato-lights`).  Always returns the
/// canonical `adapter-*` name used in the registry and workspace.
pub fn canonical_name(name: &str) -> String {
    if name.starts_with("adapter-") {
        name.to_string()
    } else {
        format!("adapter-{}", name)
    }
}

/// The short display name shown to the user, with the `adapter-` prefix
/// stripped for readability.
pub fn short_name(name: &str) -> &str {
    name.strip_prefix("adapter-").unwrap_or(name)
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(name: &str) -> Result<()> {
    let canonical = canonical_name(name);
    let workspace = resolve_workspace_root()?;
    println!("Workspace: {}", workspace.display());

    // Fetch registry
    println!("Fetching plugin registry...");
    let registry = fetch_registry()?;

    // Find the requested plugin
    let entry = registry
        .adapters
        .iter()
        .find(|a| a.name == canonical)
        .ok_or_else(|| {
            let available: Vec<String> = registry
                .adapters
                .iter()
                .map(|a| short_name(&a.name).to_string())
                .collect();
            anyhow!(
                "plugin '{}' not found in official registry.\nAvailable plugins:\n{}",
                short_name(&canonical),
                available
                    .iter()
                    .map(|n| format!("  - {n}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        })?;

    let dest = workspace.join("crates").join(&entry.name);
    if dest.exists() {
        bail!(
            "plugin '{}' is already installed.\n\
             Run 'homecmdr plugin remove {}' first if you want to reinstall.",
            short_name(&entry.name),
            short_name(&entry.name),
        );
    }

    // Download and extract plugin crate
    println!("Downloading {}  v{}...", entry.display_name, entry.version);
    let zip_bytes = fetch_archive()?;
    extract_adapter(&zip_bytes, &entry.path, &dest)
        .context("failed to extract plugin from archive")?;
    println!("  Extracted to {}", dest.display());

    // Patch 1: workspace Cargo.toml
    let workspace_toml = workspace.join("Cargo.toml");
    patch_workspace_toml(&workspace_toml, &entry.name)
        .context("failed to patch workspace Cargo.toml")?;

    // Patch 2: crates/adapters/Cargo.toml
    let adapters_toml = workspace.join("crates").join("adapters").join("Cargo.toml");
    if adapters_toml.exists() {
        patch_adapters_toml(&adapters_toml, &entry.name)
            .context("failed to patch crates/adapters/Cargo.toml")?;
    } else {
        eprintln!(
            "warning: crates/adapters/Cargo.toml not found — skipping dependency patch. \
             Add it manually."
        );
    }

    // Patch 3: crates/adapters/src/lib.rs
    let lib_rs = workspace
        .join("crates")
        .join("adapters")
        .join("src")
        .join("lib.rs");
    if lib_rs.exists() {
        patch_adapters_lib_rs(&lib_rs, &entry.name)
            .context("failed to patch crates/adapters/src/lib.rs")?;
    } else {
        eprintln!(
            "warning: crates/adapters/src/lib.rs not found — skipping factory registration. \
             Add 'use {} as _;' manually.",
            entry.name.replace('-', "_")
        );
    }

    // Read plugin.toml — required, hard fail if absent
    let manifest_path = dest.join("plugin.toml");
    if !manifest_path.exists() {
        bail!(
            "plugin '{}' is missing plugin.toml.\n\
             This file is required for the CLI to configure the plugin.\n\
             Please report this at https://github.com/homecmdr/adapters",
            short_name(&entry.name)
        );
    }
    let manifest_src = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let manifest: PluginManifest = toml::from_str(&manifest_src).with_context(|| {
        format!(
            "failed to parse plugin.toml for '{}'",
            short_name(&entry.name)
        )
    })?;

    // Prompt user for config values
    println!();
    println!(
        "Configure {} — press Enter to accept defaults:",
        entry.display_name
    );
    println!();
    let config_block =
        prompt_config_block(&manifest.config).context("failed to collect plugin configuration")?;

    // Append the block to config/default.toml
    let config_path = workspace.join("config").join("default.toml");
    append_config_block(&config_path, &config_block)
        .context("failed to write plugin config block to config/default.toml")?;
    println!();
    println!("  Config block written to config/default.toml.");

    // If the service is installed, sync the updated workspace config to
    // /etc/homecmdr/default.toml.  Without this step the service would
    // restart with a new binary but the old config — and the new adapter
    // block would never be read.
    if std::path::Path::new(crate::commands::config_sync::SYSTEM_CONFIG).exists() {
        println!("  Syncing config to system (/etc/homecmdr/default.toml)...");
        crate::commands::config_sync::sync_workspace_config_to_system(&workspace)
            .context("failed to sync config to /etc/homecmdr/default.toml")?;
    }

    println!();
    println!("Plugin '{}' installed.", short_name(&entry.name));
    println!();

    // Rebuild — always from source because the workspace Cargo.toml was just
    // patched with a new plugin crate.  A pre-built binary would not include
    // the new plugin, so force_source = true.
    //
    // If the service is already running, do a full release build + binary
    // install + service restart so the new plugin is live immediately.
    // Otherwise a debug build is sufficient.
    if is_service_active() {
        println!("Service is running — performing release build, installing, and restarting...");
        crate::commands::build::run(true, true)?;
    } else {
        crate::commands::build::run_cargo_build(&workspace, false)?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Config prompting
// ---------------------------------------------------------------------------

/// Walk every field in the manifest, prompt the user, and return a formatted
/// TOML block string ready to append to config/default.toml.
fn prompt_config_block(config: &PluginConfig) -> Result<String> {
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("[{}]", config.block));

    for field in &config.fields {
        let value = prompt_field(field)?;
        // Optional field that the user left blank — omit entirely.
        if value.is_empty() {
            continue;
        }
        let toml_line = format_toml_line(&field.key, &field.field_type, &value);
        lines.push(toml_line);
    }

    lines.push(String::new()); // trailing newline after block
    Ok(lines.join("\n"))
}

/// Prompt a single field.  Returns the raw string value entered by the user
/// (already defaulted / validated).
fn prompt_field(field: &PluginField) -> Result<String> {
    let secret_hint = if field.secret { " (sensitive)" } else { "" };

    loop {
        if field.required {
            print!("  {} [required{}]: ", field.description, secret_hint);
        } else if let Some(ref default) = field.default {
            print!("  {} [{}{}]: ", field.description, default, secret_hint);
        } else {
            // optional, no default
            print!("  {} [optional{}]: ", field.description, secret_hint);
        }

        io::stdout().flush()?;
        let mut buf = String::new();
        io::stdin().read_line(&mut buf)?;
        let trimmed = buf.trim().to_string();

        if trimmed.is_empty() {
            if let Some(ref default) = field.default {
                return Ok(default.clone());
            }
            if field.optional {
                return Ok(String::new()); // blank → omit from config
            }
            // required with no default and no input
            println!("  This field is required — please enter a value.");
            continue;
        }

        return Ok(trimmed);
    }
}

/// Format a single TOML key = value line based on the declared type.
fn format_toml_line(key: &str, field_type: &str, value: &str) -> String {
    match field_type {
        "bool" | "u64" | "i64" | "f64" => {
            // Unquoted for numeric / boolean types
            format!("{} = {}", key, value)
        }
        _ => {
            // String and anything else — quoted
            format!("{} = {:?}", key, value)
        }
    }
}

/// Append a formatted config block to the workspace config file.
fn append_config_block(config_path: &Path, block: &str) -> Result<()> {
    if !config_path.exists() {
        bail!(
            "config/default.toml not found at {}.\n\
             Run 'homecmdr init' first.",
            config_path.display()
        );
    }

    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(config_path)
        .with_context(|| format!("failed to open {} for appending", config_path.display()))?;

    writeln!(file)?; // blank line separator before new block
    file.write_all(block.as_bytes())
        .with_context(|| format!("failed to write to {}", config_path.display()))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Service detection
// ---------------------------------------------------------------------------

/// Returns true if the homecmdr systemd service is currently active.
/// Safe to call without sudo — `systemctl is-active` is a read-only query.
fn is_service_active() -> bool {
    Command::new("systemctl")
        .args(["is-active", "--quiet", "homecmdr"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

pub fn fetch_registry() -> Result<Registry> {
    let body = reqwest::blocking::get(REGISTRY_URL)
        .context("failed to fetch plugin registry")?
        .error_for_status()
        .context("registry returned an error status")?
        .text()
        .context("failed to read registry response")?;
    toml::from_str(&body).context("failed to parse plugin registry")
}

fn fetch_archive() -> Result<Vec<u8>> {
    let mut response = reqwest::blocking::get(ADAPTERS_ARCHIVE_URL)
        .context("failed to download plugin archive")?
        .error_for_status()
        .context("plugin archive returned an error status")?;
    let mut bytes = Vec::new();
    response
        .read_to_end(&mut bytes)
        .context("failed to read plugin archive")?;
    Ok(bytes)
}

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

fn extract_adapter(zip_bytes: &[u8], adapter_path: &str, dest: &Path) -> Result<()> {
    let cursor = io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor).context("failed to open zip archive")?;

    // GitHub archives are prefixed with "<repo>-<branch>/", e.g. "adapters-main/"
    let prefix = format!("adapters-main/{}/", adapter_path);

    let mut extracted = 0usize;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let raw_name = file.name().to_string();

        if !raw_name.starts_with(&prefix) {
            continue;
        }

        let relative = &raw_name[prefix.len()..];
        if relative.is_empty() {
            continue;
        }

        let out_path = dest.join(relative);

        if raw_name.ends_with('/') {
            fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut content = Vec::new();
            io::Read::read_to_end(&mut file, &mut content)?;
            fs::write(&out_path, &content)?;
            extracted += 1;
        }
    }

    if extracted == 0 {
        bail!(
            "no files found for plugin path '{}' in the archive — \
             check that the plugin name is correct",
            adapter_path
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Cargo.toml patching
// ---------------------------------------------------------------------------

pub fn patch_workspace_toml(toml_path: &Path, adapter_name: &str) -> Result<()> {
    let content = fs::read_to_string(toml_path)?;
    let member = format!("\"crates/{}\"", adapter_name);

    if content.contains(&member) {
        println!(
            "  {} already in workspace members — skipping.",
            adapter_name
        );
        return Ok(());
    }

    let patched = content.replacen("]\nresolver", &format!("    {member},\n]\nresolver"), 1);

    if patched == content {
        bail!(
            "could not patch workspace Cargo.toml — members array format not recognised. \
             Add '{}' to [workspace] members manually.",
            member
        );
    }

    fs::write(toml_path, patched)?;
    println!("  Patched workspace Cargo.toml.");
    Ok(())
}

pub fn patch_adapters_toml(toml_path: &Path, adapter_name: &str) -> Result<()> {
    let content = fs::read_to_string(toml_path)?;
    let dep_key = format!("{} =", adapter_name);

    if content.contains(&dep_key) {
        println!(
            "  {} already in crates/adapters/Cargo.toml — skipping.",
            adapter_name
        );
        return Ok(());
    }

    let new_dep = format!("{} = {{ path = \"../{}\" }}\n", adapter_name, adapter_name);
    let patched = if content.trim_end().ends_with('\n') {
        format!("{}{}", content, new_dep)
    } else {
        format!("{}\n{}", content, new_dep)
    };

    fs::write(toml_path, patched)?;
    println!("  Patched crates/adapters/Cargo.toml.");
    Ok(())
}

/// Appends `use <crate_name> as _;` to `crates/adapters/src/lib.rs`.
/// Crate name is the adapter name with hyphens replaced by underscores.
pub fn patch_adapters_lib_rs(lib_rs_path: &Path, adapter_name: &str) -> Result<()> {
    let crate_name = adapter_name.replace('-', "_");
    let use_stmt = format!("use {} as _;", crate_name);

    let content = fs::read_to_string(lib_rs_path)?;

    if content.contains(&use_stmt) {
        println!(
            "  {} already in crates/adapters/src/lib.rs — skipping.",
            adapter_name
        );
        return Ok(());
    }

    let patched = format!("{}\n{}\n", content.trim_end(), use_stmt);
    fs::write(lib_rs_path, patched)?;
    println!("  Patched crates/adapters/src/lib.rs.");
    Ok(())
}
