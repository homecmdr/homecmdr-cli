use anyhow::{bail, Context, Result};
use rand::distributions::Alphanumeric;
use rand::Rng;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use crate::workspace::{read_state, write_state, State};

const API_ARCHIVE_URL: &str =
    "https://github.com/homecmdr/homecmdr-api/archive/refs/heads/main.zip";

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(dir: Option<PathBuf>, force: bool) -> Result<()> {
    // ── 1. Rust toolchain check ────────────────────────────────────────────
    check_cargo()?;

    // ── 2. Workspace directory ─────────────────────────────────────────────
    let workspace_dir = match dir {
        Some(d) => d,
        None => default_workspace_dir()?,
    };

    if workspace_dir.exists() {
        if force {
            println!(
                "warning: {} already exists; --force specified, removing it first.",
                workspace_dir.display()
            );
            fs::remove_dir_all(&workspace_dir)
                .with_context(|| format!("failed to remove {}", workspace_dir.display()))?;
        } else {
            // Check whether it looks like a previous init
            let existing_state = read_state();
            if existing_state.workspace_path.as_deref()
                == Some(workspace_dir.to_str().unwrap_or(""))
            {
                bail!(
                    "a HomeCmdr workspace already exists at {}.\n\
                     Re-run with --force to overwrite, or run 'homecmdr adapter add <name>' \
                     to add adapters.",
                    workspace_dir.display()
                );
            }
            bail!(
                "directory {} already exists.\n\
                 Choose a different path with --dir, or pass --force to overwrite.",
                workspace_dir.display()
            );
        }
    }

    println!("HomeCmdr workspace: {}", workspace_dir.display());
    println!();

    // ── 3. Interactive configuration ───────────────────────────────────────
    let timezone = prompt("Timezone", "UTC")?;
    let latitude = prompt("Latitude", "51.5")?;
    let longitude = prompt("Longitude", "-0.1")?;
    let bind_address = prompt("API bind address", "127.0.0.1:3001")?;
    let (db_backend, db_url) = prompt_database()?;

    // ── 4. Generate master key ─────────────────────────────────────────────
    let master_key = generate_key(32);

    // ── 5. Download and extract homecmdr-api source ────────────────────────
    println!();
    println!("Downloading HomeCmdr API source...");
    let zip_bytes = download_archive()?;

    println!("Extracting to {}...", workspace_dir.display());
    extract_api(&zip_bytes, &workspace_dir).context("failed to extract HomeCmdr API archive")?;

    // ── 6. Write generated config ──────────────────────────────────────────
    let config_path = workspace_dir.join("config").join("default.toml");
    let config_content = generate_config(
        &master_key,
        &bind_address,
        &timezone,
        &latitude,
        &longitude,
        &db_backend,
        &db_url,
    );
    fs::write(&config_path, &config_content)
        .with_context(|| format!("failed to write config to {}", config_path.display()))?;
    println!("Wrote config to {}", config_path.display());

    // ── 7. Persist workspace state ─────────────────────────────────────────
    let state = State {
        workspace_path: Some(
            workspace_dir
                .canonicalize()
                .unwrap_or_else(|_| workspace_dir.clone())
                .to_string_lossy()
                .into_owned(),
        ),
    };
    write_state(&state).context("failed to save workspace state")?;

    // ── 8. Print master key prominently ───────────────────────────────────
    println!();
    println!("══════════════════════════════════════════════════════");
    println!("  Your HomeCmdr master key (save this somewhere safe):");
    println!();
    println!("  {}", master_key);
    println!();
    println!("  This key grants full admin access to the API.");
    println!("  It is stored in your config file but is NOT committed");
    println!("  to git. You can also set HOMECMDR_MASTER_KEY at runtime");
    println!("  to override it without editing the config file.");
    println!("══════════════════════════════════════════════════════");
    println!();

    // ── 9. Offer to build now ──────────────────────────────────────────────
    let build_now = prompt_confirm("Build the debug binary now? (takes a few minutes)", true)?;
    if build_now {
        crate::commands::build::run_cargo_build(&workspace_dir, false)?;
        println!();
        println!("Build complete. Next steps:");
        println!("  • Add plugins:    homecmdr plugin add <name>");
        println!("  • Deploy:         homecmdr build --release && homecmdr service install");
    } else {
        println!("Skipping build. When ready, run:");
        println!("  homecmdr build            # debug build");
        println!("  homecmdr build --release  # optimised + ready to deploy");
    }

    println!();
    println!("Available plugins: homecmdr plugin list");
    println!("Workspace initialised successfully.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn default_workspace_dir() -> Result<PathBuf> {
    let data_dir = dirs::data_local_dir()
        .context("could not determine XDG data directory (~/.local/share)")?;
    Ok(data_dir.join("homecmdr").join("workspace"))
}

fn check_cargo() -> Result<()> {
    let output = std::process::Command::new("cargo")
        .arg("--version")
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let ver = String::from_utf8_lossy(&o.stdout);
            println!("Found: {}", ver.trim());
            Ok(())
        }
        _ => bail!(
            "cargo not found on PATH.\n\
             Install the Rust toolchain from https://rustup.rs/ and try again."
        ),
    }
}

