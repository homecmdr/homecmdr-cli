use anyhow::{bail, Context, Result};
use std::process::Command;

use crate::workspace::find_workspace_root;

pub fn run() -> Result<()> {
    let workspace_root = find_workspace_root().context(
        "could not find a HomeCmdr workspace root (Cargo.toml with [workspace]) \
         in the current directory or any parent",
    )?;

    println!("Workspace: {}", workspace_root.display());
    println!("Running cargo build...");

    let status = Command::new("cargo")
        .arg("build")
        .current_dir(&workspace_root)
        .status()
        .context("failed to launch cargo — is it installed and on PATH?")?;

    if !status.success() {
        bail!(
            "cargo build failed (exit code {})",
            status.code().unwrap_or(-1)
        );
    }

    println!("Build succeeded.");
    Ok(())
}
