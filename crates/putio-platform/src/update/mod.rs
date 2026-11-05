//! Replacing the running app with a newer release.
//!
//! Two steps, so nothing on disk changes until the user says so: [`prepare`] downloads the release
//! file for this computer and unpacks it into a hidden folder next to the app; [`apply`] swaps it in
//! when the user asks to restart, and [`relaunch`] makes sure the new version starts once this process
//! has gone. The previous version stays in the staging folder until the new one has started and
//! calls [`cleanup_stale`], so a build that never launches can be put back by hand.
//!
//! What it **does not** do: check a checksum, a release signature or a code signature. What it does
//! do: take the file only from GitHub over HTTPS, bound the download and the unpacking, require the
//! unpacked app to have exactly the expected layout and the right CPU, and refuse to update a folder
//! it cannot write to or a development build. See `docs/updates.md`.
//!
//! Two strategies share this module: macOS replaces the `.app` bundle (`bundle`), Windows and Linux
//! replace the one executable (`portable`). Everything lives next to the app, on the same volume, so
//! the swap is two atomic renames.

mod archive;
#[cfg(unix)]
mod bundle;
mod portable;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use putio_core::state::UpdateStage;
use putio_core::update::UpdateAsset;

/// The folder next to the app where an update is downloaded, unpacked and kept until the new
/// version has started.
const STAGING_NAME: &str = ".putio-adder-update";
const USER_AGENT: &str = concat!("putio-adder/", env!("CARGO_PKG_VERSION"));

/// Passed to the relaunched binary (Windows and Linux), followed by the pid of the process it replaces.
pub const RELAUNCH_FLAG: &str = "--relaunch-after";

/// A downloaded and unpacked update waiting for the user's restart.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Prepared {
    /// The `.app` bundle (macOS) or the executable (elsewhere) being replaced.
    target: PathBuf,
    staging: PathBuf,
    /// The unpacked replacement.
    new: PathBuf,
}

/// Which release file this computer installs for the release tagged `tag`, e.g.
/// `putio-adder-v0.2.0-macos-arm64.zip`. `None` on a platform with no in-place update.
#[must_use]
pub fn asset_name(tag: &str) -> Option<String> {
    asset_name_for(tag, std::env::consts::OS, std::env::consts::ARCH)
}

fn asset_name_for(tag: &str, os: &str, arch: &str) -> Option<String> {
    let suffix = match (os, arch) {
        ("macos", "aarch64") => "macos-arm64.zip",
        ("macos", "x86_64") => "macos-x64.zip",
        ("windows", "aarch64") => "windows-arm64.zip",
        ("windows", "x86_64") => "windows-x64.zip",
        ("linux", "aarch64") => "linux-aarch64.tar.gz",
        ("linux", "x86_64") => "linux-x86_64.tar.gz",
        _ => return None,
    };
    Some(format!("putio-adder-{tag}-{suffix}"))
}

/// The thing this process runs from, when it can be replaced in place; otherwise why not.
pub fn install_target() -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    return bundle::install_target();
    #[cfg(not(target_os = "macos"))]
    return portable::install_target();
}

/// Blocking: downloads and unpacks `asset` next to `target`. Nothing is replaced yet.
/// `progress` is told the stage and how far through the whole job it is (0 to 1).
pub fn prepare(
    target: &Path,
    asset: &UpdateAsset,
    progress: &dyn Fn(UpdateStage, f32),
) -> Result<Prepared, String> {
    let download = |url: &str, max: u64, report: &dyn Fn(u64)| {
        putio_api::download::get(url, max, USER_AGENT, report)
    };
    prepare_with(target, asset, progress, &download)
}

/// A download: the address, the most bytes accepted and a callback for the bytes received so far.
type Download<'a> = dyn Fn(&str, u64, &dyn Fn(u64)) -> Result<Vec<u8>, String> + 'a;

fn prepare_with(
    target: &Path,
    asset: &UpdateAsset,
    progress: &dyn Fn(UpdateStage, f32),
    download: &Download<'_>,
) -> Result<Prepared, String> {
    let parent = target.parent().ok_or("The app has no parent folder.")?;
    let staging = parent.join(STAGING_NAME);
    keep_only_copy(&staging, target)?;
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir(&staging)
        .map_err(|error| format!("Could not create {STAGING_NAME}: {error}"))?;
    let result = (|| {
        progress(UpdateStage::Downloading, DOWNLOAD_START);
        // One report per percent: the callback wakes the UI.
        let reported = std::cell::Cell::new(u64::MAX);
        let bytes = download(&asset.url, asset.size, &|received| {
            let percent = received.saturating_mul(100) / asset.size.max(1);
            if reported.replace(percent) != percent {
                progress(UpdateStage::Downloading, downloading(received, asset.size));
            }
        })?;
        // Not a checksum: only a guard against a cut-off download.
        if u64::try_from(bytes.len()) != Ok(asset.size) {
            return Err("The download is incomplete.".to_owned());
        }
        progress(UpdateStage::Unpacking, DOWNLOAD_END);
        let new = unpack(&bytes, asset, &staging)?;
        drop(bytes);
        Ok(Prepared {
            target: target.to_path_buf(),
            staging: staging.clone(),
            new,
        })
    })();
    result.inspect_err(|_| {
        let _ = std::fs::remove_dir_all(&staging);
    })
}

