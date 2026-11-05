//! Being the app the system opens magnet links and `.torrent` files with.
//!
//! [`check`] asks the system whether put.io adder is the default for both; [`register`] makes it the
//! default. Both may start other processes or call system services, so call them off the UI thread.
//!
//! How it is done differs per system, and so does how much an app is *allowed* to do:
//!
//! * **macOS**: Launch Services, for the `magnet` scheme and the `.torrent` file type. The app must run
//!   from an app bundle (that is what is registered).
//! * **Linux**: a desktop entry in `~/.local/share/applications` plus `xdg-mime default`.
//! * **Windows**: the per-user registry, through `reg.exe`. Windows 10 and 11 protect the user's own
//!   choice ("UserChoice"): an app cannot override it. If the user already picked another app, the
//!   only honest thing is to open *Settings → Default apps* and say so.
//!
//! A development build never registers itself: it would make the system open links with a binary in
//! a build folder.

// Compiled on every OS so the tests run everywhere; each OS uses only its own part.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use putio_core::handler::{Check, Status};

/// The macOS bundle id and the desktop-entry id of this app.
const BUNDLE_ID: &str = "es.nico.putio-adder";
const DESKTOP_FILE: &str = "es.nico.putio-adder.desktop";
const WHY_DEV: &str = "A development build is not made the default app.";

/// Whether put.io adder is the default app for magnet links and `.torrent` files, or why it cannot be.
#[must_use]
pub fn check() -> Check {
    if cfg!(debug_assertions) {
        return Check::Unavailable(WHY_DEV.to_owned());
    }
    match imp::check() {
        Ok(status) => Check::Known(status),
        Err(reason) => Check::Unavailable(reason),
    }
}

/// Make put.io adder the default app for magnet links and `.torrent` files, and say what the system
/// then reports.
pub fn register() -> Result<Status, String> {
    if cfg!(debug_assertions) {
        return Err(WHY_DEV.to_owned());
    }
    imp::register()
}

/// The program's own path, as the system would start it.
fn current_exe() -> Result<PathBuf, String> {
    std::env::current_exe()
        .and_then(|exe| exe.canonicalize())
        .map_err(|error| format!("cannot find this app's own path: {error}"))
}

/// Run a program with no input and collect what it prints (stdout, then stderr on failure).
fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("could not run {program}: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let error = String::from_utf8_lossy(&output.stderr);
        Err(format!("{program} failed: {}", error.trim()))
    }
}

// ---- macOS --------------------------------------------------------------------------------------

/// `/…/put.io adder.app` for `/…/put.io adder.app/Contents/MacOS/putio-adder`.
fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    let is_bundle = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && app.extension()? == "app";
    is_bundle.then(|| app.to_path_buf())
}

/// Why the running copy cannot be registered with Launch Services, if it cannot.
fn mac_problem(exe: &Path) -> Option<String> {
    let Some(app) = bundle_of(exe) else {
        return Some("put.io adder is not running from an app bundle.".to_owned());
    };
    app.components()
        .any(|part| part.as_os_str() == "AppTranslocation")
        .then(|| "Move put.io adder to the Applications folder first.".to_owned())
}

/// Whether a bundle id reported by Launch Services is this app's (the system may change the case).
fn is_our_bundle(reported: Option<&str>) -> bool {
    reported.is_some_and(|id| id.eq_ignore_ascii_case(BUNDLE_ID))
}

// ---- Linux --------------------------------------------------------------------------------------

const MIME_MAGNET: &str = "x-scheme-handler/magnet";
const MIME_TORRENT: &str = "application/x-bittorrent";
const DESKTOP_TEMPLATE: &str = include_str!("../../../packaging/linux/es.nico.putio-adder.desktop");

/// `exe` as the `Exec=` value of a desktop entry: quoted, with the characters the specification gives
/// a meaning to escaped. A path with a line break (or any control character) is refused: it would end
/// the line and let the rest be read as more keys.
fn desktop_exec(exe: &Path) -> Result<String, String> {
    let path = exe.to_str().ok_or("the app's path is not plain text")?;
    // A desktop entry is a Linux thing: its paths start with a slash, whatever system runs this code.
    if path.chars().any(char::is_control) || !path.starts_with('/') {
        return Err("the app's path cannot be used in a desktop entry".to_owned());
    }
    let mut quoted = String::from("\"");
    for c in path.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(c);
            }
            // `%` starts a field code; `%%` is a literal percent sign.
            '%' => quoted.push_str("%%"),
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    Ok(quoted)
}

