use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Persisted CLI state
// ---------------------------------------------------------------------------

/// State written to `~/.config/homecmdr/state.toml` by `homecmdr init`.
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct State {
    /// Absolute path to the HomeCmdr Cargo workspace.
    pub workspace_path: Option<String>,
}

/// Returns `~/.config/homecmdr/state.toml`, or `None` if the home directory
/// cannot be determined.
pub fn state_file_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("homecmdr").join("state.toml"))
}

/// Read persisted state.  Returns a default (empty) value if the file does
/// not exist or cannot be parsed.
pub fn read_state() -> State {
    let Some(path) = state_file_path() else {
        return State::default();
    };
    let Ok(content) = fs::read_to_string(&path) else {
        return State::default();
    };
    toml::from_str(&content).unwrap_or_default()
}

/// Persist state to `~/.config/homecmdr/state.toml`.
pub fn write_state(state: &State) -> Result<()> {
    let path = state_file_path().context("could not determine XDG config directory")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let content = toml::to_string_pretty(state).context("failed to serialize state")?;
    fs::write(&path, content)
        .with_context(|| format!("failed to write state to {}", path.display()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Workspace resolution
// ---------------------------------------------------------------------------

/// Walk up from the current directory until we find a `Cargo.toml` that
/// contains `[workspace]`.  Returns the directory containing that file.
pub fn find_workspace_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let candidate = dir.join("Cargo.toml");
        if candidate.exists() {
            let content = fs::read_to_string(&candidate).ok()?;
            if content.contains("[workspace]") {
                return Some(dir);
            }
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Resolve the workspace root:
/// 1. Check `~/.config/homecmdr/state.toml` for a configured workspace path.
/// 2. Fall back to walking up from the current working directory.
///
/// Returns an error with a helpful message if no workspace can be found.
pub fn resolve_workspace_root() -> Result<PathBuf> {
    let state = read_state();
    if let Some(path_str) = state.workspace_path {
        let path = PathBuf::from(&path_str);
        if path.exists() {
            return Ok(path);
        }
        eprintln!(
            "warning: configured workspace '{}' no longer exists, falling back to directory search",
            path_str
        );
    }

    find_workspace_root().with_context(|| {
        "could not find a HomeCmdr workspace root.\n\
         Run 'homecmdr init' to set up a workspace, or run this command from inside one."
    })
}