/// Where the download part of the bar starts and ends; unpacking covers the rest.
const DOWNLOAD_START: f32 = 0.02;
const DOWNLOAD_END: f32 = 0.92;

fn downloading(received: u64, total: u64) -> f32 {
    if total == 0 {
        return DOWNLOAD_START;
    }
    // Both are small enough for f64 to be exact for any plausible download.
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    let done = (received as f64 / total as f64).clamp(0.0, 1.0) as f32;
    DOWNLOAD_START + (DOWNLOAD_END - DOWNLOAD_START) * done
}

#[cfg(target_os = "macos")]
fn unpack(bytes: &[u8], _asset: &UpdateAsset, staging: &Path) -> Result<PathBuf, String> {
    bundle::unpack(bytes, staging)
}

#[cfg(not(target_os = "macos"))]
fn unpack(bytes: &[u8], asset: &UpdateAsset, staging: &Path) -> Result<PathBuf, String> {
    portable::unpack(bytes, asset, staging)
}

/// Swaps the prepared update in. Nothing is started: [`relaunch`] does that.
pub fn apply(prepared: &Prepared) -> Result<(), String> {
    let name = prepared
        .target
        .file_name()
        .ok_or("The app has no file name.")?;
    let old = prepared.staging.join("old").join(name);
    let old_parent = old.parent().ok_or("The staging folder is malformed.")?;
    std::fs::create_dir_all(old_parent).map_err(|error| error.to_string())?;
    swap_paths(&prepared.target, &prepared.new, &old, &|from, to| {
        std::fs::rename(from, to)
    })
}

/// Arranges for the new version to start once this process is gone. Call it when the swap is done.
pub fn relaunch(prepared: &Prepared) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let started = bundle::spawn_relaunch(&prepared.target);
    #[cfg(not(target_os = "macos"))]
    let started = portable::spawn_relaunch(&prepared.target);
    started.map_err(|error| {
        format!("Updated, but could not restart: {error}. Open put.io adder again.")
    })
}

/// Run once the app has started: removes the staging folder (and the previous version in it) that a
/// finished or abandoned update left next to the app.
pub fn cleanup_stale() {
    let Ok(exe) = std::env::current_exe().and_then(|exe| exe.canonicalize()) else {
        return;
    };
    #[cfg(target_os = "macos")]
    let target = bundle::bundle_for_executable(&exe);
    #[cfg(not(target_os = "macos"))]
    let target = Some(exe);
    if let Some(parent) = target.as_deref().and_then(Path::parent) {
        let _ = std::fs::remove_dir_all(parent.join(STAGING_NAME));
    }
}

/// The pid after `--relaunch-after`, when this process was started by an update (Windows and Linux).
pub fn predecessor_pid(args: impl Iterator<Item = OsString>) -> Option<u32> {
    let mut args = args.skip(1);
    while let Some(arg) = args.next() {
        if arg == RELAUNCH_FLAG {
            return args.next()?.to_str()?.parse().ok();
        }
    }
    None
}

/// Whether a folder can be created in `dir`. `create_dir` fails if the name exists, and the probe is
/// removed only when this call created it, so a folder that was already there is never touched.
fn is_writable(dir: &Path) -> bool {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let probe = dir.join(format!(
        ".putio-adder-write-test-{}-{nanos}",
        std::process::id()
    ));
    if std::fs::create_dir(&probe).is_err() {
        return false;
    }
    let _ = std::fs::remove_dir(&probe);
    true
}

/// Moves the installation at `current` to `old`, then `new` into its place, with `rename` doing the
/// moves. If the second move fails the old one is put back; if that fails too, nothing else will
/// restore the app, so the error says where the old copy is and what to do. Both moves are within
/// one folder tree on one volume, so each is atomic, but there is a moment between them with
/// nothing at `current` (a crash there leaves the old copy in the staging folder).
fn swap_paths(
    current: &Path,
    new: &Path,
    old: &Path,
    rename: &dyn Fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), String> {
    rename(current, old)
        .map_err(|error| format!("Could not move the current app aside: {error}"))?;
    let Err(error) = rename(new, current) else {
        return Ok(());
    };
    match rename(old, current) {
        Ok(()) => Err(format!("Could not install the new app: {error}")),
        Err(restore) => Err(format!(
            "Could not install the new app: {error}. Putting the old one back failed too \
             ({restore}). It is at {}: move it to {} to get it back.",
            old.display(),
            current.display()
        )),
    }
}

/// Refuses to clear `staging` when it holds the only copy of the app: a swap that failed and could
/// not put the old one back leaves `staging/old` behind with nothing at `target`, and preparing the
/// next attempt starts by deleting the staging folder.
fn keep_only_copy(staging: &Path, target: &Path) -> Result<(), String> {
    let old = staging.join("old");
    if std::fs::symlink_metadata(target).is_err() && std::fs::symlink_metadata(&old).is_ok() {
        return Err(format!(
            "put.io adder is missing from {} and the previous version is in {}. Move it back, \
             then try the update again.",
            target.display(),
            old.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
