#!/bin/bash
# VoxLeaf — one-line Linux installer
# Usage: curl -fsSL https://raw.githubusercontent.com/vivek-502/voxleaf/main/install.sh | bash
set -euo pipefail

VERSION="${VOXLEAF_VERSION:-1.0.0}"
REPO="vivek-502/voxleaf"
ARCH="$(uname -m)"

case "$ARCH" in
  x86_64) ARTIFACT="voxleaf-${VERSION}-x86_64-linux" ;;
  aarch64|arm64) ARTIFACT="voxleaf-${VERSION}-aarch64-linux" ;;
  *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

URL="https://github.com/${REPO}/releases/download/v${VERSION}/${ARTIFACT}"

echo "==> Downloading VoxLeaf v${VERSION} (${ARCH})..."
curl -fL --progress-bar -o voxleaf "$URL"
chmod +x voxleaf

echo "==> Installed ./voxleaf"
echo "==> Run: ./voxleaf"
