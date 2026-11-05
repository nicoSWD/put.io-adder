//! Operating-system adapters for put.io adder.
//!
//! Everything that touches the OS lives here, behind small traits so the rest of the app can be
//! tested without a keychain, a notification daemon or a window system:
//!
//! * [`secret`]: the token in the OS secret store (Keychain, Credential Manager, Secret Service)
//! * [`settings_store`] and [`paths`]: where preferences live, written atomically with private permissions
//! * [`opener`]: opens an allow-listed `https` URL in the browser
//! * [`player`]: finds VLC and starts it with a signed URL
//! * [`notify`] and [`badge`]: completion notifications and the dock badge
//! * [`ipc`]: single-instance forwarding of magnet links and `.torrent` paths between processes
//! * `macos_events` (macOS): receives magnet links and files from Launch Services
//!
//! This is the only crate allowed to contain `unsafe`, and only in the macOS modules, where every
//! block carries a `SAFETY` comment.

pub mod badge;
pub mod ipc;
pub mod notify;
pub mod opener;
pub mod paths;
pub mod player;
pub mod secret;
pub mod settings_store;

#[cfg(target_os = "macos")]
pub mod macos_events;
