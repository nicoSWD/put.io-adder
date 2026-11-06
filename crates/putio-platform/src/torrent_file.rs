//! Reading a `.torrent` file that arrived from the OS (argv, Finder, IPC) without trusting it.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use putio_api::torrent::{self, InvalidTorrent, MAX_TORRENT_BYTES};

/// Longest file name kept for the label sent to put.io.
const MAX_NAME_CHARS: usize = 255;

#[derive(Debug, thiserror::Error)]
pub enum TorrentFileError {
    #[error("not a regular file")]
    NotAFile,
    #[error("the file is larger than {} MiB", MAX_TORRENT_BYTES / 1024 / 1024)]
    TooLarge,
    #[error(transparent)]
    Invalid(#[from] InvalidTorrent),
    #[error("could not read the file: {0}")]
    Io(#[from] io::Error),
}

/// Why an uploaded `.torrent` was left where it is instead of being moved to the Trash.
#[derive(Debug, thiserror::Error)]
pub enum TrashError {
    /// A symlink, a directory or something else that is not the plain file that was uploaded.
    #[error("not a regular file")]
    NotAFile,
    #[error("not a .torrent file")]
    NotATorrent,
    /// The file is not what was uploaded any more: it was replaced or edited in the meantime.
    #[error("the file changed since it was uploaded")]
    Changed,
    #[error("could not read the file: {0}")]
    Io(#[from] io::Error),
}

/// A `.torrent` that passed every check.
#[derive(Debug)]
pub struct TorrentFile {
    /// File name without directories, control characters removed.
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Read `path` if it is a regular file of sensible size that looks like a torrent.
///
/// The path may point at a FIFO, a device (`/dev/zero`), a directory or a huge file: those are
/// refused from the metadata *before* the file is opened, the size is checked again on the open
/// handle, and the read itself is capped, so nothing can block or exhaust memory.
pub fn read_torrent(path: &Path) -> Result<TorrentFile, TorrentFileError> {
    let before = std::fs::metadata(path)?;
    if !before.is_file() {
        return Err(TorrentFileError::NotAFile);
    }
    if before.len() > MAX_TORRENT_BYTES as u64 {
        return Err(TorrentFileError::TooLarge);
    }

    let file = File::open(path)?;
    let opened = file.metadata()?;
    if !opened.is_file() {
        return Err(TorrentFileError::NotAFile);
    }
    if opened.len() > MAX_TORRENT_BYTES as u64 {
        return Err(TorrentFileError::TooLarge);
    }

    let mut bytes = Vec::new();
    // One byte more than allowed, so a file that grew after the checks is noticed.
    file.take(MAX_TORRENT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_TORRENT_BYTES {
        return Err(TorrentFileError::TooLarge);
    }
    torrent::validate(&bytes)?;

    let name: String = path
        .file_name()
        .map(|n| {
            n.to_string_lossy()
                .chars()
                .filter(|c| !c.is_control())
                .take(MAX_NAME_CHARS)
                .collect()
        })
        .unwrap_or_default();
    Ok(TorrentFile {
        name: if name.is_empty() {
            "upload.torrent".into()
        } else {
            name
        },
        bytes,
    })
}

/// Move the `.torrent` at `path` to the Trash, now that its contents (`uploaded`) reached put.io.
///
/// Only the very file that was uploaded is touched: it has to be a plain file (not a symlink) with the
/// `.torrent` extension, and its bytes have to be exactly the ones that were sent. Anything else is left
/// alone and reported. `trash` does the move (see [`move_to_trash`]); it is a parameter so tests do not
/// touch the real Trash.
pub fn trash_uploaded(
    path: &Path,
    uploaded: &[u8],
    trash: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<(), TrashError> {
    // `symlink_metadata` does not follow links: a link to a file is refused, never the file behind it.
    if !std::fs::symlink_metadata(path)?.is_file() {
        return Err(TrashError::NotAFile);
    }
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("torrent"))
    {
        return Err(TrashError::NotATorrent);
    }
    if !read_torrent(path).is_ok_and(|current| current.bytes == uploaded) {
        return Err(TrashError::Changed);
    }
    trash(path)?;
    Ok(())
}

/// Put `path` in the system's Trash (Recycle Bin on Windows), where the user can get it back.
pub fn move_to_trash(path: &Path) -> io::Result<()> {
    trash::delete(path).map_err(|error| io::Error::other(error.to_string()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::settings_store::test_dir::TempDir;

    const VALID: &[u8] = b"d8:announce3:url4:infod4:name1:aee";

    fn dir(label: &str) -> TempDir {
        let dir = TempDir::new(label);
        fs::create_dir_all(&dir.0).unwrap();
        dir
    }

    #[test]
    fn reads_a_valid_torrent_and_names_it() {
        let tmp = dir("torrent-ok");
        let path = tmp.0.join("My Show.torrent");
        fs::write(&path, VALID).unwrap();
        let file = read_torrent(&path).unwrap();
        assert_eq!(file.name, "My Show.torrent");
        assert_eq!(file.bytes, VALID);
    }

    #[test]
    fn refuses_directories_and_missing_files() {
        let tmp = dir("torrent-dir");
        assert!(matches!(
            read_torrent(&tmp.0),
            Err(TorrentFileError::NotAFile)
        ));
        assert!(matches!(
            read_torrent(&tmp.0.join("nope.torrent")),
            Err(TorrentFileError::Io(_))
        ));
    }

    #[test]
    fn refuses_oversized_files_without_reading_them() {
        let tmp = dir("torrent-big");
        let path = tmp.0.join("big.torrent");
        let file = File::create(&path).unwrap();
        file.set_len(MAX_TORRENT_BYTES as u64 + 1).unwrap(); // sparse: nothing is written
        assert!(matches!(
            read_torrent(&path),
            Err(TorrentFileError::TooLarge)
        ));
    }

    #[test]
    fn refuses_files_that_are_not_torrents() {
        let tmp = dir("torrent-bad");
        let path = tmp.0.join("a.torrent");
        fs::write(&path, b"<html>not a torrent</html>").unwrap();
        assert!(matches!(
            read_torrent(&path),
            Err(TorrentFileError::Invalid(InvalidTorrent::NotATorrent))
        ));
        fs::write(&path, b"").unwrap();
        assert!(matches!(
            read_torrent(&path),
            Err(TorrentFileError::Invalid(InvalidTorrent::Empty))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn refuses_devices_and_fifos_without_blocking() {
        let tmp = dir("torrent-special");
        let link = tmp.0.join("zero.torrent");
        std::os::unix::fs::symlink("/dev/zero", &link).unwrap();
        assert!(matches!(
            read_torrent(&link),
            Err(TorrentFileError::NotAFile)
        ));

        let fifo = tmp.0.join("pipe.torrent");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(status.success());
        // Would block forever on open() if the metadata check did not come first.
        let started = std::time::Instant::now();
        assert!(matches!(
            read_torrent(&fifo),
            Err(TorrentFileError::NotAFile)
        ));
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }

    #[test]
    fn control_characters_are_removed_from_the_name() {
        let tmp = dir("torrent-name");
        let path = tmp.0.join("a\u{7}b.torrent");
        if fs::write(&path, VALID).is_ok() {
            assert_eq!(read_torrent(&path).unwrap().name, "ab.torrent");
        }
    }

    /// Run `trash_uploaded` with a mover that only records what it was asked to move.
    fn trashed(path: &Path, uploaded: &[u8]) -> (Result<(), TrashError>, Vec<std::path::PathBuf>) {
        let moved = std::cell::RefCell::new(Vec::new());
        let result = trash_uploaded(path, uploaded, |p| {
            moved.borrow_mut().push(p.to_path_buf());
            Ok(())
        });
        (result, moved.into_inner())
    }

    #[test]
    fn the_file_that_was_uploaded_is_moved_to_the_trash() {
        let tmp = dir("trash-ok");
        for name in ["a.torrent", "B.TORRENT"] {
            let path = tmp.0.join(name);
            fs::write(&path, VALID).unwrap();
            let (result, moved) = trashed(&path, VALID);
            assert!(result.is_ok(), "{name}");
            assert_eq!(moved, [path]);
        }
    }

    #[test]
    fn a_file_that_changed_after_the_upload_is_left_alone() {
        let tmp = dir("trash-changed");
        let path = tmp.0.join("a.torrent");
        fs::write(&path, b"d8:announce3:url4:infod4:name1:bee").unwrap();
        let (result, moved) = trashed(&path, VALID);
        assert!(matches!(result, Err(TrashError::Changed)), "{result:?}");
        assert_eq!(moved, Vec::<std::path::PathBuf>::new());
        // Replaced by something that is not a torrent at all.
        fs::write(&path, b"my holiday photos").unwrap();
        let (result, moved) = trashed(&path, VALID);
        assert!(matches!(result, Err(TrashError::Changed)), "{result:?}");
        assert_eq!(moved, Vec::<std::path::PathBuf>::new());
    }

    #[test]
    fn only_plain_torrent_files_are_ever_moved() {
        let tmp = dir("trash-refused");
        // A directory, even one called x.torrent.
        let folder = tmp.0.join("x.torrent");
        fs::create_dir_all(&folder).unwrap();
        let (result, moved) = trashed(&folder, VALID);
        assert!(matches!(result, Err(TrashError::NotAFile)), "{result:?}");
        assert_eq!(moved, Vec::<std::path::PathBuf>::new());
        // Another extension.
        let other = tmp.0.join("a.txt");
        fs::write(&other, VALID).unwrap();
        let (result, moved) = trashed(&other, VALID);
        assert!(matches!(result, Err(TrashError::NotATorrent)), "{result:?}");
        assert_eq!(moved, Vec::<std::path::PathBuf>::new());
        // Gone already.
        let (result, moved) = trashed(&tmp.0.join("gone.torrent"), VALID);
        assert!(matches!(result, Err(TrashError::Io(_))), "{result:?}");
        assert_eq!(moved, Vec::<std::path::PathBuf>::new());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_refused_and_the_file_behind_it_is_not_touched() {
        let tmp = dir("trash-link");
        let real = tmp.0.join("real.torrent");
        fs::write(&real, VALID).unwrap();
        let link = tmp.0.join("link.torrent");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let (result, moved) = trashed(&link, VALID);
        assert!(matches!(result, Err(TrashError::NotAFile)), "{result:?}");
        assert_eq!(moved, Vec::<std::path::PathBuf>::new());
        assert!(real.exists());
    }

    #[test]
    fn a_failing_move_is_reported_and_the_file_stays() {
        let tmp = dir("trash-fails");
        let path = tmp.0.join("a.torrent");
        fs::write(&path, VALID).unwrap();
        let result = trash_uploaded(&path, VALID, |_| Err(io::Error::other("no trash here")));
        assert!(matches!(result, Err(TrashError::Io(_))), "{result:?}");
        assert!(path.exists());
    }
}