/// The desktop entry for the app installed at `exe`: the packaged one with an absolute `Exec`.
fn desktop_entry(exe: &Path) -> Result<String, String> {
    let exec = desktop_exec(exe)?;
    let template_line = "Exec=putio-adder %U";
    if !DESKTOP_TEMPLATE.contains(template_line) {
        return Err("the packaged desktop entry has no Exec line to fill in".to_owned());
    }
    Ok(DESKTOP_TEMPLATE.replace(template_line, &format!("Exec={exec} %U")))
}

/// Whether `xdg-mime`'s answer names this app.
fn is_our_desktop_file(answer: Option<&str>) -> bool {
    answer.is_some_and(|name| name.trim() == DESKTOP_FILE)
}

fn linux_status(query: &dyn Fn(&str) -> Option<String>) -> Status {
    Status {
        magnet: is_our_desktop_file(query(MIME_MAGNET).as_deref()),
        torrent: is_our_desktop_file(query(MIME_TORRENT).as_deref()),
    }
}

/// `$XDG_DATA_HOME/applications`, or `~/.local/share/applications`.
fn applications_dir(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let os = crate::paths::Os::Linux;
    let absolute = |name: &str| env(name).filter(|v| crate::paths::is_absolute_for(os, v));
    let data = absolute("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| absolute("HOME").map(|home| PathBuf::from(home).join(".local/share")))?;
    Some(data.join("applications"))
}

// ---- Windows ------------------------------------------------------------------------------------

const PROG_ID_MAGNET: &str = "putio-adder.magnet";
const PROG_ID_TORRENT: &str = "putio-adder.torrent";
const KEY_MAGNET: &str = r"HKCU\Software\Classes\magnet";
const KEY_TORRENT_EXT: &str = r"HKCU\Software\Classes\.torrent";
const USER_CHOICE_MAGNET: &str =
    r"HKCU\Software\Microsoft\Windows\Shell\Associations\UrlAssociations\magnet\UserChoice";
const USER_CHOICE_TORRENT: &str =
    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.torrent\UserChoice";

/// One registry value to write. `name` is `None` for the key's default value.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RegValue {
    key: String,
    name: Option<&'static str>,
    data: String,
}

fn reg_value(key: &str, name: Option<&'static str>, data: &str) -> RegValue {
    RegValue {
        key: key.to_owned(),
        name,
        data: data.to_owned(),
    }
}

/// `"C:\path\putio-adder.exe" "%1"`: refused for a path that is not an absolute drive or UNC path or
/// that holds a quote or a control character (it would break out of the quoting).
fn windows_command(exe: &Path) -> Result<String, String> {
    let path = exe.to_str().ok_or("the app's path is not plain text")?;
    let absolute = crate::paths::is_absolute_for(crate::paths::Os::Windows, path);
    if !absolute || path.contains('"') || path.chars().any(char::is_control) {
        return Err("the app's path cannot be used in a registry command".to_owned());
    }
    Ok(format!("\"{path}\" \"%1\""))
}

/// The per-user registry entries that make the app a *candidate* for both kinds of link (the ones
/// the installer writes too), and, when `claim` is set, the classic ones that make it the handler
/// when the user has not chosen another app.
fn windows_entries(
    exe: &Path,
    claim_magnet: bool,
    claim_torrent: bool,
) -> Result<Vec<RegValue>, String> {
    let command = windows_command(exe)?;
    let icon = exe.to_string_lossy();
    let mut entries = vec![
        reg_value(
            &format!(r"HKCU\Software\Classes\{PROG_ID_MAGNET}"),
            None,
            "URL:magnet Protocol",
        ),
        reg_value(
            &format!(r"HKCU\Software\Classes\{PROG_ID_MAGNET}"),
            Some("URL Protocol"),
            "",
        ),
        reg_value(
            &format!(r"HKCU\Software\Classes\{PROG_ID_MAGNET}\DefaultIcon"),
            None,
            &icon,
        ),
        reg_value(
            &format!(r"HKCU\Software\Classes\{PROG_ID_MAGNET}\shell\open\command"),
            None,
            &command,
        ),
        reg_value(
            &format!(r"HKCU\Software\Classes\{PROG_ID_TORRENT}"),
            None,
            "BitTorrent Document",
        ),
        reg_value(
            &format!(r"HKCU\Software\Classes\{PROG_ID_TORRENT}\DefaultIcon"),
            None,
            &icon,
        ),
        reg_value(
            &format!(r"HKCU\Software\Classes\{PROG_ID_TORRENT}\shell\open\command"),
            None,
            &command,
        ),
        reg_value(
            &format!(r"{KEY_TORRENT_EXT}\OpenWithProgids"),
            Some(PROG_ID_TORRENT),
            "",
        ),
        reg_value(
            r"HKCU\Software\putio-adder\Capabilities",
            Some("ApplicationName"),
            "put.io adder",
        ),
        reg_value(
            r"HKCU\Software\putio-adder\Capabilities\URLAssociations",
            Some("magnet"),
            PROG_ID_MAGNET,
        ),
        reg_value(
            r"HKCU\Software\putio-adder\Capabilities\FileAssociations",
            Some(".torrent"),
            PROG_ID_TORRENT,
        ),
        reg_value(
            r"HKCU\Software\RegisteredApplications",
            Some("putio-adder"),
            r"Software\putio-adder\Capabilities",
        ),
    ];
    if claim_magnet {
        entries.push(reg_value(KEY_MAGNET, None, "URL:magnet Protocol"));
        entries.push(reg_value(KEY_MAGNET, Some("URL Protocol"), ""));
        entries.push(reg_value(
            &format!(r"{KEY_MAGNET}\shell\open\command"),
            None,
            &command,
        ));
    }
    if claim_torrent {
        entries.push(reg_value(KEY_TORRENT_EXT, None, PROG_ID_TORRENT));
    }
    Ok(entries)
}

