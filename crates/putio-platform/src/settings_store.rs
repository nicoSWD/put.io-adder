//! Reading and atomically writing the settings file.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// A settings file is a few hundred bytes; anything larger is not ours.
const MAX_BYTES: u64 = 64 * 1024;
const FILE_NAME: &str = "settings.json";

#[derive(Debug, Clone)]
pub struct SettingsFile {
    dir: PathBuf,
}

impl SettingsFile {
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    #[must_use]
    pub fn path(&self) -> PathBuf {
        self.dir.join(FILE_NAME)
    }

    /// `Ok(None)` when there is no file yet.
    pub fn load(&self) -> io::Result<Option<String>> {
        let file = match File::open(self.path()) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut text = String::new();
        // Read one byte past the limit to notice an oversized file.
        file.take(MAX_BYTES + 1).read_to_string(&mut text)?;
        if text.len() as u64 > MAX_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "settings file is too large",
            ));
        }
        Ok(Some(text))
    }

    /// Write via a temporary file in the same directory, flush it to disk, then rename over the
    /// real file, so a crash never leaves a half-written settings file. Permissions are private
    /// (`0700` directory, `0600` file) on Unix.
    pub fn save(&self, text: &str) -> io::Result<()> {
        create_private_dir(&self.dir)?;
        let temp = self
            .dir
            .join(format!("{FILE_NAME}.{}.tmp", std::process::id()));
        let result =
            write_private(&temp, text.as_bytes()).and_then(|()| fs::rename(&temp, self.path()));
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
}

fn create_private_dir(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)
}

fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
pub(crate) mod test_dir {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// A unique temporary directory removed on drop.
    pub(crate) struct TempDir(pub(crate) PathBuf);

    impl TempDir {
        pub(crate) fn new(label: &str) -> Self {
            let n = COUNTER.fetch_add(1, Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!(
                "putio-adder-test-{label}-{}-{n}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_dir::TempDir;
    use super::*;

    #[test]
    fn missing_file_is_none() {
        let tmp = TempDir::new("missing");
        assert!(SettingsFile::new(tmp.0.clone()).load().unwrap().is_none());
    }

    #[test]
    fn save_then_load_round_trips_and_replaces() {
        let tmp = TempDir::new("roundtrip");
        let file = SettingsFile::new(tmp.0.join("nested").join("dir"));
        file.save("{\"a\":1}").unwrap();
        assert_eq!(file.load().unwrap().as_deref(), Some("{\"a\":1}"));
        file.save("{\"a\":2}").unwrap();
        assert_eq!(file.load().unwrap().as_deref(), Some("{\"a\":2}"));
    }

    #[test]
    fn no_temporary_files_are_left_behind() {
        let tmp = TempDir::new("notemp");
        let file = SettingsFile::new(tmp.0.clone());
        file.save("x").unwrap();
        let names: Vec<_> = fs::read_dir(&tmp.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["settings.json"]);
    }

    #[test]
    fn oversized_files_are_rejected() {
        let tmp = TempDir::new("big");
        fs::create_dir_all(&tmp.0).unwrap();
        fs::write(
            tmp.0.join("settings.json"),
            vec![b'x'; usize::try_from(MAX_BYTES).unwrap() + 1],
        )
        .unwrap();
        let error = SettingsFile::new(tmp.0.clone()).load().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn non_utf8_content_is_an_error_not_a_panic() {
        let tmp = TempDir::new("binary");
        fs::create_dir_all(&tmp.0).unwrap();
        fs::write(tmp.0.join("settings.json"), [0xff, 0xfe, 0x00]).unwrap();
        assert!(SettingsFile::new(tmp.0.clone()).load().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn files_and_directories_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new("perms");
        let file = SettingsFile::new(tmp.0.join("cfg"));
        file.save("x").unwrap();
        let dir_mode = fs::metadata(tmp.0.join("cfg"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let file_mode = fs::metadata(file.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
        assert_eq!(file_mode, 0o600);
    }
}
