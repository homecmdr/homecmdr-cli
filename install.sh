#!/usr/bin/env bash
# HomeCmdr CLI installer
#
# Usage:
#   curl -sSf https://raw.githubusercontent.com/homecmdr/homecmdr-cli/main/install.sh | bash
#
# Override the install directory:
#   HOMECMDR_INSTALL_DIR=~/.local/bin \
#     curl -sSf .../install.sh | bash

set -euo pipefail

REPO="homecmdr/homecmdr-cli"
BIN_NAME="homecmdr"
INSTALL_DIR="${HOMECMDR_INSTALL_DIR:-/usr/local/bin}"

# ── Platform detection ───────────────────────────────────────────────────────

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux) ;;
  *)
    echo "error: HomeCmdr CLI installer only supports Linux." >&2
    echo "       Install via Rust instead:" >&2
    echo "         cargo install --git https://github.com/$REPO" >&2
    exit 1
    ;;
esac

case "$ARCH" in
  x86_64)
    TRIPLE="x86_64-unknown-linux-gnu"
    ;;
  aarch64 | arm64)
    # Pi 4/5 (64-bit OS), Pi Zero 2W
    TRIPLE="aarch64-unknown-linux-gnu"
    ;;
  armv7l | armv7*)
    # Pi 2/3/4 running 32-bit OS
    TRIPLE="armv7-unknown-linux-gnueabihf"
    ;;
  armv6l | arm*)
    # Older Pi models — use the armv7 binary (runs on ARMv6 with EABI hardfp
    # if the kernel supports it; if not, fall through to the cargo path)
    TRIPLE="armv7-unknown-linux-gnueabihf"
    echo "note: Detected ARMv6 ($ARCH). Downloading the armv7 binary;" >&2
    echo "      if it fails to run, install via: cargo install --git https://github.com/$REPO" >&2
    ;;
  *)
    echo "error: Unsupported architecture: $ARCH" >&2
    echo "       Install via Rust instead:" >&2
    echo "         cargo install --git https://github.com/$REPO" >&2
    exit 1
    ;;
esac

# ── Fetch latest release tag ─────────────────────────────────────────────────

echo "Fetching latest HomeCmdr CLI release..."

TAG=$(curl -sSf "https://api.github.com/repos/$REPO/releases/latest" \
      | grep '"tag_name"' \
      | sed -E 's/.*"([^"]+)".*/\1/')

if [ -z "$TAG" ]; then
  echo "error: Could not determine the latest release tag." >&2
  echo "       Check https://github.com/$REPO/releases or install via:" >&2
  echo "         cargo install --git https://github.com/$REPO" >&2
  exit 1
fi

DOWNLOAD_URL="https://github.com/$REPO/releases/download/$TAG/${BIN_NAME}-${TRIPLE}"

# ── Download ─────────────────────────────────────────────────────────────────

echo "Downloading HomeCmdr CLI $TAG for $TRIPLE..."

TMP="$(mktemp)"
trap 'rm -f "$TMP"' EXIT

if ! curl -sSfL "$DOWNLOAD_URL" -o "$TMP"; then
  echo "error: Download failed." >&2
  echo "       URL: $DOWNLOAD_URL" >&2
  echo "       Install via: cargo install --git https://github.com/$REPO" >&2
  exit 1
fi

chmod +x "$TMP"

# ── Install ───────────────────────────────────────────────────────────────────

echo "Installing to $INSTALL_DIR/$BIN_NAME..."

if [ -w "$INSTALL_DIR" ]; then
  mv "$TMP" "$INSTALL_DIR/$BIN_NAME"
else
  echo "  (writing to $INSTALL_DIR requires sudo)"
  sudo mv "$TMP" "$INSTALL_DIR/$BIN_NAME"
fi

echo ""
echo "Installed: $("$INSTALL_DIR/$BIN_NAME" --version)"
echo ""
echo "Get started:"
echo "  homecmdr init"
