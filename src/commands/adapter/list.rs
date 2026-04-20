use anyhow::{Context, Result};
use std::fs;

use crate::workspace::resolve_workspace_root;

use super::add::{fetch_registry, AdapterEntry};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run() -> Result<()> {
    let workspace = resolve_workspace_root()?;

    println!("Fetching adapter registry...");
    let registry = fetch_registry()?;

    // Determine which adapters are already installed in the workspace
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
    println!("Installed adapters:");
    if installed.is_empty() {
        println!("  (none)");
    } else {
        for a in &installed {
            println!(
                "  [installed]  {}  v{}  — {}",
                a.name, a.version, a.description
            );
        }
    }

    println!();
    println!("Available adapters:");
    if available.is_empty() {
        println!("  (all official adapters are installed)");
    } else {
        for a in &available {
            println!("  {}  v{}  — {}", a.name, a.version, a.description);
        }
    }

    println!();
    println!("Add an adapter:    homecmdr adapter add <name>");
    println!("Remove an adapter: homecmdr adapter remove <name>");

    Ok(())
}