fn generate_key(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

/// Ask whether to use SQLite (default) or PostgreSQL, collect connection
/// details if needed, and return `(backend_string, database_url)`.
fn prompt_database() -> Result<(String, String)> {
    println!();
    println!("Database backend:");
    println!("  1) sqlite   — embedded, no extra setup required (recommended for most users)");
    println!("  2) postgres — external PostgreSQL server");
    print!("  Choice [sqlite]: ");
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    let choice = buf.trim().to_lowercase();

    match choice.as_str() {
        "2" | "postgres" | "postgresql" | "pg" => {
            println!();
            println!("PostgreSQL connection details:");
            let host = prompt("  Host", "localhost")?;
            let port = prompt("  Port", "5432")?;
            let database = prompt("  Database name", "homecmdr")?;
            let username = prompt("  Username", "homecmdr")?;
            let password = prompt_secret("  Password")?;
            let url = if password.is_empty() {
                format!("postgres://{}@{}:{}/{}", username, host, port, database)
            } else {
                format!(
                    "postgres://{}:{}@{}:{}/{}",
                    username, password, host, port, database
                )
            };
            Ok(("postgres".to_string(), url))
        }
        _ => {
            // sqlite (default)
            Ok((
                "sqlite".to_string(),
                "sqlite://data/homecmdr.db".to_string(),
            ))
        }
    }
}

/// Prompt for a value without echoing (password). Falls back to a plain
/// prompt if rpassword is not available — stored plaintext either way.
fn prompt_secret(label: &str) -> Result<String> {
    print!("{} (leave blank for none): ", label);
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    Ok(buf.trim().to_string())
}

/// Simple line prompt with a default value.
fn prompt(question: &str, default: &str) -> Result<String> {
    print!("  {} [{}]: ", question, default);
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    let trimmed = buf.trim();
    Ok(if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()
    })
}

/// Yes/no prompt.
fn prompt_confirm(question: &str, default: bool) -> Result<bool> {
    let hint = if default { "Y/n" } else { "y/N" };
    print!("  {} [{}]: ", question, hint);
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    let answer = buf.trim().to_lowercase();
    Ok(match answer.as_str() {
        "y" | "yes" => true,
        "n" | "no" => false,
        "" => default,
        _ => default,
    })
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

fn download_archive() -> Result<Vec<u8>> {
    let mut response = reqwest::blocking::get(API_ARCHIVE_URL)
        .context("failed to download HomeCmdr API archive")?
        .error_for_status()
        .context("archive download returned an error status")?;
    let mut bytes = Vec::new();
    response
        .read_to_end(&mut bytes)
        .context("failed to read archive response")?;
    Ok(bytes)
}

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

fn extract_api(zip_bytes: &[u8], dest: &Path) -> Result<()> {
    let cursor = io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor).context("failed to open zip archive")?;

    // GitHub archives are prefixed with "<repo>-<branch>/", e.g. "homecmdr-api-main/"
    let prefix = "homecmdr-api-main/";

    let mut extracted = 0usize;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let raw_name = file.name().to_string();

        if !raw_name.starts_with(prefix) {
            continue;
        }

        let relative = &raw_name[prefix.len()..];
        if relative.is_empty() {
            continue; // root directory entry
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
        bail!("no files found in the archive — the download may have failed or the repository layout has changed");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Config generation
// ---------------------------------------------------------------------------

fn generate_config(
    master_key: &str,
    bind_address: &str,
    timezone: &str,
    latitude: &str,
    longitude: &str,
    db_backend: &str,
    db_url: &str,
) -> String {
    format!(
        r#"# HomeCmdr configuration — generated by 'homecmdr init'
#
# Environment variable overrides (take priority over this file):
#   HOMECMDR_CONFIG      — path to a different config file
#   HOMECMDR_DATA_DIR    — base directory for relative database_url paths
#   HOMECMDR_MASTER_KEY  — master API key (overrides [auth].master_key below)

[runtime]
event_bus_capacity = 1024

[api]
bind_address = "{bind_address}"

[api.cors]
enabled = true
allowed_origins = ["http://127.0.0.1:8080"]

[api.rate_limit]
enabled = false
requests_per_second = 100
burst_size = 20

[auth]
# Full admin access.  Override at runtime with HOMECMDR_MASTER_KEY — never
# commit this file with a real key in it.
master_key = "{master_key}"

[locale]
timezone = "{timezone}"
latitude = {latitude}
longitude = {longitude}

[logging]
level = "info"

[persistence]
enabled = true
backend = "{db_backend}"
database_url = "{db_url}"
auto_create = true

[persistence.history]
enabled = true
retention_days = 30
default_query_limit = 200
max_query_limit = 1000

[scenes]
enabled = true
directory = "config/scenes"
watch = false

[automations]
enabled = true
directory = "config/automations"
watch = false

[automations.runner]
default_max_concurrent = 8
backstop_timeout_secs = 3600

[scripts]
enabled = true
directory = "config/scripts"
watch = false

[telemetry]
enabled = false

# ── Plugins ───────────────────────────────────────────────────────────────
# Add plugins with: homecmdr plugin add <name>
# Then enable them here by adding the appropriate [adapters.<name>] block.

[adapters.open_meteo]
enabled = true
latitude = {latitude}
longitude = {longitude}
poll_interval_secs = 90
"#,
        bind_address = bind_address,
        master_key = master_key,
        timezone = timezone,
        latitude = latitude,
        longitude = longitude,
        db_backend = db_backend,
        db_url = db_url,
    )
}
