//! Unpacking and inspecting a downloaded update, bounded so a hostile archive can't fill the disk or
//! plant extra launchable code: the zip's directory is checked before extraction, the extraction
//! itself is size- and time-limited, and what comes out must match the app's known layout.
//!
//! All of this is structural. It cannot prove the files are genuine: nothing here (or anywhere in
//! the updater) checks a checksum or a signature.

// Compiled on every OS so the tests run everywhere; each OS uses only its own part.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// A real app unpacks to about 30 MB.
pub(super) const MAX_UNPACKED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ENTRIES: usize = 2_000;
const EXTRACT_TIMEOUT: Duration = Duration::from_secs(120);
const POLL: Duration = Duration::from_millis(100);

pub(super) const BUNDLE_NAME: &str = "put.io adder.app";
const BUNDLE_ID: &str = "es.nico.putio-adder";
const MAIN_EXECUTABLE: &str = "Contents/MacOS/putio-adder";
/// The one file a Windows update replaces, at the root of the zip.
pub(super) const EXE_NAME: &str = "putio-adder.exe";
/// The one file a Linux update replaces, inside the tarball's top folder.
pub(super) const BINARY_NAME: &str = "putio-adder";

const NOT_THIS_APP: &str = "The download is not a put.io adder app.";

// ---- zip directory ------------------------------------------------------------------------------

fn le16(bytes: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?).into())
}

fn le32(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?).into())
}

/// Reads the zip's central directory (no decompression) and refuses archives that declare too many
/// entries, too much data, ZIP64 sizes, or an entry name that is absolute or climbs out.
pub(super) fn precheck(zip: &[u8]) -> Result<(), String> {
    let invalid = || "The download is not a valid zip.".to_owned();
    let last = zip.len().checked_sub(22).ok_or_else(invalid)?;
    let first = last.saturating_sub(usize::from(u16::MAX));
    let eocd = (first..=last)
        .rev()
        .find(|&at| zip.get(at..at + 4) == Some(&[0x50, 0x4b, 0x05, 0x06]))
        .ok_or_else(invalid)?;
    let count = le16(zip, eocd + 10).ok_or_else(invalid)?;
    let directory_size = le32(zip, eocd + 12).ok_or_else(invalid)?;
    let directory_at = le32(zip, eocd + 16).ok_or_else(invalid)?;
    if count == 0xFFFF || directory_size == 0xFFFF_FFFF || directory_at == 0xFFFF_FFFF {
        return Err("The download uses an unsupported zip format.".to_owned());
    }
    if count == 0 || count > MAX_ENTRIES {
        return Err("The download has an unexpected number of files.".to_owned());
    }
    let mut at = usize::try_from(directory_at).map_err(|_| invalid())?;
    let mut total: u64 = 0;
    for _ in 0..count {
        if zip.get(at..at + 4) != Some(&[0x50, 0x4b, 0x01, 0x02]) {
            return Err(invalid());
        }
        let size = le32(zip, at + 24).ok_or_else(invalid)?;
        let (name_len, extra_len, comment_len) = (
            le16(zip, at + 28).ok_or_else(invalid)?,
            le16(zip, at + 30).ok_or_else(invalid)?,
            le16(zip, at + 32).ok_or_else(invalid)?,
        );
        let name = zip.get(at + 46..at + 46 + name_len).ok_or_else(invalid)?;
        if !is_safe_entry_name(name) {
            return Err("The download contains an unsafe file path.".to_owned());
        }
        total = total
            .checked_add(size)
            .filter(|total| *total <= MAX_UNPACKED_BYTES && size != 0xFFFF_FFFF)
            .ok_or("The download unpacks to more data than an update should.")?;
        at += 46 + name_len + extra_len + comment_len;
    }
    Ok(())
}

fn is_safe_entry_name(name: &[u8]) -> bool {
    let Ok(name) = std::str::from_utf8(name) else {
        return false;
    };
    !name.is_empty()
        && !name.starts_with('/')
        && !name.contains(['\\', '\0'])
        && name.split('/').all(|part| part != "..")
}

// ---- macOS: bounded extraction with ditto -------------------------------------------------------

/// Unpacks `zip` into `dest` with `ditto`, killing it if `dest` grows past `MAX_UNPACKED_BYTES`
/// (declared sizes can lie) or it runs past the timeout.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) fn extract_with_ditto(zip: &Path, dest: &Path) -> Result<(), String> {
    let mut command = Command::new("/usr/bin/ditto");
    command.arg("-x").arg("-k").arg(zip).arg(dest);
    run_bounded(&mut command, dest, MAX_UNPACKED_BYTES, EXTRACT_TIMEOUT)
        .map_err(|error| format!("Could not unpack the download: {error}"))
}

