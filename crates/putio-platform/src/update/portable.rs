//! The portable strategy (Windows and Linux): replace the running executable in place.
//!
//! Neither OS lets a running program be overwritten in the usual way, but both let it be renamed:
//! the swap moves the running binary into the staging folder and puts the new one where it was, and
//! the old process keeps running from its renamed file until it quits.
//!
//! There is no shell helper to reopen the app: [`spawn_relaunch`] starts the new binary with
//! `--relaunch-after <pid>`, and the new process waits for the single-instance lock to be free
//! before it opens anything (see `main`), so two instances never run at once.
//!
//! The logic is compiled on every platform so its tests run everywhere; only the process calls are
//! per-OS.
#![cfg_attr(
    not(any(windows, target_os = "linux")),
    allow(dead_code, unused_imports)
)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use putio_core::update::UpdateAsset;

use super::{RELAUNCH_FLAG, archive, is_writable};

/// The running executable, when it can be replaced in place.
pub(super) fn install_target() -> Result<PathBuf, String> {
    if !cfg!(any(windows, target_os = "linux")) {
        return Err("Automatic updates are not available on this platform.".to_owned());
    }
    // A debug build is a development build, wherever cargo put it (`CARGO_TARGET_DIR` can point outside
    // the source tree), and must never replace itself with a release.
    if cfg!(debug_assertions) {
        return Err(DEV_BUILD.to_owned());
    }
    let exe = std::env::current_exe()
        .and_then(|exe| exe.canonicalize())
        .map_err(|error| error.to_string())?;
    target_for(&plain(&exe), &plain(&source_root()))
}

pub(super) const DEV_BUILD: &str = "A development build does not update itself.";

/// The workspace this binary was built from: two levels above this crate (`crates/putio-platform`).
/// (`CARGO_MANIFEST_DIR` is this crate's folder, not the workspace root; a `cargo run` binary lives in
/// `<root>/target/`, which is not below the crate's folder.) `canonicalize` gives a `\\?\C:\…` path on
/// Windows, so callers compare after [`plain`].
pub(super) fn source_root() -> PathBuf {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = crate_dir.ancestors().nth(2).unwrap_or(crate_dir);
    root.canonicalize().unwrap_or_else(|_| root.to_path_buf())
}

/// `path` without the `\\?\` prefix `canonicalize` adds on Windows (a `\\?\UNC\` path keeps it).
pub(super) fn plain(path: &Path) -> PathBuf {
    match path.to_str().and_then(|path| path.strip_prefix(r"\\?\")) {
        Some(rest) if !rest.starts_with(r"UNC\") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

/// `exe` itself, unless it sits in a folder an update cannot write to or is a development build
/// (`cargo run` under `source_root`), which must never replace itself with a release.
pub(super) fn target_for(exe: &Path, source_root: &Path) -> Result<PathBuf, String> {
    if exe.starts_with(source_root) {
        return Err(DEV_BUILD.to_owned());
    }
    let parent = exe.parent().ok_or("The app has no parent folder.")?;
    if !is_writable(parent) {
        return Err(format!(
            "{} is not writable. {}",
            parent.display(),
            if cfg!(windows) {
                "Move put.io adder to a folder you own, such as Documents."
            } else {
                "Install put.io adder in a folder you own, such as ~/.local/bin."
            }
        ));
    }
    Ok(exe.to_path_buf())
}

/// Unpacks the downloaded archive's one binary into `staging/new` and checks it is an executable for
/// this CPU: a zip holding `putio-adder.exe` on Windows, a tarball holding the binary elsewhere.
/// Two definitions rather than a `cfg!` branch: each calls functions with OS-specific meaning.
#[cfg(windows)]
pub(super) fn unpack(
    bytes: &[u8],
    _asset: &UpdateAsset,
    staging: &Path,
) -> Result<PathBuf, String> {
    archive::precheck(bytes)?;
    let exe = archive::extract_exe(bytes, &staging.join("new"))?;
    archive::check_exe(&exe)?;
    Ok(exe)
}

#[cfg(not(windows))]
pub(super) fn unpack(bytes: &[u8], asset: &UpdateAsset, staging: &Path) -> Result<PathBuf, String> {
    let stem = asset
        .name
        .strip_suffix(".tar.gz")
        .ok_or("The download is not a tarball.")?;
    let entry = format!("{stem}/{}", archive::BINARY_NAME);
    let binary = archive::extract_tar_gz_binary(bytes, &entry, &staging.join("new"))?;
    archive::check_elf(&binary)?;
    Ok(binary)
}

/// Environment the relaunched app gets on Windows (names compare case-insensitively): it is started
/// clean, so these are what it needs to find its data folders and for anything it starts later (the
/// browser) to work. Credentials never belong here: the app reads its token from the OS store again.
#[cfg_attr(not(windows), allow(dead_code))]
pub(super) const FORWARDED_ENV: &[&str] = &[
    "APPDATA",
    "LOCALAPPDATA",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "TEMP",
    "TMP",
    "SystemRoot",
    "windir",
    "SystemDrive",
    "ComSpec",
    "PATH",
    "PATHEXT",
    "ProgramData",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "USERNAME",
    "USERDOMAIN",
    "COMPUTERNAME",
    "OS",
    "PROCESSOR_ARCHITECTURE",
    "NUMBER_OF_PROCESSORS",
];

#[cfg_attr(not(windows), allow(dead_code))]
pub(super) fn forwarded_env(
    vars: impl Iterator<Item = (OsString, OsString)>,
    allowed: &[&str],
) -> Vec<(OsString, OsString)> {
    vars.filter(|(name, _)| {
        name.to_str()
            .is_some_and(|name| allowed.iter().any(|a| a.eq_ignore_ascii_case(name)))
    })
    .collect()
}

/// What environment the relaunched process gets.
#[cfg_attr(not(windows), allow(dead_code))]
pub(super) enum Environment {
    /// Only these variables (Windows): started clean, so nothing it was not meant to see is passed.
    Only(Vec<(OsString, OsString)>),
    /// Everything this process has (Linux). It must carry `DISPLAY`/`WAYLAND_DISPLAY`/
    /// `XDG_RUNTIME_DIR`/`DBUS_SESSION_BUS_ADDRESS` (the Secret Service) or the new process would
    /// start broken, and nothing is put on the command line.
    #[cfg_attr(windows, allow(dead_code))]
    Inherit,
}

/// The new process: the exe and `--relaunch-after <pid>`, with the given environment.
pub(super) fn relaunch_command(
    exe: &Path,
    pid: u32,
    environment: Environment,
) -> std::process::Command {
    let mut command = std::process::Command::new(exe);
    command.arg(RELAUNCH_FLAG).arg(pid.to_string());
    if let Environment::Only(variables) = environment {
        command.env_clear().envs(variables);
    }
    command
}

#[cfg(windows)]
pub(super) fn spawn_relaunch(exe: &Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;

    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    relaunch_command(
        exe,
        std::process::id(),
        Environment::Only(forwarded_env(std::env::vars_os(), FORWARDED_ENV)),
    )
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
    .spawn()
    .map(drop)
}

#[cfg(unix)]
pub(super) fn spawn_relaunch(exe: &Path) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;

    // Its own process group, so closing the terminal this one was started from does not take it
    // down with the old process.
    relaunch_command(exe, std::process::id(), Environment::Inherit)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(drop)
}