/// The arguments of `reg.exe add` that write `value`.
fn reg_add_args(value: &RegValue) -> Vec<String> {
    let mut args = vec!["add".to_owned(), value.key.clone()];
    match value.name {
        Some(name) => args.extend(["/v".to_owned(), name.to_owned()]),
        None => args.push("/ve".to_owned()),
    }
    args.extend([
        "/t".to_owned(),
        "REG_SZ".to_owned(),
        "/d".to_owned(),
        value.data.clone(),
        "/f".to_owned(),
    ]);
    args
}

/// The data of a `reg query` answer. The name of the default value is translated (`(Default)`,
/// `(Standard)`, …), so the line is found by its type, which is not.
fn parse_reg_query(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let (_, data) = line.split_once("REG_SZ")?;
        Some(data.trim().to_owned())
    })
}

/// Whether the registry says put.io adder opens `magnet:` links and `.torrent` files for this user.
/// `read` gives the string value of a key (`None` name = default), or `None` if there is none.
fn windows_status(read: &dyn Fn(&str, Option<&str>) -> Option<String>, exe: &Path) -> Status {
    let Ok(command) = windows_command(exe) else {
        return Status {
            magnet: false,
            torrent: false,
        };
    };
    // The user's own pick (Settings → Default apps) wins over everything else.
    let ours = |chosen: Option<String>, our_id: &str, classic: bool| match chosen {
        Some(id) => id == our_id,
        None => classic,
    };
    let magnet_command = read(&format!(r"{KEY_MAGNET}\shell\open\command"), None);
    let magnet_classic = magnet_command.as_deref() == Some(command.as_str());
    let torrent_ext = read(KEY_TORRENT_EXT, None);
    let torrent_command = read(
        &format!(r"HKCU\Software\Classes\{PROG_ID_TORRENT}\shell\open\command"),
        None,
    );
    let torrent_classic = torrent_ext.as_deref() == Some(PROG_ID_TORRENT)
        && torrent_command.as_deref() == Some(command.as_str());
    Status {
        magnet: ours(
            read(USER_CHOICE_MAGNET, Some("ProgId")),
            PROG_ID_MAGNET,
            magnet_classic,
        ),
        torrent: ours(
            read(USER_CHOICE_TORRENT, Some("ProgId")),
            PROG_ID_TORRENT,
            torrent_classic,
        ),
    }
}

fn reg_exe(env: &dyn Fn(&str) -> Option<String>) -> String {
    let root = env("SystemRoot")
        .filter(|root| crate::paths::is_absolute_for(crate::paths::Os::Windows, root))
        .unwrap_or_else(|| r"C:\Windows".to_owned());
    format!(r"{}\System32\reg.exe", root.trim_end_matches('\\'))
}

fn windows_read(key: &str, name: Option<&str>) -> Option<String> {
    let program = reg_exe(&crate::paths::process_env);
    let mut args = vec!["query", key];
    match name {
        Some(name) => args.extend(["/v", name]),
        None => args.push("/ve"),
    }
    parse_reg_query(&run(&program, &args).ok()?)
}

/// Does the registry hold a choice of the user's for `.torrent` or `magnet` that is another app?
fn windows_blocked(status: Status) -> Vec<&'static str> {
    let mut blocked = Vec::new();
    if !status.magnet {
        blocked.push("magnet links");
    }
    if !status.torrent {
        blocked.push(".torrent files");
    }
    blocked
}

