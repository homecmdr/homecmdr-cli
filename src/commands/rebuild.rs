use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

use crate::workspace::find_workspace_root;

enum BuildMode {
    Cargo,
    Docker,
}

fn detect_build_mode(workspace_root: &Path) -> BuildMode {
    if workspace_root.join("docker-compose.yml").exists()
        || workspace_root.join("compose.yml").exists()
    {
        BuildMode::Docker
    } else {
        BuildMode::Cargo
    }
}

pub fn run() -> Result<()> {
    let workspace_root = find_workspace_root().context(
        "could not find a HomeCmdr workspace root (Cargo.toml with [workspace]) \
         in the current directory or any parent",
    )?;

    println!("Workspace: {}", workspace_root.display());

    let status = match detect_build_mode(&workspace_root) {
        BuildMode::Cargo => {
            println!("Build mode: cargo");
            println!("Running cargo build...");
            Command::new("cargo")
                .arg("build")
                .current_dir(&workspace_root)
                .status()
                .context("failed to launch cargo — is it installed and on PATH?")?
        }
        BuildMode::Docker => {
            println!("Build mode: docker compose (found compose file in workspace root)");
            println!("Running docker compose build...");
            println!("Note: run 'docker compose up -d' afterwards to restart with the new image.");
            Command::new("docker")
                .args(["compose", "build"])
                .current_dir(&workspace_root)
                .status()
                .context("failed to launch docker — is it installed and on PATH?")?
        }
    };

    if !status.success() {
        bail!("build failed (exit code {})", status.code().unwrap_or(-1));
    }

    println!("Build succeeded.");
    Ok(())
}
