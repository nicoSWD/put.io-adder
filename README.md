# put.io adder

Click a `magnet:` link or open a `.torrent` file and it lands in your [put.io](https://put.io) queue.
A small cross-platform app for **macOS, Windows and Linux** (x86-64 and arm64), written in Rust.

- Registers as a handler for `magnet:` links and `.torrent` files
- Shows your transfers (progress, speed, ETA, size) and your account
- Notification when a download finishes
- Stream finished videos in [VLC](https://www.videolan.org/vlc/) (right-click a transfer)
- Optional: choose the put.io folder new transfers are saved in
- English, German and Spanish

This is a ground-up rewrite of the original Objective-C *put.io adder* for macOS.

## Install

Download the file for your system from the [latest release](https://github.com/nicoSWD/putio-adder-rs/releases/latest).

| System | File | Notes |
|---|---|---|
| macOS (Apple Silicon / Intel) | `putio-adder-vX.Y.Z-macos-arm64.dmg` / `-x64.dmg` | Drag the app to *Applications*. If the build is not notarized, right-click → *Open* once. |
| Windows (x64 / arm64) | `…-windows-x64-setup.exe` (or the `.zip`) | Installs for your user, no administrator rights. |
| Linux (x86_64 / aarch64) | `putio-adder-vX.Y.Z-linux-x86_64.tar.gz` | Unpack, run `./install.sh` (add `--set-default` to take over magnet links). Needs a Secret Service (GNOME Keyring, KWallet). |

### Make it your magnet-link handler

- **macOS:** open a `.torrent` in Finder → *Get Info* → *Open with* → put.io adder → *Change All…*. For magnet links, use a browser-independent handler tool or the first magnet link you click (macOS asks which app to use).
- **Windows:** *Settings → Apps → Default apps → put.io adder* (the installer offers to open it).
- **Linux:** `./install.sh --set-default`, or `xdg-mime default es.nico.putio-adder.desktop x-scheme-handler/magnet application/x-bittorrent`.

On first start the app shows a short code. Enter it at [put.io/link](https://put.io/link) to link your account. Your sign-in is kept in the system keychain (Keychain, Credential Manager or Secret Service), never in a file.

## Verify your download

Releases are built by GitHub Actions from a signed tag. Every file has a build-provenance attestation, and there is a `SHA256SUMS` file and a software bill of materials (`sbom.cdx.json`):

```sh
shasum -a 256 -c SHA256SUMS --ignore-missing
gh attestation verify <file> --repo nicoSWD/putio-adder-rs
```

## Build from source

```sh
cargo build --release -p putio-adder        # the binary: target/release/putio-adder
tools/bundle-macos.sh                       # macOS: target/put.io adder.app
```

Rust is pinned in `rust-toolchain.toml`. Everything builds with `--locked`.

## How it is built

A pure state machine (`putio-core`) decides what the app does; thin crates do the I/O (`putio-api` for put.io, `putio-platform` for the OS, `putio-adder` for the window). See [docs/architecture.md](docs/architecture.md). Security is the main design constraint: [SECURITY.md](SECURITY.md), [docs/dependencies.md](docs/dependencies.md), [docs/platform.md](docs/platform.md). Cutting a release: [docs/releasing.md](docs/releasing.md).

## Licence

MIT. The put.io name and logo belong to put.io. Montserrat is under the SIL Open Font License. See [assets/PROVENANCE.md](assets/PROVENANCE.md).
