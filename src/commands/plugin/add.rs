use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

use crate::workspace::resolve_workspace_root;

pub const REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/homecmdr/plugins/main/plugins.toml";

// Base URL for raw file downloads from the plugins repo.
const PLUGINS_RAW_BASE: &str =
    "https://raw.githubusercontent.com/homecmdr/plugins/main";

// ---------------------------------------------------------------------------
// Registry types (shared with list.rs)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct Registry {
    pub plugins: Vec<PluginEntry>,
}

#[derive(Deserialize, Clone)]
pub struct PluginEntry {
    pub name: String,
    pub path: String,
    pub display_name: String,
    pub description: String,
    pub version: String,
}

// ---------------------------------------------------------------------------
// Merged .plugin.toml manifest types
// ---------------------------------------------------------------------------

/// The merged plugin manifest shipped with each plugin.
/// The `[plugin]` and `[runtime]` sections are read by the WASM host.
/// The `[[config.fields]]` section is used only by the CLI for interactive
/// config prompting; the host ignores it.
#[derive(Deserialize)]
pub struct PluginManifest {
    #[allow(dead_code)]
    pub plugin: PluginMeta,
    #[serde(default)]
    pub config: CliConfig,
}

#[allow(dead_code)]
#[derive(Deserialize)]
pub struct PluginMeta {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: String,
}

#[derive(Deserialize, Default)]
pub struct CliConfig {
    #[serde(default)]
    pub fields: Vec<PluginField>,
}

#[derive(Deserialize)]
pub struct PluginField {
    pub key: String,
    #[serde(rename = "type")]
    pub field_type: String,
    pub description: String,
    /// Pre-filled default — user can press Enter to accept.
    pub default: Option<String>,
    /// Must be provided; no default allowed.
    #[serde(default)]
    pub required: bool,
    /// May be left blank; key is omitted from the config block if empty.
    #[serde(default)]
    pub optional: bool,
    /// Hint: value is sensitive (password). Stored plaintext for now.
    #[serde(default)]
    pub secret: bool,
}

// ---------------------------------------------------------------------------
// Name normalisation
// ---------------------------------------------------------------------------

/// Accept either the full registry name (`plugin-elgato-lights`) or the short
/// form without the prefix (`elgato-lights`).  Always returns the canonical
/// `plugin-*` name used in the registry.
pub fn canonical_name(name: &str) -> String {
    if name.starts_with("plugin-") {
        name.to_string()
    } else {
        format!("plugin-{}", name)
    }
}

/// Short display name shown to the user, with the `plugin-` prefix stripped.
pub fn short_name(name: &str) -> &str {
    name.strip_prefix("plugin-").unwrap_or(name)
}

