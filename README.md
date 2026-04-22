# HomeCmdr CLI

The primary installation and management tool for [HomeCmdr](https://github.com/homecmdr/homecmdr-api) — a self-hosted home automation server.

The CLI handles everything: downloading the server binary, interactive configuration, adding WASM plugins, and deploying under systemd. No Rust toolchain required.

## Prerequisites

- Linux with systemd (for service deployment)
- `sudo` access (only required at deploy time)

HomeCmdr plugins are pre-compiled WASM binaries. No Rust toolchain is needed to install or manage them.

## Installation

### One-liner

```bash
curl -sSf https://raw.githubusercontent.com/homecmdr/homecmdr-cli/main/install.sh | bash
```

Detects your architecture automatically (x86-64, aarch64, armv7) and installs the `homecmdr` CLI binary.

### Via Cargo

```bash
cargo install --git https://github.com/homecmdr/homecmdr-cli
```

## Quick start

```bash
# 1. Download server binary, generate config and master key
homecmdr init

# 2. Add plugins (interactive — prompts for each config value)
homecmdr plugin add zigbee2mqtt
homecmdr plugin add elgato-lights

# 3. Create system user, install config, write systemd unit, start service
homecmdr service install

# 4. Check it started cleanly
homecmdr service logs
```

## Commands

### `homecmdr init [--dir <path>] [--force]`

Downloads the `homecmdr-server` binary for your architecture from the latest
[homecmdr-api release](https://github.com/homecmdr/homecmdr-api/releases), runs
interactive prompts for timezone, location, bind address, and database backend
(SQLite or PostgreSQL), generates a random master key, and writes
`config/default.toml` plus all required subdirectories.

The workspace is created at `~/.local/share/homecmdr/workspace/` by default.

### `homecmdr plugin add <name>`

Installs an official WASM plugin:

1. Fetches the plugin registry from [homecmdr/plugins](https://github.com/homecmdr/plugins)
2. Downloads `<name>.wasm` and `<name>.plugin.toml` into `config/plugins/`
3. Reads the `[[config.fields]]` section from the manifest and interactively
   prompts for all config values
4. Appends the completed `[adapters.<name>]` block to `config/default.toml`
5. Restarts the service if it is already running (no recompile needed)

Accepts either the short name (`zigbee2mqtt`) or the full name (`plugin-zigbee2mqtt`).

### `homecmdr plugin remove <name>`

Removes a plugin:

1. Deletes `config/plugins/<name>.wasm` and `config/plugins/<name>.plugin.toml`
2. Removes the `[adapters.<name>]` block from `config/default.toml`
3. Restarts the service if running

### `homecmdr plugin list`

Shows installed plugins (detected from `config/plugins/*.wasm`) and available
plugins from the official registry.

### `homecmdr service install`

Installs HomeCmdr as a systemd service:

1. Copies `homecmdr-server` to `/usr/local/bin/`
2. Creates the `homecmdr` system user
3. Creates `/etc/homecmdr/` and `/var/lib/homecmdr/`
4. Copies and patches `config/default.toml` (rewrites relative paths to absolute)
5. Copies `config/plugins`, `config/scenes`, `config/automations`, and `config/scripts`
6. Writes `/etc/systemd/system/homecmdr.service`
7. Enables and starts the service

### `homecmdr service uninstall`

Stops, disables, and removes the systemd unit. Config and data directories are
preserved.

### `homecmdr service start|stop|restart|status|logs`

Wrappers around `systemctl` and `journalctl -u homecmdr -f`.

## How plugins work

HomeCmdr plugins are pre-compiled WASM binaries (`*.wasm`). The server loads them
at startup from `config/plugins/` using the wasmtime component model. Each plugin
pairs with a `*.plugin.toml` manifest that declares its name and poll interval.

Adding a plugin is three steps:

1. Drop `<name>.wasm` + `<name>.plugin.toml` into `config/plugins/`
2. Add `[adapters.<name>]` config block to `config/default.toml`
3. Restart the server

`homecmdr plugin add` automates all three steps.

## Writing a plugin

Every plugin submitted to the official registry must include a merged
`.plugin.toml` manifest. The `[plugin]` and `[runtime]` sections are read by
the WASM host at runtime. The `[[config.fields]]` section is used only by the
CLI for interactive config prompting — the host ignores it.

```toml
[plugin]
name        = "my_plugin"
version     = "0.1.0"
description = "My HomeCmdr plugin"
api_version = "0.1.0"

[runtime]
poll_interval_secs = 60

# CLI config prompting — ignored by the WASM host
[[config.fields]]
key         = "enabled"
type        = "bool"
description = "Enable or disable this plugin"
default     = "true"

[[config.fields]]
key         = "host"
type        = "string"
description = "Hostname or IP address of the device"
required    = true

[[config.fields]]
key         = "poll_interval_secs"
type        = "u64"
description = "How often to poll for state changes (seconds)"
default     = "30"

[[config.fields]]
key         = "password"
type        = "string"
description = "Device password"
optional    = true
secret      = true
```

Field attributes:

| Attribute | Effect |
|---|---|
| `default = "..."` | Pre-filled; user can press Enter to accept |
| `required = true` | User must provide a value; loops until non-empty |
| `optional = true` | User may leave blank; key is omitted from the config block if empty |
| `secret = true` | Marks the field as sensitive (stored plaintext) |

See [plugin_authoring_guide.md](https://github.com/homecmdr/homecmdr-api/blob/main/config/docs/plugin_authoring_guide.md)
for the full guide on building a WASM plugin from scratch.