// ---- the per-system part ------------------------------------------------------------------------

#[cfg(target_os = "macos")]
#[allow(unsafe_code)] // Launch Services calls; every block carries a SAFETY comment
mod imp {
    use objc2::rc::Retained;
    use objc2_foundation::NSString;

    use super::{BUNDLE_ID, Status, current_exe, is_our_bundle, mac_problem};

    /// Every role (viewer, editor, …): the app takes over opening the file type, not just one role.
    const ROLES_ALL: u32 = 0xFFFF_FFFF;

    // CoreFoundation strings and NSStrings are the same objects ("toll-free bridged"), so an `NSString`
    // pointer is a valid `CFStringRef` argument, and a returned `CFStringRef` can be owned as one.
    #[link(name = "CoreServices", kind = "framework")]
    unsafe extern "C" {
        fn LSCopyDefaultHandlerForURLScheme(scheme: *const NSString) -> *mut NSString;
        fn LSSetDefaultHandlerForURLScheme(
            scheme: *const NSString,
            bundle_id: *const NSString,
        ) -> i32;
        fn LSCopyDefaultRoleHandlerForContentType(
            content_type: *const NSString,
            roles: u32,
        ) -> *mut NSString;
        fn LSSetDefaultRoleHandlerForContentType(
            content_type: *const NSString,
            roles: u32,
            bundle_id: *const NSString,
        ) -> i32;
        fn UTTypeCreatePreferredIdentifierForTag(
            tag_class: *const NSString,
            tag: *const NSString,
            conforming_to: *const NSString,
        ) -> *mut NSString;
    }

    /// The address of an `NSString`, as the `CFStringRef` the Launch Services functions take.
    fn ptr(string: &NSString) -> *const NSString {
        std::ptr::from_ref(string)
    }

    /// The (dynamic) type identifier macOS gives `.torrent` files: no system type is declared for them.
    fn torrent_type() -> Option<Retained<NSString>> {
        let class = NSString::from_str("public.filename-extension");
        let tag = NSString::from_str("torrent");
        // SAFETY: both arguments are valid NSStrings; the function returns a +1 retained CFString or null.
        let raw = unsafe {
            UTTypeCreatePreferredIdentifierForTag(ptr(&class), ptr(&tag), std::ptr::null())
        };
        // SAFETY: `raw` is null or an owned (+1) CFString, which is an NSString.
        unsafe { Retained::from_raw(raw) }
    }

    fn handler_of_scheme(scheme: &str) -> Option<String> {
        let scheme = NSString::from_str(scheme);
        // SAFETY: valid NSString argument; returns a +1 retained CFString or null.
        let raw = unsafe { LSCopyDefaultHandlerForURLScheme(ptr(&scheme)) };
        // SAFETY: `raw` is null or an owned CFString.
        unsafe { Retained::from_raw(raw) }.map(|id| id.to_string())
    }

    fn handler_of_type(content_type: &NSString) -> Option<String> {
        // SAFETY: valid NSString argument; returns a +1 retained CFString or null.
        let raw = unsafe { LSCopyDefaultRoleHandlerForContentType(ptr(content_type), ROLES_ALL) };
        // SAFETY: `raw` is null or an owned CFString.
        unsafe { Retained::from_raw(raw) }.map(|id| id.to_string())
    }

    pub(super) fn status() -> Result<Status, String> {
        let torrent_type = torrent_type().ok_or("macOS has no type for .torrent files")?;
        Ok(Status {
            magnet: is_our_bundle(handler_of_scheme("magnet").as_deref()),
            torrent: is_our_bundle(handler_of_type(&torrent_type).as_deref()),
        })
    }

    pub(super) fn check() -> Result<Status, String> {
        if let Some(problem) = mac_problem(&current_exe()?) {
            return Err(problem);
        }
        status()
    }