fn run_bounded(
    command: &mut Command,
    watch: &Path,
    max_bytes: u64,
    timeout: Duration,
) -> Result<(), String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    let started = Instant::now();
    let fail = |child: &mut std::process::Child, message: &str| {
        let _ = child.kill();
        let _ = child.wait();
        Err(message.to_owned())
    };
    loop {
        match child.try_wait().map_err(|error| error.to_string())? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => return Err(format!("unpacking failed ({status})")),
            None => {}
        }
        match directory_bytes(watch) {
            Ok(bytes) if bytes <= max_bytes => {}
            Ok(_) => return fail(&mut child, "it unpacks to more data than an update should"),
            // A size that cannot be measured cannot be bounded: stop rather than let it grow.
            Err(error) => return fail(&mut child, &format!("could not measure it ({error})")),
        }
        if started.elapsed() > timeout {
            return fail(&mut child, "unpacking took too long");
        }
        std::thread::sleep(POLL);
    }
}

/// Total size of the regular files under `dir` (symlinks are not followed). A folder that does not
/// exist yet counts as empty; any other read error is reported, so a tree that cannot be fully
/// measured is never taken to be small.
fn directory_bytes(dir: &Path) -> std::io::Result<u64> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let mut total = 0_u64;
    for entry in entries {
        let entry = entry?;
        let meta = match entry.metadata() {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if meta.is_dir() {
            total = total.saturating_add(directory_bytes(&entry.path())?);
        } else if meta.is_file() {
            total = total.saturating_add(meta.len());
        }
    }
    Ok(total)
}

// ---- Windows: the one executable inside a zip ---------------------------------------------------

/// Inflates `EXE_NAME` from the zip in `bytes` into `dest_dir`, refusing more than
/// `MAX_UNPACKED_BYTES`. The output name is fixed, so nothing in the archive picks where a file
/// lands, and every other entry is ignored. `precheck` has already bounded the directory itself.
pub(super) fn extract_exe(bytes: &[u8], dest_dir: &Path) -> Result<PathBuf, String> {
    use std::io::Read;

    let failed = |error: &dyn std::fmt::Display| format!("Could not unpack the download: {error}");
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|error| failed(&error))?;
    // Two entries of one name would leave it to the reader which one is "the" executable.
    if archive.has_duplicate_names() {
        return Err("The download contains the same file twice.".to_owned());
    }
    let mut entry = archive
        .by_name(EXE_NAME)
        .map_err(|_| NOT_THIS_APP.to_owned())?;
    if !entry.is_file() || entry.size() > MAX_UNPACKED_BYTES {
        return Err(NOT_THIS_APP.to_owned());
    }
    std::fs::create_dir_all(dest_dir).map_err(|error| failed(&error))?;
    let dest = dest_dir.join(EXE_NAME);
    let mut out = std::fs::File::create(&dest).map_err(|error| failed(&error))?;
    // Declared sizes can lie: count what actually comes out.
    let copied = std::io::copy(&mut (&mut entry).take(MAX_UNPACKED_BYTES + 1), &mut out)
        .map_err(|error| failed(&error))?;
    if copied > MAX_UNPACKED_BYTES {
        return Err("The download unpacks to more data than an update should.".to_owned());
    }
    Ok(dest)
}

/// The PE machine type of the running build, which an update for another CPU must not replace.
const fn expected_pe_machine() -> u16 {
    if cfg!(target_arch = "aarch64") {
        0xAA64
    } else {
        0x8664
    }
}

fn pe_machine(exe: &Path) -> Result<u16, String> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(exe).map_err(|_| NOT_THIS_APP.to_owned())?;
    let mut dos = [0u8; 64];
    file.read_exact(&mut dos)
        .map_err(|_| NOT_THIS_APP.to_owned())?;
    if dos.get(..2) != Some(b"MZ") {
        return Err(NOT_THIS_APP.to_owned());
    }
    let pe_at = u32::from_le_bytes([dos[0x3C], dos[0x3D], dos[0x3E], dos[0x3F]]);
    let mut pe = [0u8; 6];
    file.seek(SeekFrom::Start(pe_at.into()))
        .and_then(|_| file.read_exact(&mut pe))
        .map_err(|_| NOT_THIS_APP.to_owned())?;
    if pe.get(..4) != Some(b"PE\0\0") {
        return Err(NOT_THIS_APP.to_owned());
    }
    Ok(u16::from_le_bytes([pe[4], pe[5]]))
}

/// Structural check of an unpacked Windows executable: a PE image for the running CPU.
pub(super) fn check_exe(exe: &Path) -> Result<(), String> {
    if pe_machine(exe)? != expected_pe_machine() {
        return Err("The download is for a different kind of processor.".to_owned());
    }
    Ok(())
}

// ---- Linux: the one binary inside a tarball -----------------------------------------------------

/// Inflates the entry `entry_path` (the binary inside the tarball's top folder) from the gzipped
/// tarball in `bytes` into `dest_dir` (mode 0755), refusing more than `MAX_UNPACKED_BYTES` of
/// decompressed data in total, more than `MAX_ENTRIES` entries, an unsafe entry name anywhere, and
/// a binary that is not a regular file or appears twice. The output name is fixed.
pub(super) fn extract_tar_gz_binary(
    bytes: &[u8],
    entry_path: &str,
    dest_dir: &Path,
) -> Result<PathBuf, String> {
    extract_tar_gz_binary_within(bytes, entry_path, dest_dir, MAX_UNPACKED_BYTES)
}

