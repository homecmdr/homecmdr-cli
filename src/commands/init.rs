use anyhow::{bail, Context, Result};
use rand::distributions::Alphanumeric;
use rand::Rng;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::workspace::{read_state, write_state, State};

// ---------------------------------------------------------------------------
// Release download URLs
//
// NOTE: These point to GitHub Releases for homecmdr/homecmdr-api.
// Release CI must publish binaries at these paths for the download to succeed.
// Architecture detection mirrors the pattern used in install.sh for the CLI.
// ---------------------------------------------------------------------------

const API_REPO: &str = "homecmdr/homecmdr-api";

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(dir: Option<PathBuf>, force: bool) -> Result<()> {
    // ── 1. Workspace directory ─────────────────────────────────────────────
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
            let existing_state = read_state();
            if existing_state.workspace_path.as_deref()
                == Some(workspace_dir.to_str().unwrap_or(""))
            {
                bail!(
                    "a HomeCmdr workspace already exists at {}.\n\
                     Re-run with --force to overwrite, or run 'homecmdr plugin add <name>' \
                     to add plugins.",
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

    // ── 2. Interactive configuration ───────────────────────────────────────
    let timezone = prompt("Timezone", "UTC")?;
    let latitude = prompt("Latitude", "51.5")?;
    let longitude = prompt("Longitude", "-0.1")?;
    let bind_address = prompt("API bind address", "127.0.0.1:3001")?;
    let (db_backend, db_url) = prompt_database()?;

    // ── 3. Generate master key ─────────────────────────────────────────────
    let master_key = generate_key(32);

    // ── 4. Create workspace directory structure ────────────────────────────
    println!();
    println!("Creating workspace directories...");
    for subdir in &[
        "config/plugins",
        "config/scenes",
        "config/automations",
        "config/scripts",
    ] {
        let path = workspace_dir.join(subdir);
        fs::create_dir_all(&path)
            .with_context(|| format!("failed to create {}", path.display()))?;
    }
    println!("  Created {}.", workspace_dir.display());

    // ── 5. Download HomeCmdr server binary ─────────────────────────────────
    println!();
    println!("Downloading HomeCmdr server binary...");
    let triple = detect_target_triple()?;
    let server_bin = workspace_dir.join("homecmdr-server");
    download_server_binary(&triple, &server_bin)?;
    println!("  Downloaded to {}.", server_bin.display());

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
    println!("Wrote config to {}.", config_path.display());

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
    println!("Workspace initialised successfully.");
    println!();
    println!("Next steps:");
    println!("  • Add plugins:  homecmdr plugin add <name>");
    println!("  • Deploy:       homecmdr service install");
    println!("  • List plugins: homecmdr plugin list");

    Ok(())
}

// ---------------------------------------------------------------------------
// Architecture detection
// ---------------------------------------------------------------------------

fn detect_target_triple() -> Result<String> {
    // uname -m on Linux
    let output = Command::new("uname")
        .arg("-m")
        .output()
        .context("failed to run 'uname -m'")?;

    let arch = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_lowercase();

    let triple = match arch.as_str() {
        "x86_64" => "x86_64-unknown-linux-gnu",
        "aarch64" | "arm64" => "aarch64-unknown-linux-gnu",
        other => bail!(
            "unsupported architecture: {}.\n\
             Supported: x86_64, aarch64 (Pi 4/5).\n\
             Please open an issue at https://github.com/homecmdr/homecmdr-cli",
            other
        ),
    };

    Ok(triple.to_string())
}

// ---------------------------------------------------------------------------
// Server binary download
// ---------------------------------------------------------------------------

fn download_server_binary(triple: &str, dest: &Path) -> Result<()> {
    // Fetch latest release tag from GitHub API
    let tag = fetch_latest_tag(API_REPO)
        .context("failed to determine the latest homecmdr-api release")?;

    let url = format!(
        "https://github.com/{API_REPO}/releases/download/{tag}/homecmdr-server-{triple}"
    );

    println!("  Release: {tag}");
    println!("  URL: {url}");

    let mut response = reqwest::blocking::get(&url)
        .context("failed to download server binary")?
        .error_for_status()
        .with_context(|| {
            format!(
                "server binary download returned an error.\n\
                 Check that release {tag} has a 'homecmdr-server-{triple}' asset at:\n  {url}"
            )
        })?;

    let mut bytes = Vec::new();
    response
        .read_to_end(&mut bytes)
        .context("failed to read server binary download")?;

    fs::write(dest, &bytes)
        .with_context(|| format!("failed to write binary to {}", dest.display()))?;

    // Mark executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(dest)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(dest, perms)?;
    }

    Ok(())
}

fn fetch_latest_tag(repo: &str) -> Result<String> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let body = reqwest::blocking::Client::new()
        .get(&url)
        .header("User-Agent", "homecmdr-cli")
        .send()
        .context("failed to query GitHub releases API")?
        .error_for_status()
        .context("GitHub releases API returned an error")?
        .text()
        .context("failed to read GitHub releases API response")?;

    // Works with both compact and pretty-printed JSON: locate `"tag_name":`
    // then skip optional whitespace and extract the quoted value.
    let key = "\"tag_name\":";
    body.find(key)
        .and_then(|pos| {
            let rest = &body[pos + key.len()..];
            let rest = rest.trim_start_matches([' ', '\n', '\r', '\t']);
            let rest = rest.strip_prefix('"')?;
            let end = rest.find('"')?;
            Some(rest[..end].to_string())
        })
        .ok_or_else(|| anyhow::anyhow!("could not parse tag_name from GitHub releases response"))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn default_workspace_dir() -> Result<PathBuf> {
    let data_dir = dirs::data_local_dir()
        .context("could not determine XDG data directory (~/.local/share)")?;
    Ok(data_dir.join("homecmdr").join("workspace"))
}

fn generate_key(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

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
        _ => Ok((
            "sqlite".to_string(),
            "sqlite://data/homecmdr.db".to_string(),
        )),
    }
}

fn prompt_secret(label: &str) -> Result<String> {
    print!("{} (leave blank for none): ", label);
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    Ok(buf.trim().to_string())
}

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

[plugins]
enabled = true
directory = "config/plugins"

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

# ── Adapters ─────────────────────────────────────────────────────────────────
# Add plugins with: homecmdr plugin add <name>
# Each plugin installed will append an [adapters.<name>] block here.

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
