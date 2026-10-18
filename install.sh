#!/usr/bin/env bash
#
# VoxLeaf installer for Linux.
#
#   curl -fsSL https://raw.githubusercontent.com/vivek-502/voxleaf/main/install.sh | bash
#
# Downloads the portable build of VoxLeaf from the GitHub releases and installs
# it for the current user (no root needed):
#
#   ~/.local/bin/voxleaf              the app
#   ~/.local/share/icons/hicolor/512x512/apps/voxleaf.png
#   ~/.local/share/applications/voxleaf.desktop
#
# Options (pass after `bash -s --`):
#   1.0.0            install a specific version instead of the latest
#   --uninstall      remove everything this script installed
#
set -euo pipefail

REPO="vivek-502/voxleaf"
ZIP_NAME="VoxLeaf-linux-x64-portable.zip"
BIN_NAME="voxleaf"

BIN_DIR="$HOME/.local/bin"
ICON_DIR="$HOME/.local/share/icons/hicolor/512x512/apps"
APP_DIR="$HOME/.local/share/applications"
BIN_PATH="$BIN_DIR/$BIN_NAME"
ICON_PATH="$ICON_DIR/$BIN_NAME.png"
DESKTOP_PATH="$APP_DIR/$BIN_NAME.desktop"

say()  { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarning:\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

# ---------------------------------------------------------------- uninstall
if [ "${1:-}" = "--uninstall" ]; then
  removed=0
  for f in "$BIN_PATH" "$ICON_PATH" "$DESKTOP_PATH"; do
    if [ -e "$f" ]; then rm -f "$f"; say "removed $f"; removed=1; fi
  done
  [ "$removed" -eq 1 ] || say "nothing to remove — VoxLeaf does not appear to be installed"
  exit 0
fi

VERSION="${1:-latest}"

# ------------------------------------------------------------ preflight
[ "$(uname -s)" = "Linux" ] || die "this installer supports Linux only"
case "$(uname -m)" in
  x86_64|amd64) ;;
  *) die "no prebuilt VoxLeaf for $(uname -m) — build from source instead (see README)" ;;
esac

for cmd in curl unzip; do
  command -v "$cmd" >/dev/null 2>&1 || die "'$cmd' is required. Install it first:
    Debian/Ubuntu : sudo apt install $cmd
    Fedora        : sudo dnf install $cmd
    Arch          : sudo pacman -S $cmd"
done

# WebKitGTK 4.1 is the one real runtime dependency of a Tauri app on Linux.
# (grep on a variable, not a pipe: `pipefail` + `grep -q` racing ldconfig's
# SIGPIPE produces false negatives.)
if command -v ldconfig >/dev/null 2>&1; then
  ld_out="$(ldconfig -p 2>/dev/null || :)"
  if ! grep -q 'libwebkit2gtk-4\.1' <<<"$ld_out"; then
    warn "the WebKitGTK 4.1 runtime was not found — VoxLeaf will not start without it:"
    warn "    Debian/Ubuntu : sudo apt install libwebkit2gtk-4.1-0"
    warn "    Fedora        : sudo dnf install webkit2gtk4.1"
    warn "    Arch          : sudo pacman -S webkit2gtk-4.1"
  fi
fi

# ------------------------------------------------------------- download
if [ "$VERSION" = "latest" ]; then
  URL="https://github.com/$REPO/releases/latest/download/$ZIP_NAME"
else
  URL="https://github.com/$REPO/releases/download/v${VERSION#v}/$ZIP_NAME"
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
say "Downloading $URL"
curl -fSL --progress-bar -o "$TMP/$ZIP_NAME" "$URL" \
  || die "download failed — check the version tag at https://github.com/$REPO/releases"

unzip -q "$TMP/$ZIP_NAME" -d "$TMP/pkg"

# Archives have shipped both flat (voxleaf at the root) and wrapped in a
# top-level folder — find the binary wherever it lives instead of assuming.
BIN_IN_ZIP="$(find "$TMP/pkg" -maxdepth 3 -type f -name "$BIN_NAME" | head -n1)"
[ -n "$BIN_IN_ZIP" ] || die "unexpected archive layout: no '$BIN_NAME' inside the zip:
$(find "$TMP/pkg" -maxdepth 2 | sed 's/^/    /')"
PAYLOAD="$(dirname "$BIN_IN_ZIP")"

# -------------------------------------------------------------- install
mkdir -p "$BIN_DIR" "$ICON_DIR" "$APP_DIR"
install -m 755 "$PAYLOAD/$BIN_NAME" "$BIN_PATH"
install -m 644 "$PAYLOAD/icon.png"   "$ICON_PATH"

cat > "$DESKTOP_PATH" <<EOF
[Desktop Entry]
Type=Application
Name=VoxLeaf
GenericName=Audiobook Library
Comment=Read and listen to your own EPUB and audio library
Exec=$BIN_PATH
Icon=$BIN_NAME
Terminal=false
StartupNotify=true
StartupWMClass=$BIN_NAME
Categories=AudioVideo;Audio;Literature;
Keywords=audiobook;epub;books;tts;library;player;
EOF
chmod 644 "$DESKTOP_PATH"

command -v update-desktop-database >/dev/null 2>&1 \
  && update-desktop-database "$APP_DIR" 2>/dev/null || true
command -v gtk-update-icon-cache >/dev/null 2>&1 \
  && gtk-update-icon-cache -q -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true

# --------------------------------------------------------------- finish
say "VoxLeaf installed to $BIN_PATH"
# `case` instead of a pipe: grep -q can exit on the first PATH entry and the
# same pipefail/SIGPIPE race would print a bogus warning.
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *)  warn "$BIN_DIR is not on your PATH — add this to your shell profile:"
      warn "    export PATH=\"\$HOME/.local/bin:\$PATH\"" ;;
esac
say "Launch with:  $BIN_NAME     (or find 'VoxLeaf' in your app menu)"
say "Uninstall:    curl -fsSL https://raw.githubusercontent.com/$REPO/main/install.sh | bash -s -- --uninstall"
