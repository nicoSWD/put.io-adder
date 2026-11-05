# Platform layer notes

## What is verified where

| Area | macOS (this repo's dev machine) | Windows / Linux |
|---|---|---|
| paths, settings file, opener allow-list, player discovery, framing | unit tests | unit tests (same code) in CI |
| IPC over local sockets (forwarding, stale socket, hostile clients, caps, private directory) | unit tests | **CI only**; Windows named pipes are not exercised by the Unix-only socket tests |
| Apple Events (cold/warm magnet and `.torrent`) | `tools/probe-macos-intake.sh` (PASS) | n/a |
| Dock badge, notifications | main-thread/bundle guards tested; the notification banner itself and clicks are **not** automated | notification code is compile-checked by CI only |
| Keychain / Credential Manager / Secret Service | `cargo test -p putio-platform -- --ignored keyring` (touches the real keychain, run it deliberately) | same test, on a machine with a Secret Service / Credential Manager |

## Known gaps (deliberate, tracked)

- **Windows named pipe ACL.** The pipe uses `interprocess`' default security descriptor, and the name contains the user name.
  The server treats every frame as hostile (magic, length cap, deadline, UTF-8, then `Intake::parse`), so the worst a
  local process can do is add a *valid magnet link or `.torrent` path* to the user's account queue. An owner-only descriptor is a follow-up.
- **Notification clicks on Windows and Linux** are not reported (macOS opens the file on put.io). The notification still informs.
- **No file fallback for the token.** If no secret store is available the app must say so; it never writes the token to disk.
- **Linux Flatpak**: the sandbox needs the Secret Service / notification portals; Flatpak packaging is a later phase.