    pub(super) fn register() -> Result<Status, String> {
        if let Some(problem) = mac_problem(&current_exe()?) {
            return Err(problem);
        }
        let id = NSString::from_str(BUNDLE_ID);
        let scheme = NSString::from_str("magnet");
        let torrent_type = torrent_type().ok_or("macOS has no type for .torrent files")?;
        // SAFETY: valid NSString arguments for both calls; they return an OSStatus.
        let (magnet, torrent) = unsafe {
            (
                LSSetDefaultHandlerForURLScheme(ptr(&scheme), ptr(&id)),
                LSSetDefaultRoleHandlerForContentType(ptr(&torrent_type), ROLES_ALL, ptr(&id)),
            )
        };
        if magnet != 0 || torrent != 0 {
            return Err(format!(
                "macOS refused the change (codes {magnet} and {torrent})"
            ));
        }
        status()
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{
        DESKTOP_FILE, MIME_MAGNET, MIME_TORRENT, Status, applications_dir, current_exe,
        desktop_entry, linux_status, run,
    };

    fn query(mime: &str) -> Option<String> {
        run("xdg-mime", &["query", "default", mime]).ok()
    }

    pub(super) fn check() -> Result<Status, String> {
        run("xdg-mime", &["--version"])
            .map_err(|_| "xdg-mime was not found, so the default app cannot be set.".to_owned())?;
        Ok(linux_status(&query))
    }

    pub(super) fn register() -> Result<Status, String> {
        check()?;
        let entry = desktop_entry(&current_exe()?)?;
        let dir = applications_dir(&crate::paths::process_env)
            .ok_or("cannot find the applications folder")?;
        std::fs::create_dir_all(&dir)
            .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
        let file = dir.join(DESKTOP_FILE);
        // Only write when it differs, and atomically, so a launcher reading it never sees half a file.
        if std::fs::read_to_string(&file).ok().as_deref() != Some(entry.as_str()) {
            let temporary = dir.join(format!(".{DESKTOP_FILE}.tmp"));
            std::fs::write(&temporary, &entry)
                .map_err(|error| format!("cannot write the desktop entry: {error}"))?;
            std::fs::rename(&temporary, &file)
                .map_err(|error| format!("cannot write the desktop entry: {error}"))?;
        }
        let dir_text = dir.to_string_lossy().into_owned();
        let _ = run("update-desktop-database", &[&dir_text]); // optional: only speeds up discovery
        run(
            "xdg-mime",
            &["default", DESKTOP_FILE, MIME_MAGNET, MIME_TORRENT],
        )?;
        check()
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use super::{
        Status, current_exe, reg_add_args, reg_exe, windows_blocked, windows_entries, windows_read,
        windows_status,
    };

    pub(super) fn check() -> Result<Status, String> {
        Ok(windows_status(&windows_read, &current_exe()?))
    }

    fn write(entries: &[super::RegValue]) -> Result<(), String> {
        let program = reg_exe(&crate::paths::process_env);
        for entry in entries {
            let args = reg_add_args(entry);
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            super::run(&program, &refs)?;
        }
        Ok(())
    }

    pub(super) fn register() -> Result<Status, String> {
        let exe = current_exe()?;
        // Take the classic keys only where the user has not made a choice of their own: Windows ignores
        // them otherwise, and writing them anyway would only hide the real state.
        let user_choice_magnet = windows_read(super::USER_CHOICE_MAGNET, Some("ProgId")).is_some();
        let user_choice_torrent =
            windows_read(super::USER_CHOICE_TORRENT, Some("ProgId")).is_some();
        write(&windows_entries(
            &exe,
            !user_choice_magnet,
            !user_choice_torrent,
        )?)?;
        let after = windows_status(&windows_read, &exe);
        if after.is_default() {
            return Ok(after);
        }
        // Windows lets only the user replace an app they chose themselves.
        let _ = std::process::Command::new(format!(
            r"{}\explorer.exe",
            std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned())
        ))
        .arg("ms-settings:defaultapps")
        .spawn();
        Err(format!(
            "Windows keeps your earlier choice for {}. Pick put.io adder under Settings, Default apps.",
            windows_blocked(after).join(" and ")
        ))
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
mod imp {
    use super::Status;

    pub(super) fn check() -> Result<Status, String> {
        Err("this system has no default-app setting that put.io adder can change".to_owned())
    }

    pub(super) fn register() -> Result<Status, String> {
        check()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    // ---- development builds -------------------------------------------------------------------

    #[test]
    fn a_development_build_is_never_registered() {
        // The tests are a debug build.
        assert!(
            matches!(check(), Check::Unavailable(reason) if reason.contains("development build"))
        );
        assert!(register().unwrap_err().contains("development build"));
    }

    // ---- macOS --------------------------------------------------------------------------------

    #[test]
    fn only_a_real_app_bundle_can_be_registered() {
        let ok = Path::new("/Applications/put.io adder.app/Contents/MacOS/putio-adder");
        assert_eq!(mac_problem(ok), None);
        assert_eq!(
            bundle_of(ok),
            Some(PathBuf::from("/Applications/put.io adder.app"))
        );
        let loose = Path::new("/usr/local/bin/putio-adder");
        assert!(mac_problem(loose).unwrap().contains("app bundle"));
        assert!(mac_problem(Path::new("/work/target/release/putio-adder")).is_some());
        let moved = "/private/var/folders/x/AppTranslocation/ABC/d/put.io adder.app/Contents/MacOS/putio-adder";
        assert!(
            mac_problem(Path::new(moved))
                .unwrap()
                .contains("Applications folder")
        );
    }

    #[test]
    fn the_reported_bundle_id_is_compared_without_regard_to_case() {
        assert!(is_our_bundle(Some("es.nico.putio-adder")));
        assert!(is_our_bundle(Some("ES.NICO.PUTIO-ADDER")));
        assert!(!is_our_bundle(Some("org.qbittorrent.qBittorrent")));
        assert!(!is_our_bundle(Some("es.nico.putio-adder.evil")));
        assert!(!is_our_bundle(None));
    }

    // ---- Linux --------------------------------------------------------------------------------

    #[test]
    fn a_path_becomes_a_quoted_exec_value_with_the_special_characters_escaped() {
        let exec = |p: &str| desktop_exec(Path::new(p)).unwrap();
        assert_eq!(
            exec("/home/me/.local/bin/putio-adder"),
            r#""/home/me/.local/bin/putio-adder""#
        );
        assert_eq!(
            exec("/home/my name/bin/putio-adder"),
            r#""/home/my name/bin/putio-adder""#
        );
        assert_eq!(exec(r#"/a/b"c/d"#), r#""/a/b\"c/d""#);
        assert_eq!(exec("/a/$HOME/`x`/d"), r#""/a/\$HOME/\`x\`/d""#);
        assert_eq!(exec(r"/a/back\slash"), r#""/a/back\\slash""#);
        assert_eq!(exec("/a/100%/d"), r#""/a/100%%/d""#);
    }

    #[test]
    fn a_path_that_could_inject_another_line_is_refused() {
        for bad in [
            "/a/b\nExec=evil",
            "/a/b\rX",
            "/a/\u{0}b",
            "/a/\u{1b}[0m",
            "relative/putio-adder",
            "putio-adder",
        ] {
            assert!(desktop_exec(Path::new(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_desktop_entry_is_the_packaged_one_with_an_absolute_exec() {
        let entry = desktop_entry(Path::new("/home/me/.local/bin/putio-adder")).unwrap();
        assert!(entry.contains("Exec=\"/home/me/.local/bin/putio-adder\" %U\n"));
        assert!(!entry.contains("Exec=putio-adder"));
        for kept in [
            "[Desktop Entry]",
            "MimeType=x-scheme-handler/magnet;application/x-bittorrent;",
            "StartupWMClass=es.nico.putio-adder",
        ] {
            assert!(entry.contains(kept), "{kept}");
        }
        assert_eq!(
            entry.lines().count(),
            DESKTOP_TEMPLATE.lines().count(),
            "no line was added"
        );
        assert!(desktop_entry(Path::new("/x\ny")).is_err());
    }

    #[test]
    fn xdg_mime_answers_are_matched_to_this_app() {
        assert!(is_our_desktop_file(Some("es.nico.putio-adder.desktop\n")));
        assert!(!is_our_desktop_file(Some(
            "org.qbittorrent.qBittorrent.desktop"
        )));
        assert!(!is_our_desktop_file(Some("")));
        assert!(!is_our_desktop_file(None));
        assert!(linux_status(&|_| Some(DESKTOP_FILE.to_owned())).is_default());
        let magnet_only =
            linux_status(&|mime| (mime == MIME_MAGNET).then(|| DESKTOP_FILE.to_owned()));
        assert_eq!((magnet_only.magnet, magnet_only.torrent), (true, false));
        assert!(!linux_status(&|_| None).magnet);
    }

    #[test]
    fn the_applications_folder_follows_the_xdg_rules_and_trusts_only_absolute_paths() {
        let dir = |vars: &[(&str, &str)]| {
            let vars: HashMap<String, String> = vars
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect();
            applications_dir(&move |name| vars.get(name).cloned())
        };
        let home = PathBuf::from("/home/me/.local/share/applications");
        assert_eq!(dir(&[("HOME", "/home/me")]), Some(home.clone()));
        assert_eq!(
            dir(&[("HOME", "/home/me"), ("XDG_DATA_HOME", "/data")]),
            Some(PathBuf::from("/data/applications"))
        );
        assert_eq!(
            dir(&[("HOME", "/home/me"), ("XDG_DATA_HOME", "relative/dir")]),
            Some(home),
            "a relative XDG_DATA_HOME is ignored"
        );
        assert_eq!(dir(&[("HOME", "home")]), None);
        assert_eq!(dir(&[]), None);
    }

    // ---- Windows ------------------------------------------------------------------------------

    const EXE: &str = r"C:\Users\me\AppData\Local\Programs\put.io adder\putio-adder.exe";
    const COMMAND: &str =
        r#""C:\Users\me\AppData\Local\Programs\put.io adder\putio-adder.exe" "%1""#;
    type Registry = HashMap<(String, Option<String>), String>;

    #[test]
    fn the_registry_command_quotes_the_path_and_refuses_anything_that_could_break_out() {
        assert_eq!(windows_command(Path::new(EXE)).unwrap(), COMMAND);
        assert!(windows_command(Path::new(r"\\server\share\putio-adder.exe")).is_ok());
        for bad in [
            r#"C:\a"b\x.exe"#,
            "C:\\a\nb\\x.exe",
            r"relative\x.exe",
            "putio-adder.exe",
            "",
        ] {
            assert!(windows_command(Path::new(bad)).is_err(), "{bad:?}");
        }
    }

    fn find(entries: &[RegValue], key: &str, name: Option<&str>) -> Option<String> {
        entries
            .iter()
            .find(|e| e.key == key && e.name == name)
            .map(|e| e.data.clone())
    }

    #[test]
    fn the_candidate_entries_are_always_written_and_the_claim_only_when_asked() {
        let exe = Path::new(EXE);
        let candidate = windows_entries(exe, false, false).unwrap();
        let magnet_command = r"HKCU\Software\Classes\putio-adder.magnet\shell\open\command";
        assert_eq!(
            find(&candidate, magnet_command, None).as_deref(),
            Some(COMMAND)
        );
        assert_eq!(
            find(
                &candidate,
                r"HKCU\Software\Classes\putio-adder.magnet",
                Some("URL Protocol")
            )
            .as_deref(),
            Some("")
        );
        assert_eq!(
            find(
                &candidate,
                r"HKCU\Software\RegisteredApplications",
                Some("putio-adder")
            )
            .as_deref(),
            Some(r"Software\putio-adder\Capabilities")
        );
        assert!(
            find(&candidate, KEY_MAGNET, None).is_none(),
            "no claim on the plain magnet key"
        );
        assert!(
            find(&candidate, KEY_TORRENT_EXT, None).is_none(),
            "no claim on .torrent"
        );

        let magnet_only = windows_entries(exe, true, false).unwrap();
        let claim = format!(r"{KEY_MAGNET}\shell\open\command");
        assert_eq!(find(&magnet_only, &claim, None).as_deref(), Some(COMMAND));
        assert!(find(&magnet_only, KEY_TORRENT_EXT, None).is_none());
        let torrent_only = windows_entries(exe, false, true).unwrap();
        assert_eq!(
            find(&torrent_only, KEY_TORRENT_EXT, None).as_deref(),
            Some(PROG_ID_TORRENT)
        );
        assert!(windows_entries(Path::new("relative.exe"), true, true).is_err());
        // Every key is under the current user: nothing needs administrator rights.
        let all = windows_entries(exe, true, true).unwrap();
        assert!(all.iter().all(|e| e.key.starts_with(r"HKCU\")));
    }

    #[test]
    fn registry_writes_are_built_as_arguments_not_as_a_shell_line() {
        let named = reg_value(r"HKCU\Software\X", Some("URL Protocol"), "");
        assert_eq!(
            reg_add_args(&named),
            [
                "add",
                r"HKCU\Software\X",
                "/v",
                "URL Protocol",
                "/t",
                "REG_SZ",
                "/d",
                "",
                "/f"
            ]
        );
        let default = reg_value(r"HKCU\Software\X\command", None, COMMAND);
        let args = reg_add_args(&default);
        assert_eq!(&args[..3], ["add", r"HKCU\Software\X\command", "/ve"]);
        assert_eq!(args[5], "/d");
        assert_eq!(
            args[6], COMMAND,
            "the whole command is one argument, quotes and all"
        );
    }

    #[test]
    fn a_registry_answer_is_read_by_its_type_whatever_language_the_default_is_named_in() {
        let english = "\nHKEY_CURRENT_USER\\Software\\Classes\\magnet\n    (Default)    REG_SZ    URL:magnet Protocol\n\n";
        let german = "\nHKEY_CURRENT_USER\\Software\\Classes\\magnet\n    (Standard)    REG_SZ    URL:magnet Protocol\n";
        assert_eq!(
            parse_reg_query(english).as_deref(),
            Some("URL:magnet Protocol")
        );
        assert_eq!(
            parse_reg_query(german).as_deref(),
            Some("URL:magnet Protocol")
        );
        let progid = "    ProgId    REG_SZ    FirefoxURL-308046B0AF4A39CB\n";
        assert_eq!(
            parse_reg_query(progid).as_deref(),
            Some("FirefoxURL-308046B0AF4A39CB")
        );
        assert_eq!(
            parse_reg_query("    (Default)    REG_SZ    \n").as_deref(),
            Some("")
        );
        let missing = "ERROR: The system was unable to find the specified registry key or value.";
        assert_eq!(parse_reg_query(missing), None);
        assert_eq!(parse_reg_query(""), None);
    }

    fn status_in(values: &Registry) -> Status {
        let read = |key: &str, name: Option<&str>| {
            values
                .get(&(key.to_owned(), name.map(str::to_owned)))
                .cloned()
        };
        windows_status(&read, Path::new(EXE))
    }

    fn claimed() -> Registry {
        windows_entries(Path::new(EXE), true, true)
            .unwrap()
            .into_iter()
            .map(|e| ((e.key, e.name.map(str::to_owned)), e.data))
            .collect()
    }

    fn user_choice(map: &mut Registry, key: &str, progid: &str) {
        map.insert(
            (key.to_owned(), Some("ProgId".to_owned())),
            progid.to_owned(),
        );
    }

    #[test]
    fn nothing_registered_means_not_the_default_and_a_full_claim_means_the_default() {
        assert_eq!(
            status_in(&HashMap::new()),
            Status {
                magnet: false,
                torrent: false
            }
        );
        assert!(status_in(&claimed()).is_default());
    }

    #[test]
    fn a_choice_the_user_made_in_settings_decides_whatever_the_classic_keys_say() {
        let mut map = claimed();
        user_choice(&mut map, USER_CHOICE_MAGNET, "FirefoxURL-308046B0AF4A39CB");
        let status = status_in(&map);
        assert_eq!((status.magnet, status.torrent), (false, true));
        user_choice(&mut map, USER_CHOICE_TORRENT, "qBittorrent.File");
        let status = status_in(&map);
        assert_eq!((status.magnet, status.torrent), (false, false));
        // Choosing put.io adder there is what makes it the default.
        user_choice(&mut map, USER_CHOICE_MAGNET, PROG_ID_MAGNET);
        user_choice(&mut map, USER_CHOICE_TORRENT, PROG_ID_TORRENT);
        assert!(status_in(&map).is_default());
        let mut only_choices = HashMap::new();
        user_choice(&mut only_choices, USER_CHOICE_MAGNET, PROG_ID_MAGNET);
        user_choice(&mut only_choices, USER_CHOICE_TORRENT, PROG_ID_TORRENT);
        assert!(status_in(&only_choices).is_default());
    }

    #[test]
    fn a_registration_that_points_at_a_moved_copy_is_not_the_default() {
        let mut map = claimed();
        let claim = format!(r"{KEY_MAGNET}\shell\open\command");
        map.insert((claim, None), r#""C:\Old\putio-adder.exe" "%1""#.to_owned());
        let status = status_in(&map);
        assert_eq!((status.magnet, status.torrent), (false, true));
        let mut map = claimed();
        map.insert(
            (KEY_TORRENT_EXT.to_owned(), None),
            "qBittorrent.File".to_owned(),
        );
        let status = status_in(&map);
        assert_eq!((status.magnet, status.torrent), (true, false));
    }

    #[test]
    fn the_reg_program_is_found_by_an_absolute_path_under_the_windows_folder() {
        let at = |value: Option<&str>| reg_exe(&|_| value.map(str::to_owned));
        assert_eq!(at(Some(r"D:\Windows")), r"D:\Windows\System32\reg.exe");
        assert_eq!(at(Some(r"D:\Windows\")), r"D:\Windows\System32\reg.exe");
        assert_eq!(at(Some("relative")), r"C:\Windows\System32\reg.exe");
        assert_eq!(at(None), r"C:\Windows\System32\reg.exe");
    }

    #[test]
    fn the_message_names_what_windows_kept() {
        let kept = |magnet, torrent| windows_blocked(Status { magnet, torrent });
        assert_eq!(kept(false, true), ["magnet links"]);
        assert_eq!(kept(false, false), ["magnet links", ".torrent files"]);
        assert_eq!(kept(true, true), Vec::<&str>::new());
    }

    /// The real Launch Services, read-only: the calls work, return plain answers and agree with
    /// themselves. (What the answers are depends on the Mac: only their shape is checked. Nothing here
    /// changes a default app.)
    #[cfg(target_os = "macos")]
    #[test]
    fn launch_services_answers_for_both_kinds_of_link() {
        let first = imp::status().unwrap();
        assert_eq!(
            imp::status().unwrap(),
            first,
            "asking twice gives the same answer"
        );
    }
}
