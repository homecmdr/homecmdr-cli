use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;

use crate::workspace::resolve_workspace_root;

use super::add::{adapter_name, fetch_registry, short_name};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run() -> Result<()> {
    let workspace = resolve_workspace_root()?;
    let plugins_dir = workspace.join("config").join("plugins");

    // Determine which plugins are installed by scanning config/plugins/ for
    // .plugin.toml manifests.  This covers both WASM and IPC plugin types.
    // Each <adapter>.plugin.toml corresponds to an installed plugin.
    let installed_adapters: HashSet<String> = if plugins_dir.exists() {
        fs::read_dir(&plugins_dir)
            .with_context(|| format!("failed to read {}", plugins_dir.display()))?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let path = entry.path();
                let name = path.file_name().and_then(|n| n.to_str())?;
                name.strip_suffix(".plugin.toml").map(|s| s.to_string())
            })
            .collect()
    } else {
        HashSet::new()
    };

    // Fetch registry for the available-plugin list.
    println!("Fetching plugin registry...");
    let registry = fetch_registry()?;

    let mut installed = Vec::new();
    let mut available = Vec::new();

    for entry in &registry.plugins {
        let adapter = adapter_name(&entry.name);
        if installed_adapters.contains(&adapter) {
            installed.push(entry);
        } else {
            available.push(entry);
        }
    }

    // Also report any locally installed plugins not present in the registry
    // (e.g. custom or third-party plugins).
    let registry_adapters: HashSet<String> = registry
        .plugins
        .iter()
        .map(|p| adapter_name(&p.name))
        .collect();
    let local_only: Vec<&str> = installed_adapters
        .iter()
        .filter(|a| !registry_adapters.contains(*a))
        .map(|s| s.as_str())
        .collect();

    println!();
    println!("Installed plugins:");
    if installed.is_empty() && local_only.is_empty() {
        println!("  (none)");
    } else {
        for p in &installed {
            println!(
                "  [installed]  {}  v{}  — {}",
                short_name(&p.name),
                p.version,
                p.description
            );
        }
        for name in &local_only {
            println!("  [installed]  {}  (not in official registry)", name);
        }
    }

    println!();
    println!("Available plugins:");
    if available.is_empty() {
        println!("  (all official plugins are installed)");
    } else {
        for p in &available {
            println!(
                "  {}  v{}  — {}",
                short_name(&p.name),
                p.version,
                p.description
            );
        }
    }

    println!();
    println!("Add a plugin:    homecmdr plugin add <name>");
    println!("Remove a plugin: homecmdr plugin remove <name>");

    Ok(())
}
