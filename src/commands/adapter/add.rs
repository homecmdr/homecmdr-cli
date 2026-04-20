use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::fs;
use std::io::{self, Read};
use std::path::Path;

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
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(name: &str) -> Result<()> {
    let workspace = resolve_workspace_root()?;
    println!("Workspace: {}", workspace.display());

    // Fetch registry
    println!("Fetching adapter registry...");
    let registry = fetch_registry()?;

    // Find the requested adapter
    let entry = registry
        .adapters
        .iter()
        .find(|a| a.name == name)
        .ok_or_else(|| {
            let available: Vec<&str> = registry.adapters.iter().map(|a| a.name.as_str()).collect();
            anyhow!(
                "adapter '{}' not found in official registry.\nAvailable adapters:\n{}",
                name,
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
            "'{}' already exists at {}.\nRemove it first with 'homecmdr adapter remove {}' if you want to reinstall.",
            entry.name,
            dest.display(),
            entry.name,
        );
    }

    // Download and extract adapter crate
    println!("Downloading {}  v{}...", entry.display_name, entry.version);
    let zip_bytes = fetch_archive()?;
    extract_adapter(&zip_bytes, &entry.path, &dest)
        .context("failed to extract adapter from archive")?;
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

    // Patch 3: crates/adapters/src/lib.rs  ← this was the missing step
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

    println!();
    println!("{} added successfully.", entry.name);
    println!();
    println!(
        "Next: add an [adapters.{}] block to config/default.toml.",
        entry.name.replace("adapter-", "").replace('-', "_")
    );
    println!(
        "      Refer to {}/README.md for config options.",
        dest.display()
    );
    println!();

    // Rebuild
    crate::commands::build::run_cargo_build(&workspace, false)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

pub fn fetch_registry() -> Result<Registry> {
    let body = reqwest::blocking::get(REGISTRY_URL)
        .context("failed to fetch adapter registry")?
        .error_for_status()
        .context("registry returned an error status")?
        .text()
        .context("failed to read registry response")?;
    toml::from_str(&body).context("failed to parse adapter registry")
}

fn fetch_archive() -> Result<Vec<u8>> {
    let mut response = reqwest::blocking::get(ADAPTERS_ARCHIVE_URL)
        .context("failed to download adapters archive")?
        .error_for_status()
        .context("adapters archive returned an error status")?;
    let mut bytes = Vec::new();
    response
        .read_to_end(&mut bytes)
        .context("failed to read adapters archive")?;
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
            "no files found for adapter path '{}' in the archive — \
             check that the adapter name is correct",
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