pub(super) fn extract_tar_gz_binary_within(
    bytes: &[u8],
    entry_path: &str,
    dest_dir: &Path,
    max_bytes: u64,
) -> Result<PathBuf, String> {
    use std::io::Read;

    let failed = |error: &dyn std::fmt::Display| format!("Could not unpack the download: {error}");
    if bytes.get(..2) != Some(&[0x1f, 0x8b]) {
        return Err("The download is not a valid archive.".to_owned());
    }
    // Declared sizes can lie, and so can compression ratios: bound what actually comes out.
    let stream = flate2::read::GzDecoder::new(bytes).take(max_bytes);
    let mut archive = tar::Archive::new(stream);
    let mut written = None;
    for (index, entry) in archive.entries().map_err(|e| failed(&e))?.enumerate() {
        if index >= MAX_ENTRIES {
            return Err("The download has an unexpected number of files.".to_owned());
        }
        let mut entry = entry.map_err(|e| failed(&e))?;
        let path = entry.path().map_err(|e| failed(&e))?;
        let name = path
            .to_str()
            .ok_or("The download contains an unsafe file path.")?;
        if !is_safe_entry_name(name.as_bytes()) {
            return Err("The download contains an unsafe file path.".to_owned());
        }
        if name.trim_start_matches("./") != entry_path {
            continue;
        }
        if written.is_some() || !entry.header().entry_type().is_file() {
            return Err(NOT_THIS_APP.to_owned());
        }
        std::fs::create_dir_all(dest_dir).map_err(|e| failed(&e))?;
        let dest = dest_dir.join(BINARY_NAME);
        let mut out = std::fs::File::create(&dest).map_err(|e| failed(&e))?;
        std::io::copy(&mut (&mut entry).take(max_bytes), &mut out).map_err(|e| failed(&e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| failed(&e))?;
        }
        written = Some(dest);
    }
    written.ok_or_else(|| NOT_THIS_APP.to_owned())
}

/// The ELF `e_machine` of the running build, which an update for another CPU must not replace.
const fn expected_elf_machine() -> u16 {
    if cfg!(target_arch = "aarch64") {
        0xB7
    } else {
        0x3E
    }
}

fn elf_machine(path: &Path) -> Result<u16, String> {
    use std::io::Read;

    let mut header = [0u8; 20];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|_| NOT_THIS_APP.to_owned())?;
    let kind = u16::from_le_bytes([header[16], header[17]]);
    // ELF magic, ELFCLASS64, ELFDATA2LSB; ET_EXEC or ET_DYN.
    if header.get(..4) != Some(b"\x7fELF")
        || header[4] != 2
        || header[5] != 1
        || !matches!(kind, 2 | 3)
    {
        return Err(NOT_THIS_APP.to_owned());
    }
    Ok(u16::from_le_bytes([header[18], header[19]]))
}

/// Structural check of an unpacked Linux binary: a 64-bit little-endian ELF executable (or PIE) for
/// the running CPU.
pub(super) fn check_elf(path: &Path) -> Result<(), String> {
    if elf_machine(path)? != expected_elf_machine() {
        return Err("The download is for a different kind of processor.".to_owned());
    }
    Ok(())
}

// ---- macOS: the bundle layout -------------------------------------------------------------------

/// The unpacked bundle must be exactly put.io adder: its bundle id and executable, no other
/// launchable file, and no link that leaves the bundle.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub(super) fn check_bundle(app: &Path) -> Result<(), String> {
    let declares_id = std::fs::read_to_string(app.join("Contents/Info.plist"))
        .is_ok_and(|plist| plist.contains(&format!("<string>{BUNDLE_ID}</string>")));
    let main_executable = app.join(MAIN_EXECUTABLE);
    if !declares_id || !main_executable.is_file() {
        return Err(NOT_THIS_APP.to_owned());
    }
    // If the bundle's layout changes on purpose, update the constants above.
    inspect(app, app, &[main_executable])
        .map_err(|problem| format!("The download is not a put.io adder app ({problem})."))
}

fn inspect(root: &Path, dir: &Path, expected: &[PathBuf]) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|error| error.to_string())?;
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        let meta = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        if meta.file_type().is_symlink() {
            let target = std::fs::read_link(&path).map_err(|error| error.to_string())?;
            let escapes = target.is_absolute()
                || target
                    .components()
                    .any(|part| matches!(part, std::path::Component::ParentDir));
            if escapes {
                return Err(format!("{relative} links outside the app"));
            }
        } else if meta.is_dir() {
            inspect(root, &path, expected)?;
        } else if is_executable(&meta) && !expected.contains(&path) {
            return Err(format!("unexpected executable {relative}"));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn is_executable(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_: &std::fs::Metadata) -> bool {
    false
}
