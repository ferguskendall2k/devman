#!/bin/bash
set -euo pipefail

# DevMan installer — downloads the latest release for your platform
REPO="ferguskendall2k/devman"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

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

# Download
TMPFILE=$(mktemp)
echo "Downloading ${LATEST}..."
curl -fsSL -o "${TMPFILE}" "${LATEST}"
chmod +x "${TMPFILE}"

# Install
if [ -w "${INSTALL_DIR}" ]; then
    mv "${TMPFILE}" "${INSTALL_DIR}/devman"
else
    echo "Installing to ${INSTALL_DIR} (requires sudo)..."
    sudo mv "${TMPFILE}" "${INSTALL_DIR}/devman"
fi

echo "✅ DevMan installed to ${INSTALL_DIR}/devman"
echo "   Run 'devman init' to get started"
