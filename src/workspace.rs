use std::fs;
use std::path::PathBuf;

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