/// Convert a plugin name to the snake_case adapter name used in config keys
/// and manifest `[plugin] name`.  e.g. "elgato-lights" → "elgato_lights".
pub fn adapter_name(canonical: &str) -> String {
    short_name(canonical).replace('-', "_")
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(name: &str) -> Result<()> {
    let canonical = canonical_name(name);
    let workspace = resolve_workspace_root()?;
    println!("Workspace: {}", workspace.display());

    // Ensure plugins directory exists
    let plugins_dir = workspace.join("config").join("plugins");
    fs::create_dir_all(&plugins_dir)
        .with_context(|| format!("failed to create {}", plugins_dir.display()))?;

    let adapter = adapter_name(&canonical);

    // Check if already installed (wasm file present)
    let wasm_dest = plugins_dir.join(format!("{}.wasm", adapter));
    if wasm_dest.exists() {
        bail!(
            "plugin '{}' is already installed (found {}).\n\
             Run 'homecmdr plugin remove {}' first if you want to reinstall.",
            short_name(&canonical),
            wasm_dest.display(),
            short_name(&canonical),
        );
    }

    // Fetch registry
    println!("Fetching plugin registry...");
    let registry = fetch_registry()?;

    // Find the requested plugin
    let entry = registry
        .plugins
        .iter()
        .find(|p| p.name == canonical)
        .ok_or_else(|| {
            let available: Vec<String> = registry
                .plugins
                .iter()
                .map(|p| short_name(&p.name).to_string())
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

    println!("Downloading {}  v{}...", entry.display_name, entry.version);

    // Download .plugin.toml manifest
    let manifest_url = format!(
        "{PLUGINS_RAW_BASE}/{}/{}.plugin.toml",
        entry.path, adapter
    );
    let manifest_bytes = download_bytes(&manifest_url)
        .with_context(|| format!("failed to download manifest from {}", manifest_url))?;
    let manifest_str = String::from_utf8(manifest_bytes.clone())
        .context("manifest file is not valid UTF-8")?;

    // Parse manifest for CLI config prompting
    let manifest: PluginManifest = toml::from_str(&manifest_str)
        .with_context(|| format!("failed to parse manifest for '{}'", short_name(&canonical)))?;

    // Download .wasm binary
    let wasm_url = format!(
        "{PLUGINS_RAW_BASE}/{}/{}.wasm",
        entry.path, adapter
    );
    let wasm_bytes = download_bytes(&wasm_url)
        .with_context(|| format!("failed to download WASM binary from {}", wasm_url))?;

    // Write files to config/plugins/
    let manifest_dest = plugins_dir.join(format!("{}.plugin.toml", adapter));
    fs::write(&manifest_dest, &manifest_bytes)
        .with_context(|| format!("failed to write {}", manifest_dest.display()))?;
    println!("  Written: {}", manifest_dest.display());

    fs::write(&wasm_dest, &wasm_bytes)
        .with_context(|| format!("failed to write {}", wasm_dest.display()))?;
    println!("  Written: {}", wasm_dest.display());

    // Prompt user for config values (if [[config.fields]] section present)
    let config_block = if manifest.config.fields.is_empty() {
        // No interactive config needed — emit a minimal block
        format!("[adapters.{}]\nenabled = true\n", adapter)
    } else {
        println!();
        println!(
            "Configure {} — press Enter to accept defaults:",
            entry.display_name
        );
        println!();
        prompt_config_block(&adapter, &manifest.config)
            .context("failed to collect plugin configuration")?
    };

    // Append the block to config/default.toml
    let config_path = workspace.join("config").join("default.toml");
    append_config_block(&config_path, &config_block)
        .context("failed to write plugin config block to config/default.toml")?;
    println!();
    println!("  Config block written to config/default.toml.");

    // If the service is installed, sync the updated workspace config to
    // /etc/homecmdr/default.toml so the service picks it up on restart.
    if std::path::Path::new(crate::commands::config_sync::SYSTEM_CONFIG).exists() {
        println!("  Syncing config to system (/etc/homecmdr/default.toml)...");
        crate::commands::config_sync::sync_workspace_config_to_system(&workspace)
            .context("failed to sync config to /etc/homecmdr/default.toml")?;
    }

    println!();
    println!("Plugin '{}' installed.", short_name(&canonical));
    println!();

    // Restart service if running — no rebuild needed.
    if is_service_active() {
        println!("Service is running — restarting to load the new plugin...");
        restart_service();
    } else {
        println!("To activate: start the HomeCmdr service or run the server directly.");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Config prompting
// ---------------------------------------------------------------------------

fn prompt_config_block(adapter: &str, config: &CliConfig) -> Result<String> {
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("[adapters.{}]", adapter));

    for field in &config.fields {
        let value = prompt_field(field)?;
        if value.is_empty() {
            continue;
        }
        let toml_line = format_toml_line(&field.key, &field.field_type, &value);
        lines.push(toml_line);
    }

    lines.push(String::new());
    Ok(lines.join("\n"))
}

fn prompt_field(field: &PluginField) -> Result<String> {
    let secret_hint = if field.secret { " (sensitive)" } else { "" };

    loop {
        if field.required {
            print!("  {} [required{}]: ", field.description, secret_hint);
        } else if let Some(ref default) = field.default {
            print!("  {} [{}{}]: ", field.description, default, secret_hint);
        } else {
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
                return Ok(String::new());
            }
            println!("  This field is required — please enter a value.");
            continue;
        }

        return Ok(trimmed);
    }
}

fn format_toml_line(key: &str, field_type: &str, value: &str) -> String {
    match field_type {
        "bool" | "u64" | "i64" | "f64" => format!("{} = {}", key, value),
        _ => format!("{} = {:?}", key, value),
    }
}

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

    writeln!(file)?;
    file.write_all(block.as_bytes())
        .with_context(|| format!("failed to write to {}", config_path.display()))?;

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

fn download_bytes(url: &str) -> Result<Vec<u8>> {
    use std::io::Read as _;
    let mut response = reqwest::blocking::get(url)
        .with_context(|| format!("request failed: {url}"))?
        .error_for_status()
        .with_context(|| format!("server returned error for: {url}"))?;
    let mut bytes = Vec::new();
    response
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read response body from: {url}"))?;
    Ok(bytes)
}
