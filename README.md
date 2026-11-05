# VoxLeaf

A fast, fully-local desktop library for your EPUBs — built-in reader, chapter audio player, and on-demand neural TTS. Built with Tauri 2 + Rust + React.

![VoxLeaf screenshot](./assets/screenshot.png)

## Features

- **EPUB library** — import and browse your own EPUB collection
- **Built-in reader** — full-screen reading view with chapter navigation
- **Chapter audio player** — listen to your books
- **Neural TTS** — on-demand text-to-speech narration
- **Local-first** — everything runs on your machine; no cloud, no account
- **Linux (.deb / portable) and Windows installers**
- **One-line install:** `curl -fsSL https://raw.githubusercontent.com/vivek-502/voxleaf/main/install.sh | bash`

## Installation

### Linux (one-liner)

```bash
curl -fsSL https://raw.githubusercontent.com/vivek-502/voxleaf/main/install.sh | bash
```

This downloads the latest portable binary, makes it executable, and leaves it as `./voxleaf`. Run it with `./voxleaf`.

### Linux (manual)

Download `voxleaf-<version>-x86_64-linux` from the [Releases](https://github.com/vivek-502/voxleaf/releases) page:

```bash
chmod +x voxleaf-*-x86_64-linux
./voxleaf-*-x86_64-linux
```

### Windows

Download `voxleaf-<version>-x86_64-pc-windows-gnu-setup.exe` from the [Releases](https://github.com/vivek-502/voxleaf/releases) page and run it.

A portable `voxleaf-<version>-x86_64-pc-windows-gnu.exe` is also available if you prefer not to install.

## Building from source

```bash
# clone
git clone https://github.com/vivek-502/voxleaf.git
cd voxleaf

# install frontend deps
npm install

# build frontend
npm run build

# build rust binary (release)
cargo build --release

# binary at: target/release/voicebook-desktop
```

## Release assets

Each release ships:

| Asset | Description |
|-------|-------------|
| `voxleaf-<ver>-x86_64-linux` | Portable Linux binary |
| `voxleaf-<ver>-x86_64-pc-windows-gnu.exe` | Portable Windows binary |
| `voxleaf-<ver>-x86_64-pc-windows-gnu-setup.exe` | Windows installer (NSIS) |
| `install.sh` | One-line Linux installer |

## License

MIT — see [LICENSE](./LICENSE).
