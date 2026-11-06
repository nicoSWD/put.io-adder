# Direct dependencies and why

Every direct dependency is a decision. A new one needs an entry here, a license from the allow-list in
`deny.toml`, and a reason why 20 lines of our own code would not do.

| Crate | Used by | Why | Notes |
|---|---|---|---|
| `serde`, `serde_json` | api, core | JSON models and the settings file | de-facto standard |
| `thiserror` | all | error types without boilerplate | proc-macro, no runtime code |
| `zeroize` | api | wipe the token on drop | |
| `ureq` (rustls, no gzip) | api (`http` feature) | blocking HTTPS client, no async runtime | pulls rustls, `ring`, `webpki-roots`; **not** in `putio-core` |
| `semver` | core | compare versions correctly (`0.10 > 0.9`) | |
| `keyring-core` + one store per OS | platform | token in Keychain / Credential Manager / Secret Service | stores are separate crates so only the OS's own is compiled; Linux uses the pure-Rust `zbus` store (no libdbus) |
| `interprocess` | platform | local sockets (Unix) and named pipes (Windows) for single-instance forwarding | wrapping `unsafe` OS calls ourselves would be worse |
| `notify-rust` | platform (Windows, Linux) | desktop notifications | macOS uses `objc2-user-notifications` directly |
| `objc2`, `objc2-foundation`, `objc2-app-kit`, `objc2-user-notifications`, `block2` | platform (macOS) | Apple Events, Dock badge, notifications | same stack as DegenView; feature lists are minimal |
| `zip` (inflate only), `tar`, `flate2` | platform | unpack the update's zip (Windows) and tarball (Linux) in-process, with size and entry limits; macOS uses `ditto` | `zip` pulls `typed-path`, `nix` (Unix permissions), `crc32fast`; `tar` pulls `filetime`; all MIT / Apache-2.0 |
| `rfd` (`xdg-portal` only) | platform (`file_dialog`) | the native "choose files" dialog for the Upload button: `NSOpenPanel` on macOS, the common item dialog on Windows, the desktop portal on Linux | writing three native dialogs ourselves (Objective-C, COM, D-Bus) is far more than 20 lines; no GTK, no async runtime; adds `rfd` and `pollster` (and reuses the `objc2` and `windows-sys` crates already in the tree); macOS needs the main thread, which is where the UI runs |
| `proptest` | core (dev) | property tests for parsers and formatters | dev only |

Not used on purpose: `tokio`/async runtimes, `openssl`/`native-tls` (banned), `reqwest`, `regex`, `url`,
`directories`, `tempfile`, any crate fetched from git.

Counts: 214 packages in `Cargo.lock` across all six targets (most of the Linux share is `zbus`).
`cargo metadata` shows every license is covered by `deny.toml` and every source is crates.io.
