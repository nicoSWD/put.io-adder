//! Finding VLC and starting it with a signed put.io URL.
//!
//! The URL comes from `GET /files/{id}/url`: it carries its own short-lived, IP-bound credentials,
//! so the account token never appears on the player's command line (the old app leaked it there).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::paths::Os;

#[derive(Debug, thiserror::Error)]
pub enum PlayError {
    #[error("VLC was not found")]
    NoPlayer,
    #[error("nothing to play")]
    NothingToPlay,
    #[error("refusing to pass a URL that is not a plain https URL to the player")]
    BadUrl,
    #[error("could not start the player: {0}")]
    Spawn(#[from] std::io::Error),
}

/// How to start VLC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Player {
    Binary(PathBuf),
    /// `flatpak run org.videolan.VLC`
    Flatpak,
}

/// Fixed locations to look in (no search of arbitrary directories).
fn candidate_paths(os: Os, env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    match os {
        Os::MacOs => vec![PathBuf::from("/Applications/VLC.app/Contents/MacOS/VLC")],
        Os::Windows => ["ProgramFiles", "ProgramFiles(x86)"]
            .iter()
            .filter_map(|name| env(name))
            .map(|base| {
                PathBuf::from(base)
                    .join("VideoLAN")
                    .join("VLC")
                    .join("vlc.exe")
            })
            .collect(),
        Os::Linux => ["/usr/bin/vlc", "/usr/local/bin/vlc", "/snap/bin/vlc"]
            .iter()
            .map(PathBuf::from)
            .collect(),
    }
}

/// Find VLC. `exists` is injected so the search is testable.
#[must_use]
pub fn find(
    os: Os,
    env: &dyn Fn(&str) -> Option<String>,
    exists: &dyn Fn(&Path) -> bool,
) -> Option<Player> {
    if let Some(path) = candidate_paths(os, env)
        .into_iter()
        .find(|path| path.is_absolute() && exists(path))
    {
        return Some(Player::Binary(path));
    }
    if os == Os::Linux
        && (exists(Path::new("/var/lib/flatpak/app/org.videolan.VLC"))
            || env("HOME").is_some_and(|home| {
                exists(&PathBuf::from(home).join(".local/share/flatpak/app/org.videolan.VLC"))
            }))
    {
        return Some(Player::Flatpak);
    }
    None
}

/// Only plain `https` URLs without anything an argument parser could mistake for an option.
fn url_is_safe(url: &str) -> bool {
    url.starts_with("https://") && url.len() <= 4096 && url.bytes().all(|b| b.is_ascii_graphic())
}

/// The command that plays `urls` (more than one makes a playlist). Never a shell.
pub fn command(player: &Player, urls: &[String]) -> Result<Command, PlayError> {
    if urls.is_empty() {
        return Err(PlayError::NothingToPlay);
    }
    if !urls.iter().all(|url| url_is_safe(url)) {
        return Err(PlayError::BadUrl);
    }
    let mut command = match player {
        Player::Binary(path) => Command::new(path),
        Player::Flatpak => {
            let mut command = Command::new("flatpak");
            command.args(["run", "org.videolan.VLC"]);
            command
        }
    };
    // Every URL starts with `https://`, so none can be read as an option.
    command.args(urls);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Ok(command)
}

pub trait PlayerLauncher: Send + Sync {
    fn play(&self, urls: &[String]) -> Result<(), PlayError>;
}

#[derive(Debug, Default)]
pub struct VlcLauncher;

impl PlayerLauncher for VlcLauncher {
    fn play(&self, urls: &[String]) -> Result<(), PlayError> {
        let player = find(Os::current(), &crate::paths::process_env, &|path| {
            path.exists()
        })
        .ok_or(PlayError::NoPlayer)?;
        command(&player, urls)?.spawn()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn finds_vlc_in_the_fixed_locations() {
        let mac = find(Os::MacOs, &no_env, &|p| {
            p == Path::new("/Applications/VLC.app/Contents/MacOS/VLC")
        });
        assert_eq!(
            mac,
            Some(Player::Binary(
                "/Applications/VLC.app/Contents/MacOS/VLC".into()
            ))
        );
        let linux = find(Os::Linux, &no_env, &|p| p == Path::new("/usr/bin/vlc"));
        assert_eq!(linux, Some(Player::Binary("/usr/bin/vlc".into())));
        assert_eq!(find(Os::Linux, &no_env, &|_| false), None);
    }

    #[test]
    fn windows_uses_program_files() {
        let env = |name: &str| (name == "ProgramFiles").then(|| "C:\\Program Files".to_owned());
        let expected = PathBuf::from("C:\\Program Files")
            .join("VideoLAN")
            .join("VLC")
            .join("vlc.exe");
        // On non-Windows hosts `C:\…` is not absolute, so only assert the candidate list itself.
        assert_eq!(candidate_paths(Os::Windows, &env), [expected]);
    }

    #[test]
    fn falls_back_to_the_flatpak() {
        let found = find(Os::Linux, &no_env, &|p| {
            p == Path::new("/var/lib/flatpak/app/org.videolan.VLC")
        });
        assert_eq!(found, Some(Player::Flatpak));
    }

    #[test]
    fn command_passes_urls_as_plain_arguments() {
        let player = Player::Binary("/usr/bin/vlc".into());
        let urls = vec![
            "https://example.invalid/a.mkv?token=abc".to_owned(),
            "https://example.invalid/b.mkv".to_owned(),
        ];
        let cmd = command(&player, &urls).unwrap();
        assert_eq!(cmd.get_program(), "/usr/bin/vlc");
        assert_eq!(
            cmd.get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            urls
        );
    }

    #[test]
    fn flatpak_command_shape() {
        let cmd = command(&Player::Flatpak, &["https://x.invalid/a".to_owned()]).unwrap();
        assert_eq!(cmd.get_program(), "flatpak");
        assert_eq!(
            cmd.get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            ["run", "org.videolan.VLC", "https://x.invalid/a"]
        );
    }

    #[test]
    fn unsafe_urls_are_refused() {
        let player = Player::Binary("/usr/bin/vlc".into());
        for bad in [
            "http://x.invalid/a",
            "--fullscreen",
            "file:///etc/passwd",
            "https://x.invalid/a b",
            "https://x.invalid/a\n--x",
            "-I",
            "",
        ] {
            assert!(
                matches!(command(&player, &[bad.to_owned()]), Err(PlayError::BadUrl)),
                "{bad:?}"
            );
        }
        // One bad URL poisons the whole list.
        assert!(
            command(
                &player,
                &["https://ok.invalid/a".to_owned(), "--x".to_owned()]
            )
            .is_err()
        );
        assert!(matches!(
            command(&player, &[]),
            Err(PlayError::NothingToPlay)
        ));
    }
}
