//! Things the OS hands us to add: magnet links and `.torrent` files.
//!
//! Everything here comes from outside the app (browser clicks, Finder, command lines, a second
//! process over IPC) and is treated as hostile until it has been through this module.

use std::path::{Path, PathBuf};

use crate::magnet::{Magnet, MagnetError};

/// A validated request to add something to put.io.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intake {
    Magnet(Magnet),
    /// Path of a `.torrent` file. Only the name is checked here; the file is read, size-checked and
    /// sanity-checked by the shell (`putio_api::torrent::validate`) right before upload.
    TorrentFile(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IntakeError {
    #[error(transparent)]
    Magnet(#[from] MagnetError),
    #[error("not a .torrent file")]
    NotTorrent,
    #[error("file URL is not a local absolute path")]
    BadFileUrl,
}

impl Intake {
    /// Interpret one command-line argument, Apple Event string or IPC payload.
    ///
    /// Accepts `magnet:` links, absolute `file://` URLs and plain paths that end in `.torrent`.
    pub fn parse(raw: &str) -> Result<Self, IntakeError> {
        let raw = raw.trim();
        if raw.len() >= "magnet:".len()
            && raw.is_char_boundary(7)
            && raw[..7].eq_ignore_ascii_case("magnet:")
        {
            return Ok(Self::Magnet(Magnet::parse(raw)?));
        }
        let path = if raw.len() >= 7
            && raw.is_char_boundary(7)
            && raw[..7].eq_ignore_ascii_case("file://")
        {
            file_url_to_path(raw)?
        } else if raw.contains("://") {
            // Some other URL (http, ftp, …): never treat it as a file path.
            return Err(IntakeError::NotTorrent);
        } else {
            PathBuf::from(raw)
        };
        if raw.chars().any(|c| c == '\0') || !has_torrent_extension(&path) {
            return Err(IntakeError::NotTorrent);
        }
        Ok(Self::TorrentFile(path))
    }

    /// Name to show while working on it.
    #[must_use]
    pub fn display_name(&self) -> String {
        match self {
            Self::Magnet(magnet) => magnet.display_name(),
            Self::TorrentFile(path) => path
                .file_name()
                .map_or_else(|| "torrent".into(), |n| n.to_string_lossy().into_owned()),
        }
    }
}

fn has_torrent_extension(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("torrent"))
}

/// `file:///Users/me/a%20b.torrent` → `/Users/me/a b.torrent`. Only local absolute paths
/// (`file:///…` or `file://localhost/…`) are accepted; query strings and fragments are rejected.
fn file_url_to_path(url: &str) -> Result<PathBuf, IntakeError> {
    let rest = url.get(7..).ok_or(IntakeError::BadFileUrl)?;
    let path = match rest.strip_prefix("localhost") {
        Some(after) => after,
        None => rest,
    };
    // Windows drive paths arrive as `file:///C:/Users/…`.
    let path = path.strip_prefix('/').ok_or(IntakeError::BadFileUrl)?;
    if path.contains(['?', '#']) {
        return Err(IntakeError::BadFileUrl);
    }
    let decoded = percent_decode_path(path).ok_or(IntakeError::BadFileUrl)?;
    let windows_drive = decoded.as_bytes().get(1) == Some(&b':')
        && decoded
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic);
    let full = if windows_drive {
        decoded
    } else {
        format!("/{decoded}")
    };
    let path = PathBuf::from(full);
    if path.is_absolute() || windows_drive {
        Ok(path)
    } else {
        Err(IntakeError::BadFileUrl)
    }
}

fn percent_decode_path(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        if byte == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(byte);
            i += 1;
        }
    }
    let text = String::from_utf8(out).ok()?;
    (!text.contains('\0')).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn magnets_are_validated_through_the_magnet_parser() {
        let ok = Intake::parse(&format!("magnet:?xt=urn:btih:{HASH}&dn=Sintel")).unwrap();
        assert_eq!(ok.display_name(), "Sintel");
        assert!(matches!(
            Intake::parse("magnet:?dn=x"),
            Err(IntakeError::Magnet(MagnetError::NoInfoHash))
        ));
    }

    #[test]
    fn plain_paths_must_end_in_torrent() {
        assert_eq!(
            Intake::parse("/tmp/a.torrent").unwrap(),
            Intake::TorrentFile("/tmp/a.torrent".into())
        );
        assert_eq!(
            Intake::parse("/tmp/A.TORRENT").unwrap().display_name(),
            "A.TORRENT"
        );
        for bad in [
            "/tmp/a.txt",
            "/tmp/torrent",
            "/tmp/a.torrent.exe",
            "",
            "hello",
            "https://x/a.torrent",
        ] {
            assert_eq!(
                Intake::parse(bad).unwrap_err(),
                IntakeError::NotTorrent,
                "{bad}"
            );
        }
    }

    #[test]
    fn file_urls_are_decoded_to_local_paths() {
        assert_eq!(
            Intake::parse("file:///Users/me/My%20Show.torrent").unwrap(),
            Intake::TorrentFile("/Users/me/My Show.torrent".into())
        );
        assert_eq!(
            Intake::parse("file://localhost/tmp/a.torrent").unwrap(),
            Intake::TorrentFile("/tmp/a.torrent".into())
        );
        assert_eq!(
            Intake::parse("FILE:///tmp/ü%C3%BC.torrent").unwrap(),
            Intake::TorrentFile("/tmp/üü.torrent".into())
        );
    }

    #[test]
    fn hostile_file_urls_are_rejected() {
        for bad in [
            "file://evil.example/share/a.torrent",
            "file:///tmp/a.torrent?x=1",
            "file:///tmp/a.torrent#frag",
            "file:///tmp/%zz.torrent",
            "file:///tmp/%00.torrent",
            "file:///tmp/%ff.torrent",
            "file://",
            "file:",
        ] {
            assert!(Intake::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn nul_bytes_are_rejected() {
        assert!(Intake::parse("/tmp/a\0.torrent").is_err());
    }

    #[test]
    fn multibyte_input_never_panics() {
        for input in [
            "日本語日本語",
            "mägnet:?x",
            "fïle:///x",
            "ééééééé",
            "magnét:",
        ] {
            let _ = Intake::parse(input);
        }
    }

    mod props {
        use proptest::prelude::*;

        use super::super::*;

        proptest! {
            #[test]
            fn parse_never_panics(input in ".{0,200}") {
                let _ = Intake::parse(&input);
            }
        }
    }
}
