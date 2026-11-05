# Platform layer notes

## What is verified where

| Area | macOS (this repo's dev machine) | Windows / Linux |
|---|---|---|
| paths, settings file, opener allow-list, player discovery, framing | unit tests | the same tests run in CI. Path logic is decided from the *target OS*, not the host (`paths::is_absolute_for`), so the macOS/Linux/Windows cases hold on any runner. **Never run on Windows yet** |
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

## Single-instance rules (`ipc`)

A running instance is never replaced. Start-up takes a lock file (`ipc.lock`, in the private socket directory), then:

1. connect and forward the message → done (`Forwarded`);
2. connect **refused** or no socket → nobody is listening: remove the stale file, bind (no `try_overwrite`: that option
   deletes the socket on any `AddrInUse`, which steals a live instance's socket), become the primary;
3. anything else (timeout, rejected frame) → return an error; **do not** start a second instance.

Regression tests: a live socket cannot be re-bound, eight simultaneous launches yield exactly one primary, a hung
instance's socket is not deleted, a stale socket from a crash is replaced, and excess connections are dropped and recover.

## Wiring notes for the UI phase

- Call `macos_events::install` (macOS) **and** `notify::system_notifier` in `main`, before the event loop: a click
  on a notification that launches the app cold is only delivered if the delegate exists before launch finishes.
- Read `.torrent` files only through `torrent_file::read_torrent` (regular files only, size-capped, bounded read).
  A path from argv or IPC can be a FIFO or `/dev/zero`.

## Default app for magnet links and `.torrent` files (`handler`)

On start the app asks the system whether it is the default for both (off the UI thread). If not, and the user
has not said "don't ask again", a dialog asks once per run; "Yes" registers, "Not now" only closes it,
"Don't ask again" is saved (`ask_default_handler`). Preferences has the same status and a **Make default** button.
A development build, and on macOS an app outside a `.app` bundle (or one that is translocated), is never offered.

| System | Read | Write | Limits |
|---|---|---|---|
| macOS | `LSCopyDefaultHandlerForURLScheme("magnet")` and `LSCopyDefaultRoleHandlerForContentType` for the `.torrent` type | `LSSetDefaultHandlerForURLScheme`, `LSSetDefaultRoleHandlerForContentType` | `.torrent` has no system type, so the dynamic type for the extension is used. The calls are deprecated by Apple but still work; macOS may ask the user to confirm. |
| Linux | `xdg-mime query default` | writes `~/.local/share/applications/es.nico.putio-adder.desktop` (the packaged entry with an absolute, escaped `Exec`), then `xdg-mime default` | needs `xdg-mime`; otherwise the app says so and offers nothing |
| Windows | per-user registry via `reg.exe`, including the user's own choice (`UserChoice`) | the same ProgIDs and Capabilities the installer writes, plus the classic `magnet` and `.torrent` keys when the user has made no choice of their own | Windows protects the user's choice: if another app was picked, the app opens *Settings → Default apps* and says so |

Everything is built from the app's own path with strict quoting (`Exec=` escaping, registry commands built as
arguments, never a shell line); a path with a control character or a quote is refused.

**Run on:** the macOS read path (the real Launch Services, read-only), the pure logic everywhere. **Not run:** any
write on any system (it changes the user's default apps), and everything on Windows and Linux.
