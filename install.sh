#!/bin/bash
set -euo pipefail

# DevMan installer — downloads latest release, sets up service
REPO="ferguskendall2k/devman"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
SERVICE_URL="https://raw.githubusercontent.com/${REPO}/main/devman.service"

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

case "${OS}-${ARCH}" in
    Linux-x86_64)   ARTIFACT="devman-linux-x86_64" ;;
    Darwin-x86_64)  ARTIFACT="devman-macos-x86_64" ;;
    Darwin-arm64)   ARTIFACT="devman-macos-aarch64" ;;
    *)
        echo "Unsupported platform: ${OS}-${ARCH}"
        echo "Build from source: cargo install --git https://github.com/${REPO}"
        exit 1
        ;;
esac

# Get latest release URL
echo "🔧 Installing DevMan for ${OS} ${ARCH}..."
LATEST=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep "browser_download_url.*${ARTIFACT}" | cut -d '"' -f 4)

if [ -z "${LATEST}" ]; then
    echo "No release found. Build from source:"
    echo "  cargo install --git https://github.com/${REPO}"
    exit 1
fi

# Download binary
TMPFILE=$(mktemp)
echo "📥 Downloading ${LATEST}..."
curl -fsSL -o "${TMPFILE}" "${LATEST}"
chmod +x "${TMPFILE}"

# Install binary
if [ -w "${INSTALL_DIR}" ]; then
    mv "${TMPFILE}" "${INSTALL_DIR}/devman"
else
    echo "Installing to ${INSTALL_DIR} (requires sudo)..."
    sudo mv "${TMPFILE}" "${INSTALL_DIR}/devman"
fi

echo "✅ DevMan installed to ${INSTALL_DIR}/devman"

# Create memory directory
MEMORY_DIR=".devman/memory/tasks"
mkdir -p "${MEMORY_DIR}"
echo "📁 Memory directory created: ${MEMORY_DIR}"

# Run init if no config exists
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/devman"
if [ ! -f "${CONFIG_DIR}/config.toml" ]; then
    echo ""
    echo "🔧 Running initial setup..."
    "${INSTALL_DIR}/devman" init
else
    echo "📋 Config already exists: ${CONFIG_DIR}/config.toml"
fi

# Install systemd service (Linux only)
if [ "${OS}" = "Linux" ] && command -v systemctl &>/dev/null; then
    echo ""
    read -rp "🔄 Install systemd service for auto-restart? [y/N] " INSTALL_SERVICE
    if [[ "${INSTALL_SERVICE}" =~ ^[Yy] ]]; then
        # Download service file and patch paths
        TMPSERVICE=$(mktemp)
        curl -fsSL -o "${TMPSERVICE}" "${SERVICE_URL}"

        # Patch user and binary path
        CURRENT_USER=$(whoami)
        sed -i "s|User=fergus|User=${CURRENT_USER}|g" "${TMPSERVICE}"
        sed -i "s|ExecStart=.*|ExecStart=${INSTALL_DIR}/devman serve|g" "${TMPSERVICE}"
        sed -i "s|WorkingDirectory=.*|WorkingDirectory=${HOME}|g" "${TMPSERVICE}"
        sed -i "s|/home/fergus/.cargo/bin|${HOME}/.cargo/bin|g" "${TMPSERVICE}"

        sudo cp "${TMPSERVICE}" /etc/systemd/system/devman.service
        sudo systemctl daemon-reload
        sudo systemctl enable devman
        rm "${TMPSERVICE}"

        echo "✅ Systemd service installed and enabled"
        echo "   Start with: sudo systemctl start devman"
        echo "   Logs:       journalctl -u devman -f"
    fi
fi

echo ""
echo "🚀 Setup complete!"
echo ""
echo "Next steps:"
echo "  1. Add your Anthropic API key or Claude Code OAuth:"
echo "     Edit ${CONFIG_DIR}/credentials.toml"
echo "  2. Add a Telegram bot token (from @BotFather):"
echo "     Edit ${CONFIG_DIR}/credentials.toml"
echo "  3. Start DevMan:"
echo "     devman serve          # foreground"
echo "     sudo systemctl start devman  # background (if service installed)"
echo ""
echo "  devman chat    — interactive terminal chat"
echo "  devman serve   — Telegram bot + dashboard + cron"
echo "  devman cost    — usage and cost summary"
