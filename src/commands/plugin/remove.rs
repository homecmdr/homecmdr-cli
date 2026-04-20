use anyhow::{bail, Context, Result};
use std::fs;
use std::path::Path;

use crate::workspace::resolve_workspace_root;

use super::add::{canonical_name, short_name};

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

    // Remove the crate directory
    fs::remove_dir_all(&dest).with_context(|| format!("failed to remove {}", dest.display()))?;
    println!("  Removed {}.", dest.display());

    println!();
    println!("Plugin '{}' removed.", short_name(&canonical));

    let config_key = short_name(&canonical).replace('-', "_");
    println!(
        "Remember to also remove the [adapters.{}] block from config/default.toml.",
        config_key
    );
    println!();

    // Rebuild
    crate::commands::build::run_cargo_build(&workspace, false)?;

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
