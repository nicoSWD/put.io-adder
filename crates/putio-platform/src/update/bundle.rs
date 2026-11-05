//! The macOS strategy: replace the running `.app` bundle.
//!
//! Compiled on the other Unixes too so that its path logic is tested everywhere; only macOS uses it.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::archive;
use super::is_writable;

/// How long the relaunch helper waits for this process to exit before giving up (0.2 s per tick).
const RELAUNCH_WAIT_TICKS: u32 = 300;

/// The `.app` bundle this process runs from, when it can be replaced in place.
pub(super) fn install_target() -> Result<PathBuf, String> {
    if !cfg!(target_os = "macos") {
        return Err("Automatic updates are not available on this platform.".to_owned());
    }
    let exe = std::env::current_exe()
        .and_then(|exe| exe.canonicalize())
        .map_err(|error| error.to_string())?;
    let app = bundle_for_executable(&exe)
        .ok_or_else(|| "put.io adder is not running from an app bundle.".to_owned())?;
    if app
        .components()
        .any(|part| part.as_os_str() == "AppTranslocation")
    {
        return Err("Move put.io adder to the Applications folder first.".to_owned());
    }
    let parent = app.parent().ok_or("The app has no parent folder.")?;
    if !is_writable(parent) {
        return Err(not_writable_message(&app, parent));
    }
    Ok(app)
}

fn not_writable_message(app: &Path, parent: &Path) -> String {
    // The app is on a read-only volume (a disk image) or in a folder this user cannot write to.
    if parent.starts_with("/Volumes") {
        "Drag put.io adder from the disk image to the Applications folder first.".to_owned()
    } else {
        format!(
            "{} is not writable. Move put.io adder to a folder you own, such as Applications.",
            app.parent().unwrap_or(app).display()
        )
    }
}

/// `/…/put.io adder.app` for `/…/put.io adder.app/Contents/MacOS/putio-adder`.
pub(super) fn bundle_for_executable(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    let is_bundle = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && app.extension()? == "app";
    is_bundle.then(|| app.to_path_buf())
}

/// Unpacks the downloaded zip into `staging/new` and checks the bundle it holds.
pub(super) fn unpack(bytes: &[u8], staging: &Path) -> Result<PathBuf, String> {
    archive::precheck(bytes)?;
    let zip = staging.join("update.zip");
    std::fs::write(&zip, bytes).map_err(|error| format!("Could not save the download: {error}"))?;
    let extracted = staging.join("new");
    archive::extract_with_ditto(&zip, &extracted)?;
    let _ = std::fs::remove_file(&zip);
    let new_app = extracted.join(archive::BUNDLE_NAME);
    archive::check_bundle(&new_app)?;
    Ok(new_app)
}

/// What the helper needs itself (it runs `sh`, `kill`, `sleep`, `open`).
const HELPER_ENV: &[&str] = &["HOME", "PATH", "USER", "TMPDIR"];
/// What the reopened app is given: `open` otherwise starts it clean. Only names that are safe to show
/// in a process listing belong here (the helper's arguments are visible to every user on the Mac).
const FORWARDED_ENV: &[&str] = &["HOME"];

fn pick_env(
    vars: impl Iterator<Item = (OsString, OsString)>,
    allowed: &[&str],
) -> Vec<(OsString, OsString)> {
    vars.filter(|(name, _)| name.to_str().is_some_and(|name| allowed.contains(&name)))
        .collect()
}

/// The detached helper: waits for `pid` to exit (at most `ticks` × 0.2 s), then runs `launch`, but
/// only when `pid` is really gone: a process that outlives the wait (a hung shutdown) is left alone
/// rather than given a second instance.
fn helper_command(ticks: u32, pid: u32, launch: &[OsString]) -> std::process::Command {
    let script = format!(
        "pid=$1; shift; i=0; \
         while kill -0 \"$pid\" 2>/dev/null && [ $i -lt {ticks} ]; do sleep 0.2; i=$((i+1)); done; \
         if kill -0 \"$pid\" 2>/dev/null; then exit 0; fi; \
         exec \"$@\""
    );
    let mut command = std::process::Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(script)
        .arg("sh")
        .arg(pid.to_string())
        .args(launch);
    command
}

/// The helper that reopens `app` with `open -n`; its environment is cleared down to `helper_env`.
pub(super) fn relaunch_command(
    app: &Path,
    pid: u32,
    forwarded: &[(OsString, OsString)],
    helper_env: Vec<(OsString, OsString)>,
) -> std::process::Command {
    let mut launch: Vec<OsString> = vec!["/usr/bin/open".into(), "-n".into()];
    for (name, value) in forwarded {
        let mut assignment = name.clone();
        assignment.push("=");
        assignment.push(value);
        launch.push("--env".into());
        launch.push(assignment);
    }
    launch.push(app.into());
    let mut command = helper_command(RELAUNCH_WAIT_TICKS, pid, &launch);
    command.env_clear().envs(helper_env);
    command
}

pub(super) fn spawn_relaunch(app: &Path) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;

    relaunch_command(
        app,
        std::process::id(),
        &pick_env(std::env::vars_os(), FORWARDED_ENV),
        pick_env(std::env::vars_os(), HELPER_ENV),
    )
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .process_group(0)
    .spawn()
    .map(drop)
}
