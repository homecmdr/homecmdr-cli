use anyhow::{Context, Result};
use std::fs;

use crate::workspace::resolve_workspace_root;

use super::add::{fetch_registry, short_name, AdapterEntry};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run() -> Result<()> {
    let workspace = resolve_workspace_root()?;

    println!("Fetching plugin registry...");
    let registry = fetch_registry()?;

    let workspace_toml = workspace.join("Cargo.toml");
    let workspace_contents = fs::read_to_string(&workspace_toml)
        .with_context(|| format!("failed to read {}", workspace_toml.display()))?;

    let installed: Vec<&AdapterEntry> = registry
        .adapters
        .iter()
        .filter(|a| workspace_contents.contains(&format!("\"crates/{}\"", a.name)))
        .collect();

    let available: Vec<&AdapterEntry> = registry
        .adapters
        .iter()
        .filter(|a| !workspace_contents.contains(&format!("\"crates/{}\"", a.name)))
        .collect();

    println!();
    println!("Installed plugins:");
    if installed.is_empty() {
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
