# HomeCmdr CLI

The primary installation and management tool for [HomeCmdr](https://github.com/homecmdr/homecmdr-api) — a self-hosted home automation server.

The CLI handles everything: downloading the API source, interactive configuration, adding plugins, compiling, and deploying under systemd. No manual workspace cloning or `Cargo.toml` editing required.

## Prerequisites

- Rust toolchain (`cargo`) — install from [rustup.rs](https://rustup.rs/)
- Linux with systemd (for service deployment)
- `sudo` access (only required at deploy time)

HomeCmdr plugins are compile-time Rust crates linked into the API binary at build time, so a local Rust toolchain is required to build and customise the server.

## Installation

### One-liner

```bash
curl -sSf https://raw.githubusercontent.com/homecmdr/homecmdr-cli/main/install.sh | bash
```

Detects your architecture automatically (x86-64, aarch64, armv7) and installs the `homecmdr` CLI binary. No Rust required for the CLI itself.

### Via Cargo

```bash
cargo install --git https://github.com/homecmdr/homecmdr-cli
```

## Quick start

```bash
# 1. Download API source, generate config and master key
homecmdr init

# 2. Add plugins (interactive — prompts for each config value)
homecmdr plugin add zigbee2mqtt
homecmdr plugin add elgato-lights

# 3. Compile an optimised binary and install to /usr/local/bin/
homecmdr build --release

# 4. Create system user, install config, write systemd unit, start service
homecmdr service install

# 5. Check it started cleanly
homecmdr service logs
```

## Commands

### `homecmdr init [--dir <path>] [--force]`

Downloads the `homecmdr-api` source into `~/.local/share/homecmdr/workspace/` (or
a path of your choice), runs interactive prompts for timezone, location, bind
address, and database backend (SQLite or PostgreSQL), generates a random master
key, and writes `config/default.toml`. Offers to build the debug binary immediately.

### `homecmdr plugin add <name>`

Adds an official plugin to the workspace:

1. Fetches the plugin registry from [homecmdr/adapters](https://github.com/homecmdr/adapters)
2. Downloads and extracts the plugin crate into `crates/adapter-<name>/`
3. Patches `Cargo.toml`, `crates/adapters/Cargo.toml`, and `crates/adapters/src/lib.rs`
4. Reads `plugin.toml` from the crate and interactively prompts for all config values
5. Appends the completed `[adapters.<name>]` block to `config/default.toml`
6. Rebuilds the binary

Accepts either the short name (`zigbee2mqtt`) or the full name (`adapter-zigbee2mqtt`).

### `homecmdr plugin remove <name>`

Reverses `plugin add`: unpatches all three workspace files, removes the
`[adapters.<name>]` block from `config/default.toml`, deletes the crate directory,
and rebuilds.

### `homecmdr plugin list`

Shows installed plugins and available plugins from the official registry.

### `homecmdr build [--release]`

Builds the HomeCmdr binary inside the workspace.

- Without `--release`: debug build at `target/debug/api`
- With `--release`: optimised build, installs to `/usr/local/bin/homecmdr` (via sudo),
  and restarts the systemd service if it is already running

### `homecmdr service install`

Installs HomeCmdr as a systemd service:

1. Creates the `homecmdr` system user
2. Creates `/etc/homecmdr/` and `/var/lib/homecmdr/`
3. Copies and patches `config/default.toml` (rewrites relative paths to absolute)
4. Copies `config/scenes`, `config/automations`, and `config/scripts`
5. Writes `/etc/systemd/system/homecmdr.service`
6. Enables and starts the service

### `homecmdr service uninstall`

Stops, disables, and removes the systemd unit. Config and data directories are
preserved.

### `homecmdr service start|stop|restart|status|logs`

Wrappers around `systemctl` and `journalctl -u homecmdr -f`.

## How it works

HomeCmdr plugins are compile-time Rust crates that self-register with the runtime
via the `inventory` crate and are linked into the binary at build time. The CLI
automates all the workspace patching steps that would otherwise require manual
`Cargo.toml` edits, and drives configuration through a `plugin.toml` manifest
shipped with each plugin crate.

## Writing a plugin

Every plugin crate submitted to the official registry must include a `plugin.toml`
manifest alongside its `Cargo.toml`. This file declares the config block name and
each configuration field:

```toml
[config]
block = "adapters.my_plugin"

[[config.fields]]
key = "enabled"
type = "bool"
description = "Enable or disable this plugin"
default = "true"

[[config.fields]]
key = "host"
type = "string"
description = "Hostname or IP address of the device"
required = true

[[config.fields]]
key = "poll_interval_secs"
type = "u64"
description = "How often to poll for state changes (seconds)"
default = "30"

[[config.fields]]
key = "password"
type = "string"
description = "Device password"
optional = true
secret = true
```

Field attributes:

| Attribute | Effect |
|---|---|
| `default = "..."` | Pre-filled; user can press Enter to accept |
| `required = true` | User must provide a value; loops until non-empty |
| `optional = true` | User may leave blank; key is omitted from the config block if empty |
| `secret = true` | Marks the field as sensitive (stored plaintext) |

The CLI hard-fails if `plugin.toml` is absent — there is no fallback.
