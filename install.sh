#!/bin/bash
# VoxLeaf — one-line Linux installer
# Usage: curl -fsSL https://raw.githubusercontent.com/vivek-502/voxleaf/main/install.sh | bash
set -euo pipefail

VERSION="${VOXLEAF_VERSION:-1.0.1}"
REPO="vivek-502/voxleaf"
ARCH="$(uname -m)"

case "$ARCH" in
  x86_64) ARTIFACT="voxleaf-${VERSION}-x86_64-linux" ;;
  aarch64|arm64) ARTIFACT="voxleaf-${VERSION}-aarch64-linux" ;;
  *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"
DESKTOP_FILE="$APP_DIR/voxleaf.desktop"

URL="https://github.com/${REPO}/releases/download/v${VERSION}/${ARTIFACT}"
ICON_URL="https://raw.githubusercontent.com/${REPO}/main/icon.png"

echo "==> VoxLeaf installer v${VERSION}"
echo "==> Architecture: $ARCH"

mkdir -p "$BIN_DIR" "$APP_DIR" "$ICON_DIR"

echo "==> Downloading binary to $BIN_DIR/voxleaf ..."
tmp_bin="$(mktemp)"
curl -fL --progress-bar -o "$tmp_bin" "$URL"
chmod +x "$tmp_bin"
mv "$tmp_bin" "$BIN_DIR/voxleaf"

echo "==> Downloading app icon ..."
curl -fL --progress-bar -o "$ICON_DIR/voxleaf.png" "$ICON_URL"

echo "==> Creating desktop entry ..."
cat > "$DESKTOP_FILE" <<EOF
[Desktop Entry]
Name=VoxLeaf
GenericName=EPUB Library & Reader
Comment=Fast, fully-local desktop library for your EPUBs — reader, chapter audio, neural TTS
Exec=$BIN_DIR/voxleaf
Icon=$ICON_DIR/voxleaf.png
Type=Application
Categories=AudioVideo;Player;Office;
Terminal=false
StartupNotify=true
EOF

chmod +x "$DESKTOP_FILE"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APP_DIR" 2>/dev/null || true
fi

echo
echo "==> Installed successfully!"
echo "==> Binary:   $BIN_DIR/voxleaf"
echo "==> App menu: $DESKTOP_FILE"
echo
echo "Launch VoxLeaf from your app launcher, or run: $BIN_DIR/voxleaf"
echo
echo "Tip: Add $BIN_DIR to your PATH so you can run \`voxleaf\` from any terminal:"
echo "      echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.bashrc"
