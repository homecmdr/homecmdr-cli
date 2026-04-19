# HomeCmdr CLI

Command-line tool for managing adapters in a [HomeCmdr](https://github.com/homecmdr/homecmdr-api) workspace.

## Installation

```bash
cargo install --git https://github.com/homecmdr/homecmdr-cli
```

Or clone and build locally:

```bash
git clone https://github.com/homecmdr/homecmdr-cli
cd homecmdr-cli
cargo install --path .
```

## Usage

Run commands from inside your `homecmdr-api` workspace directory (or any subdirectory).

### Pull an adapter

```bash
homecmdr pull adapter-elgato-lights
```

This will:

1. Locate your workspace root by walking up the directory tree for a `Cargo.toml` containing `[workspace]`
2. Fetch the official adapter registry from [homecmdr/adapters](https://github.com/homecmdr/adapters)
3. Download and extract the adapter crate into `crates/adapter-elgato-lights/`
4. Patch the workspace `Cargo.toml` to add it as a member
5. Patch `crates/adapters/Cargo.toml` to link it into the binary

Then rebuild:

```bash
cargo build
```

Enable the adapter in `config/default.toml` (refer to the adapter's README for config options).

## Available Adapters

See [github.com/homecmdr/adapters](https://github.com/homecmdr/adapters) for the full list.

## How It Works

HomeCmdr adapters are compile-time Rust crates. They self-register with the runtime
via the `inventory` crate and are linked into the binary at build time. The CLI
automates the steps that would otherwise require manual edits to `Cargo.toml` files.

## Prerequisites

- Rust toolchain (`cargo`)
- A local clone of [homecmdr-api](https://github.com/homecmdr/homecmdr-api)
