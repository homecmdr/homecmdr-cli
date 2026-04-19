use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::fs;
use std::io::{self, Read};
use std::path::Path;

use crate::workspace::find_workspace_root;

const REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/homecmdr/adapters/main/adapters.toml";
const ADAPTERS_ARCHIVE_URL: &str =
    "https://github.com/homecmdr/adapters/archive/refs/heads/main.zip";

// ---------------------------------------------------------------------------
// Registry types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct Registry {
    adapters: Vec<AdapterEntry>,
}

#[derive(Deserialize)]
struct AdapterEntry {
    name: String,
    path: String,
    #[allow(dead_code)]
    display_name: String,
    #[allow(dead_code)]
    description: String,
    #[allow(dead_code)]
    version: String,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(name: &str) -> Result<()> {
    let workspace_root = find_workspace_root()
        .context("could not find a HomeCmdr workspace root (Cargo.toml with [workspace]) in the current directory or any parent")?;

    println!("Workspace: {}", workspace_root.display());

    // Fetch registry
    println!("Fetching registry...");
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

    let dest = workspace_root.join("crates").join(&entry.name);
    if dest.exists() {
        bail!(
            "'{}' already exists at {}. Remove it first if you want to reinstall.",
            entry.name,
            dest.display()
        );
    }

    // Download and extract
    println!("Downloading {}...", entry.name);
    let zip_bytes = fetch_archive()?;
    extract_adapter(&zip_bytes, &entry.path, &dest)
        .context("failed to extract adapter from archive")?;
    println!("Extracted to {}", dest.display());

    // Patch workspace Cargo.toml
    let workspace_toml = workspace_root.join("Cargo.toml");
    patch_workspace_toml(&workspace_toml, &entry.name)
        .context("failed to patch workspace Cargo.toml")?;

    // Patch crates/adapters/Cargo.toml
    let adapters_toml = workspace_root.join("crates").join("adapters").join("Cargo.toml");
    if adapters_toml.exists() {
        patch_adapters_toml(&adapters_toml, &entry.name)
            .context("failed to patch crates/adapters/Cargo.toml")?;
    } else {
        eprintln!(
            "warning: crates/adapters/Cargo.toml not found — skipping linker crate patch. \
             Add the adapter dependency manually."
        );
    }

    println!();
    println!("{} added successfully.", entry.name);
    println!("Rebuilding workspace...");
    println!();
    super::rebuild::run()?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

fn fetch_registry() -> Result<Registry> {
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

        // Strip the archive prefix to get the relative path within the adapter
        let relative = &raw_name[prefix.len()..];
        if relative.is_empty() {
            continue; // the directory entry itself
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

fn patch_workspace_toml(toml_path: &Path, adapter_name: &str) -> Result<()> {
    let content = fs::read_to_string(toml_path)?;
    let member = format!("\"crates/{}\"", adapter_name);

    if content.contains(&member) {
        println!("{} already in workspace members — skipping.", adapter_name);
        return Ok(());
    }

    // Insert before the closing bracket of the members array
    let patched = content.replacen(
        "]\nresolver",
        &format!("    {member},\n]\nresolver"),
        1,
    );

    if patched == content {
        bail!(
            "could not patch workspace Cargo.toml — members array format not recognised. \
             Add '{}' to [workspace] members manually.",
            member
        );
    }

    fs::write(toml_path, patched)?;
    println!("Patched workspace Cargo.toml.");
    Ok(())
}

fn patch_adapters_toml(toml_path: &Path, adapter_name: &str) -> Result<()> {
    let content = fs::read_to_string(toml_path)?;
    let dep_key = format!("{} =", adapter_name);

    if content.contains(&dep_key) {
        println!("{} already in crates/adapters/Cargo.toml — skipping.", adapter_name);
        return Ok(());
    }

    let new_dep = format!("{} = {{ path = \"../{}\" }}\n", adapter_name, adapter_name);
    let patched = if content.trim_end().ends_with('\n') {
        format!("{}{}", content, new_dep)
    } else {
        format!("{}\n{}", content, new_dep)
    };

    fs::write(toml_path, patched)?;
    println!("Patched crates/adapters/Cargo.toml.");
    Ok(())
}
