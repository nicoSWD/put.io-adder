use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use putio_core::update::UpdateAsset;

use super::archive::{self, BINARY_NAME, BUNDLE_NAME, EXE_NAME};
use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("putio-adder-update-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

// ---- file names -----------------------------------------------------------------------------

#[test]
fn every_supported_computer_has_exactly_one_release_file() {
    let name = |os, arch| asset_name_for("v0.2.0", os, arch);
    assert_eq!(
        name("macos", "aarch64").as_deref(),
        Some("putio-adder-v0.2.0-macos-arm64.zip")
    );
    assert_eq!(
        name("macos", "x86_64").as_deref(),
        Some("putio-adder-v0.2.0-macos-x64.zip")
    );
    assert_eq!(
        name("windows", "x86_64").as_deref(),
        Some("putio-adder-v0.2.0-windows-x64.zip")
    );
    assert_eq!(
        name("windows", "aarch64").as_deref(),
        Some("putio-adder-v0.2.0-windows-arm64.zip")
    );
    assert_eq!(
        name("linux", "x86_64").as_deref(),
        Some("putio-adder-v0.2.0-linux-x86_64.tar.gz")
    );
    assert_eq!(
        name("linux", "aarch64").as_deref(),
        Some("putio-adder-v0.2.0-linux-aarch64.tar.gz")
    );
    assert_eq!(name("freebsd", "x86_64"), None);
    assert_eq!(name("linux", "riscv64"), None);
    // The release workflow must publish exactly these names (see docs/updates.md).
    assert!(
        asset_name("v1.0.0").is_some()
            || !cfg!(any(target_os = "macos", windows, target_os = "linux"))
    );
}

#[test]
fn the_pid_after_the_relaunch_flag_is_read_and_nothing_else_is() {
    let args = |list: &[&str]| {
        list.iter()
            .map(OsString::from)
            .collect::<Vec<_>>()
            .into_iter()
    };
    assert_eq!(
        predecessor_pid(args(&["app", "--relaunch-after", "1234"])),
        Some(1234)
    );
    assert_eq!(
        predecessor_pid(args(&["app", "x.torrent", "--relaunch-after", "7"])),
        Some(7)
    );
    assert_eq!(predecessor_pid(args(&["app"])), None);
    assert_eq!(predecessor_pid(args(&["app", "--relaunch-after"])), None);
    assert_eq!(
        predecessor_pid(args(&["app", "--relaunch-after", "abc"])),
        None
    );
    assert_eq!(
        predecessor_pid(args(&["app", "--relaunch-after", "-1"])),
        None
    );
    // The program name itself is not an argument.
    assert_eq!(predecessor_pid(args(&["--relaunch-after", "5"])), None);
}

// ---- the zip directory ----------------------------------------------------------------------

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in entries {
        writer.start_file(*name, options).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[test]
fn a_plain_zip_passes_the_directory_check() {
    assert!(archive::precheck(&zip_of(&[("a.txt", b"x"), ("dir/b.txt", b"y")])).is_ok());
}

#[test]
fn zips_with_unsafe_names_or_no_directory_are_refused() {
    for name in ["../escape", "/abs", "a/../../b", "..", "dir\\file"] {
        let error = archive::precheck(&zip_of(&[(name, b"x")])).unwrap_err();
        assert!(error.contains("unsafe"), "{name}: {error}");
    }
    for junk in [
        &b""[..],
        b"not a zip",
        &[0x50, 0x4b, 0x05, 0x06][..],
        &[0u8; 100][..],
    ] {
        assert!(archive::precheck(junk).is_err());
    }
    // A zip with no entries at all.
    let empty = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()))
        .finish()
        .unwrap()
        .into_inner();
    assert!(
        archive::precheck(&empty)
            .unwrap_err()
            .contains("number of files")
    );
}

#[test]
fn a_zip_that_declares_too_much_data_is_refused() {
    let mut zip = zip_of(&[("a.bin", b"abcd")]);
    // Patch the declared uncompressed size in the central directory (offset 24 into the entry).
    let eocd = zip
        .windows(4)
        .rposition(|w| w == [0x50, 0x4b, 0x05, 0x06])
        .unwrap();
    let directory_at = u32::from_le_bytes(zip[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
    zip[directory_at + 24..directory_at + 28].copy_from_slice(&0x7fff_ffff_u32.to_le_bytes());
    assert!(archive::precheck(&zip).unwrap_err().contains("more data"));
}

// ---- Windows: the one exe in a zip ----------------------------------------------------------

#[test]
fn only_the_executable_is_taken_out_of_the_zip() {
    let dir = scratch("exe");
    let zip = zip_of(&[
        (EXE_NAME, b"MZ-fake"),
        ("LICENSE", b"mit"),
        ("licenses/x.txt", b"y"),
    ]);
    let exe = archive::extract_exe(&zip, &dir.join("new")).unwrap();
    assert_eq!(exe, dir.join("new").join(EXE_NAME));
    assert_eq!(fs::read(&exe).unwrap(), b"MZ-fake");
    assert_eq!(
        fs::read_dir(dir.join("new")).unwrap().count(),
        1,
        "nothing else is written"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_zip_without_the_executable_is_not_this_app() {
    let dir = scratch("exe-missing");
    let error = archive::extract_exe(&zip_of(&[("other.exe", b"x")]), &dir).unwrap_err();
    assert!(error.contains("not a put.io adder app"), "{error}");
    let _ = fs::remove_dir_all(dir);
}

fn pe_file(path: &Path, machine: u16) {
    let mut bytes = vec![0u8; 0x100];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x3C..0x40].copy_from_slice(&0x80_u32.to_le_bytes());
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    bytes[0x84..0x86].copy_from_slice(&machine.to_le_bytes());
    fs::write(path, bytes).unwrap();
}

#[test]
fn a_windows_executable_must_be_a_pe_image_for_this_cpu() {
    let dir = scratch("pe");
    let exe = dir.join("app.exe");
    let (mine, other) = if cfg!(target_arch = "aarch64") {
        (0xAA64, 0x8664)
    } else {
        (0x8664, 0xAA64)
    };
    pe_file(&exe, mine);
    assert!(archive::check_exe(&exe).is_ok());
    pe_file(&exe, other);
    assert!(
        archive::check_exe(&exe)
            .unwrap_err()
            .contains("different kind of processor")
    );
    fs::write(&exe, b"#!/bin/sh\necho hi\n").unwrap();
    assert!(archive::check_exe(&exe).is_err());
    fs::write(&exe, b"MZ").unwrap();
    assert!(archive::check_exe(&exe).is_err(), "truncated header");
    assert!(archive::check_exe(&dir.join("missing.exe")).is_err());
    let _ = fs::remove_dir_all(dir);
}

// ---- Linux: the one binary in a tarball -----------------------------------------------------

fn elf_bytes(machine: u16) -> Vec<u8> {
    let mut bytes = vec![0u8; 64];
    bytes[..4].copy_from_slice(b"\x7fELF");
    bytes[4] = 2; // 64-bit
    bytes[5] = 1; // little endian
    bytes[16..18].copy_from_slice(&3u16.to_le_bytes()); // ET_DYN
    bytes[18..20].copy_from_slice(&machine.to_le_bytes());
    bytes
}

fn this_cpu_elf() -> Vec<u8> {
    elf_bytes(if cfg!(target_arch = "aarch64") {
        0xB7
    } else {
        0x3E
    })
}

fn tarball(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (name, data) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        builder.append_data(&mut header, name, *data).unwrap();
    }
    let tar = builder.into_inner().unwrap();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&tar).unwrap();
    gz.finish().unwrap()
}

const ENTRY: &str = "putio-adder-v0.2.0-linux-x86_64/putio-adder";

#[test]
fn the_binary_is_taken_out_of_its_folder_in_the_tarball_and_made_executable() {
    let dir = scratch("tar");
    let bytes = tarball(&[
        ("putio-adder-v0.2.0-linux-x86_64/LICENSE", b"mit"),
        (ENTRY, b"binary"),
        ("putio-adder-v0.2.0-linux-x86_64/install.sh", b"#!/bin/sh"),
    ]);
    let path = archive::extract_tar_gz_binary(&bytes, ENTRY, &dir.join("new")).unwrap();
    assert_eq!(path, dir.join("new").join(BINARY_NAME));
    assert_eq!(fs::read(&path).unwrap(), b"binary");
    assert_eq!(fs::read_dir(dir.join("new")).unwrap().count(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_tarball_without_the_binary_or_with_it_twice_or_in_the_wrong_place_is_refused() {
    let dir = scratch("tar-bad");
    let out = dir.join("new");
    let not_app =
        |bytes: &[u8], entry: &str| archive::extract_tar_gz_binary(bytes, entry, &out).unwrap_err();
    assert!(not_app(&tarball(&[("other/file", b"x")]), ENTRY).contains("not a put.io adder app"));
    assert!(not_app(&tarball(&[("putio-adder", b"x")]), ENTRY).contains("not a put.io adder app"));
    assert!(
        not_app(&tarball(&[(ENTRY, b"a"), (ENTRY, b"b")]), ENTRY)
            .contains("not a put.io adder app")
    );
    assert!(not_app(b"plain text", ENTRY).contains("not a valid archive"));
    assert!(not_app(&[0x1f, 0x8b, 0, 0], ENTRY).contains("Could not unpack"));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_tarball_that_inflates_past_the_limit_is_cut_off() {
    let dir = scratch("tar-big");
    let bytes = tarball(&[(ENTRY, &vec![7u8; 200_000])]);
    let result = archive::extract_tar_gz_binary_within(&bytes, ENTRY, &dir.join("new"), 50_000);
    assert!(result.is_err());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_linux_binary_must_be_a_64_bit_elf_for_this_cpu() {
    let dir = scratch("elf");
    let file = dir.join("bin");
    fs::write(&file, this_cpu_elf()).unwrap();
    assert!(archive::check_elf(&file).is_ok());
    fs::write(
        &file,
        elf_bytes(if cfg!(target_arch = "aarch64") {
            0x3E
        } else {
            0xB7
        }),
    )
    .unwrap();
    assert!(
        archive::check_elf(&file)
            .unwrap_err()
            .contains("different kind of processor")
    );
    let mut elf32 = this_cpu_elf();
    elf32[4] = 1;
    fs::write(&file, &elf32).unwrap();
    assert!(archive::check_elf(&file).is_err());
    let mut object = this_cpu_elf();
    object[16] = 1; // ET_REL: not something to run
    fs::write(&file, &object).unwrap();
    assert!(archive::check_elf(&file).is_err());
    fs::write(&file, b"MZ").unwrap();
    assert!(archive::check_elf(&file).is_err());
    fs::write(&file, b"#!/bin/sh\n# long enough to read a header from\n").unwrap();
    assert!(archive::check_elf(&file).is_err());
    let _ = fs::remove_dir_all(dir);
}

// ---- the macOS bundle -----------------------------------------------------------------------

/// A bundle with the layout `tools/bundle-macos.sh` produces; the executable contains `marker`.
fn fake_app(root: &Path, marker: &str) -> PathBuf {
    let app = root.join(BUNDLE_NAME);
    fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    fs::create_dir_all(app.join("Contents/Resources")).unwrap();
    fs::write(
        app.join("Contents/Info.plist"),
        "<plist><dict><key>CFBundleIdentifier</key><string>es.nico.putio-adder</string></dict></plist>",
    )
    .unwrap();
    let exe = app.join("Contents/MacOS/putio-adder");
    fs::write(&exe, marker).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(app.join("Contents/Resources/AppIcon.icns"), "icon").unwrap();
    app
}

#[test]
fn a_bundle_with_the_expected_layout_passes() {
    let root = scratch("bundle-ok");
    assert!(archive::check_bundle(&fake_app(&root, "x")).is_ok());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_bundle_that_is_not_this_app_is_refused() {
    let root = scratch("bundle-bad");
    let app = fake_app(&root, "x");
    fs::write(
        app.join("Contents/Info.plist"),
        "<string>com.example.other</string>",
    )
    .unwrap();
    assert!(archive::check_bundle(&app).is_err(), "other bundle id");
    let app = fake_app(&root, "x");
    fs::remove_file(app.join("Contents/MacOS/putio-adder")).unwrap();
    assert!(archive::check_bundle(&app).is_err(), "no executable");
    assert!(archive::check_bundle(&root.join("missing.app")).is_err());
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn a_bundle_with_extra_launchable_code_or_an_escaping_link_is_refused() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = scratch("bundle-extra");
    let app = fake_app(&root.join("case1"), "x");
    let extra = app.join("Contents/Resources/helper");
    fs::write(&extra, "#!/bin/sh").unwrap();
    fs::set_permissions(&extra, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        archive::check_bundle(&app)
            .unwrap_err()
            .contains("unexpected executable")
    );

    let app = fake_app(&root.join("case2"), "x");
    symlink("/usr/bin/true", app.join("Contents/Resources/link")).unwrap();
    assert!(
        archive::check_bundle(&app)
            .unwrap_err()
            .contains("links outside")
    );
    let app = fake_app(&root.join("case3"), "x");
    symlink("../../../outside", app.join("Contents/Resources/link")).unwrap();
    assert!(
        archive::check_bundle(&app)
            .unwrap_err()
            .contains("links outside")
    );
    // A link that stays inside is fine.
    let app = fake_app(&root.join("case4"), "x");
    symlink("AppIcon.icns", app.join("Contents/Resources/alias")).unwrap();
    assert!(archive::check_bundle(&app).is_ok());
    let _ = fs::remove_dir_all(root);
}

#[cfg(target_os = "macos")]
#[test]
fn a_real_zip_of_a_bundle_unpacks_with_ditto_and_is_checked() {
    let root = scratch("ditto");
    let app = fake_app(&root.join("src"), "new-version");
    let zip = root.join("release.zip");
    let status = std::process::Command::new("/usr/bin/ditto")
        .args(["-c", "-k", "--keepParent"])
        .arg(&app)
        .arg(&zip)
        .status()
        .unwrap();
    assert!(status.success());
    let staging = root.join("staging");
    fs::create_dir_all(&staging).unwrap();
    let new_app = super::bundle::unpack(&fs::read(&zip).unwrap(), &staging).unwrap();
    assert_eq!(new_app, staging.join("new").join(BUNDLE_NAME));
    assert_eq!(
        fs::read_to_string(new_app.join("Contents/MacOS/putio-adder")).unwrap(),
        "new-version"
    );
    assert!(
        !staging.join("update.zip").exists(),
        "the zip is removed once unpacked"
    );
    // Garbage that is not a zip never reaches ditto.
    assert!(super::bundle::unpack(b"not a zip", &staging).is_err());
    let _ = fs::remove_dir_all(root);
}

// The bundle strategy is compiled on Unix only (macOS uses it; Linux for the tests).
#[cfg(unix)]
#[test]
fn the_bundle_is_found_from_the_executable_inside_it() {
    let found = |p: &str| super::bundle::bundle_for_executable(Path::new(p));
    assert_eq!(
        found("/Applications/put.io adder.app/Contents/MacOS/putio-adder"),
        Some(PathBuf::from("/Applications/put.io adder.app"))
    );
    assert_eq!(found("/usr/local/bin/putio-adder"), None);
    assert_eq!(found("/Applications/Foo/Contents/MacOS/x"), None);
    assert_eq!(found("/Applications/Foo.app/Contents/Other/x"), None);
}

#[cfg(unix)]
#[test]
fn the_macos_helper_waits_for_the_old_process_and_only_forwards_what_it_is_given() {
    let app = Path::new("/Applications/put.io adder.app");
    let forwarded = vec![(OsString::from("HOME"), OsString::from("/Users/me"))];
    let helper_env = vec![(OsString::from("PATH"), OsString::from("/usr/bin"))];
    let command = super::bundle::relaunch_command(app, 4242, &forwarded, helper_env);
    let args: Vec<String> = command
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(command.get_program(), "/bin/sh");
    assert!(args[1].contains("kill -0"), "{args:?}");
    assert_eq!(&args[2..4], ["sh", "4242"]);
    assert_eq!(
        &args[4..],
        [
            "/usr/bin/open",
            "-n",
            "--env",
            "HOME=/Users/me",
            "/Applications/put.io adder.app"
        ]
    );
    let envs: Vec<_> = command.get_envs().collect();
    assert_eq!(
        envs.len(),
        1,
        "the helper's environment is cleared down to what it needs"
    );
}

// ---- Windows and Linux: where the executable is ---------------------------------------------

#[test]
fn a_development_build_never_replaces_itself() {
    let error = super::portable::target_for(
        Path::new("/work/put.io-adder/target/debug/putio-adder"),
        Path::new("/work/put.io-adder"),
    )
    .unwrap_err();
    assert!(error.contains("development build"), "{error}");
}

/// The root the code really computes (not a hand-made one) is the workspace, and the binary that is
/// running these tests, which `cargo` built under `target/`, counts as a development build.
#[test]
fn the_source_root_is_the_workspace_and_the_test_binary_is_a_development_build() {
    let root = super::portable::source_root();
    assert!(
        root.join("Cargo.lock").is_file(),
        "{} is not the workspace root",
        root.display()
    );
    assert!(root.join("crates").join("putio-platform").is_dir());
    let exe = std::env::current_exe().unwrap().canonicalize().unwrap();
    // (Unless the target folder was moved out of the tree: then only the debug-build rule applies.)
    if exe.starts_with(&root) {
        let error = super::portable::target_for(
            &super::portable::plain(&exe),
            &super::portable::plain(&root),
        )
        .unwrap_err();
        assert!(error.contains("development build"), "{error}");
    }
}

/// `make app` writes a bundle to `target/` inside the source tree: that is a development build.
#[cfg(unix)]
#[test]
fn a_bundle_built_inside_the_source_tree_never_updates_itself() {
    let root = scratch("bundle-dev");
    let exe = root.join("target/put.io adder.app/Contents/MacOS/putio-adder");
    let error = super::bundle::target_for(&exe, &root).unwrap_err();
    assert!(error.contains("development build"), "{error}");
}

#[cfg(unix)]
#[test]
fn a_bundle_outside_the_source_tree_in_a_folder_the_user_owns_is_the_target() {
    let apps = scratch("bundle-ok");
    let exe = apps.join("put.io adder.app/Contents/MacOS/putio-adder");
    assert_eq!(
        super::bundle::target_for(&exe, Path::new("/elsewhere")),
        Ok(apps.join("put.io adder.app"))
    );
}

#[test]
fn an_executable_in_a_folder_the_user_owns_is_the_target() {
    let dir = scratch("target-ok");
    let exe = dir.join("putio-adder");
    fs::write(&exe, "x").unwrap();
    assert_eq!(
        super::portable::target_for(&exe, Path::new("/elsewhere")),
        Ok(exe)
    );
    let _ = fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn an_executable_in_a_folder_it_cannot_write_to_is_not_updatable() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("target-ro");
    let exe = dir.join("putio-adder");
    fs::write(&exe, "x").unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
    // (Skipped when the tests run as root, which can write anywhere.)
    if !is_writable(&dir) {
        let error = super::portable::target_for(&exe, Path::new("/elsewhere")).unwrap_err();
        assert!(error.contains("not writable"), "{error}");
    }
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn the_windows_extended_prefix_is_removed_for_comparisons_but_not_for_unc_paths() {
    let plain = |p: &str| super::portable::plain(Path::new(p));
    assert_eq!(
        plain(r"\\?\C:\Users\me\app.exe"),
        PathBuf::from(r"C:\Users\me\app.exe")
    );
    assert_eq!(
        plain(r"\\?\UNC\server\share\app.exe"),
        PathBuf::from(r"\\?\UNC\server\share\app.exe")
    );
    assert_eq!(plain("/usr/bin/app"), PathBuf::from("/usr/bin/app"));
}

#[test]
fn the_relaunched_process_gets_the_flag_and_only_the_environment_it_is_given() {
    use super::portable::{Environment, FORWARDED_ENV, forwarded_env, relaunch_command};
    let exe = Path::new("/home/me/.local/bin/putio-adder");
    let inherit = relaunch_command(exe, 77, Environment::Inherit);
    let args: Vec<String> = inherit
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args, ["--relaunch-after", "77"]);
    assert_eq!(inherit.get_envs().count(), 0, "inherited as is");
    let clean = relaunch_command(
        exe,
        77,
        Environment::Only(vec![(OsString::from("APPDATA"), OsString::from("C:\\x"))]),
    );
    assert_eq!(clean.get_envs().count(), 1);
    // Environment names match case-insensitively on Windows, and secrets never pass.
    let forwarded = forwarded_env(
        vec![
            (OsString::from("AppData"), OsString::from("a")),
            (OsString::from("PUTIO_TOKEN"), OsString::from("secret")),
            (OsString::from("PATH"), OsString::from("p")),
        ]
        .into_iter(),
        FORWARDED_ENV,
    );
    let names: Vec<_> = forwarded
        .iter()
        .map(|(n, _)| n.to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["AppData", "PATH"]);
}

// ---- preparing and applying -----------------------------------------------------------------

fn asset(name: &str, size: usize) -> UpdateAsset {
    UpdateAsset {
        name: name.to_owned(),
        url: format!("https://github.com/nicoSWD/put.io-adder/releases/download/v0.2.0/{name}"),
        size: size as u64,
    }
}

/// Run `prepare_with` for a Linux-style tarball (the portable strategy) with a canned download.
#[cfg(not(any(windows, target_os = "macos")))]
fn prepare_linux(
    exe: &Path,
    bytes: &[u8],
    declared: usize,
    progress: &dyn Fn(UpdateStage, f32),
) -> Result<Prepared, String> {
    let asset = asset("putio-adder-v0.2.0-linux-x86_64.tar.gz", declared);
    prepare_with(exe, &asset, progress, &|_url, _max, report| {
        report(bytes.len() as u64 / 2);
        report(bytes.len() as u64);
        Ok(bytes.to_vec())
    })
}

#[cfg(not(any(windows, target_os = "macos")))]
#[test]
fn preparing_downloads_unpacks_and_leaves_the_running_app_alone() {
    let dir = scratch("prepare-ok");
    let exe = dir.join("putio-adder");
    fs::write(&exe, "old").unwrap();
    let bytes = tarball(&[(ENTRY, &this_cpu_elf())]);
    let seen = std::cell::RefCell::new(Vec::new());
    let prepared = prepare_linux(&exe, &bytes, bytes.len(), &|stage, fraction| {
        seen.borrow_mut().push((stage, fraction));
    })
    .unwrap();
    assert_eq!(
        fs::read_to_string(&exe).unwrap(),
        "old",
        "nothing is replaced yet"
    );
    assert_eq!(
        prepared.new,
        dir.join(STAGING_NAME).join("new").join(BINARY_NAME)
    );
    assert!(prepared.new.is_file());
    let seen = seen.into_inner();
    assert_eq!(seen.first().map(|s| s.0), Some(UpdateStage::Downloading));
    assert_eq!(seen.last().map(|s| s.0), Some(UpdateStage::Unpacking));
    assert!(
        seen.windows(2).all(|pair| pair[0].1 <= pair[1].1),
        "the bar only moves forward: {seen:?}"
    );
    assert!(seen.iter().all(|(_, f)| (0.0..=1.0).contains(f)));
    let _ = fs::remove_dir_all(dir);
}

#[cfg(not(any(windows, target_os = "macos")))]
#[test]
fn a_cut_off_a_bad_or_foreign_download_is_refused_and_cleaned_up() {
    let dir = scratch("prepare-bad");
    let exe = dir.join("putio-adder");
    fs::write(&exe, "old").unwrap();
    let good = tarball(&[(ENTRY, &this_cpu_elf())]);
    let staging = dir.join(STAGING_NAME);
    let none = &|_: UpdateStage, _: f32| {};

    let error = prepare_linux(&exe, &good, good.len() + 1, none).unwrap_err();
    assert!(error.contains("incomplete"), "{error}");
    assert!(
        !staging.exists(),
        "a failed attempt leaves no staging folder"
    );

    let junk = b"definitely not an archive".to_vec();
    assert!(prepare_linux(&exe, &junk, junk.len(), none).is_err());
    let wrong_cpu = tarball(&[(ENTRY, &elf_bytes(0x28))]);
    let error = prepare_linux(&exe, &wrong_cpu, wrong_cpu.len(), none).unwrap_err();
    assert!(error.contains("different kind of processor"), "{error}");
    let script = tarball(&[(ENTRY, b"#!/bin/sh\necho owned\n#...............")]);
    assert!(prepare_linux(&exe, &script, script.len(), none).is_err());
    assert!(!staging.exists());
    assert_eq!(fs::read_to_string(&exe).unwrap(), "old");

    let failing = prepare_with(
        &exe,
        &asset("putio-adder-v0.2.0-linux-x86_64.tar.gz", 10),
        none,
        &|_, _, _| Err("Download failed: HTTP 404.".to_owned()),
    );
    assert_eq!(failing.unwrap_err(), "Download failed: HTTP 404.");
    assert!(!staging.exists());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn applying_swaps_the_new_version_in_and_keeps_the_old_one_in_staging() {
    let dir = scratch("apply");
    let target = dir.join("putio-adder");
    let staging = dir.join(STAGING_NAME);
    fs::create_dir_all(staging.join("new")).unwrap();
    let new = staging.join("new").join("putio-adder");
    fs::write(&target, "old").unwrap();
    fs::write(&new, "new").unwrap();
    let prepared = Prepared {
        target: target.clone(),
        staging: staging.clone(),
        new,
    };
    apply(&prepared).unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    assert_eq!(
        fs::read_to_string(staging.join("old").join("putio-adder")).unwrap(),
        "old"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn preparing_never_clears_the_staging_folder_that_holds_the_only_copy() {
    let dir = scratch("only-copy");
    let target = dir.join("putio-adder"); // missing: a swap lost it
    let staging = dir.join(STAGING_NAME);
    fs::create_dir_all(staging.join("old")).unwrap();
    fs::write(staging.join("old").join("putio-adder"), "the only copy").unwrap();
    let error = prepare_with(&target, &asset("x.tar.gz", 1), &|_, _| {}, &|_, _, _| {
        Ok(vec![0])
    })
    .unwrap_err();
    assert!(error.contains("previous version"), "{error}");
    assert!(staging.join("old").join("putio-adder").exists());
    let _ = fs::remove_dir_all(dir);
}

// ---- swapping -------------------------------------------------------------------------------

use std::io;

/// A `current` app holding "old" and a `new` one holding "new", under a scratch folder.
fn swap_fixture(name: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let root = scratch(name);
    let (current, new) = (root.join("app"), root.join("staging-new"));
    let old = root.join("staging").join("old").join("app");
    fs::write(&current, "old").unwrap();
    fs::write(&new, "new").unwrap();
    fs::create_dir_all(old.parent().unwrap()).unwrap();
    (root, current, new, old)
}

#[test]
fn a_swap_installs_the_new_and_keeps_the_old() {
    let (root, current, new, old) = swap_fixture("swap-ok");
    swap_paths(&current, &new, &old, &|from, to| fs::rename(from, to)).unwrap();
    assert_eq!(fs::read_to_string(&current).unwrap(), "new");
    assert_eq!(fs::read_to_string(&old).unwrap(), "old");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_swap_that_cannot_start_changes_nothing() {
    let (root, current, new, old) = swap_fixture("swap-first");
    let refuse = |_: &Path, _: &Path| Err(io::Error::other("busy"));
    let error = swap_paths(&current, &new, &old, &refuse).unwrap_err();
    assert!(error.contains("move the current app aside"), "{error}");
    assert_eq!(fs::read_to_string(&current).unwrap(), "old");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_failed_second_move_puts_the_old_one_back() {
    let (root, current, new, old) = swap_fixture("swap-second");
    fs::remove_file(&new).unwrap(); // only the move into place fails
    let error = swap_paths(&current, &new, &old, &|from, to| fs::rename(from, to)).unwrap_err();
    assert!(
        error.starts_with("Could not install the new app"),
        "{error}"
    );
    assert!(!error.contains("Putting the old one back"), "{error}");
    assert_eq!(fs::read_to_string(&current).unwrap(), "old");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_failed_put_back_says_where_the_old_copy_is() {
    let (root, current, new, old) = swap_fixture("swap-both");
    let rename = |from: &Path, to: &Path| {
        if from == new {
            Err(io::Error::other("no space"))
        } else if to == current && from == old {
            Err(io::Error::other("denied"))
        } else {
            fs::rename(from, to)
        }
    };
    let error = swap_paths(&current, &new, &old, &rename).unwrap_err();
    assert!(
        error.contains("no space") && error.contains("denied"),
        "{error}"
    );
    assert!(error.contains(&old.display().to_string()), "{error}");
    assert!(error.contains(&current.display().to_string()), "{error}");
    assert!(!current.exists());
    assert_eq!(fs::read_to_string(&old).unwrap(), "old");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_only_copy_in_the_staging_folder_is_never_cleared() {
    let (root, current, _new, old) = swap_fixture("keep-only");
    let staging = root.join("staging");
    assert!(
        keep_only_copy(&staging, &current).is_ok(),
        "healthy: staging is disposable"
    );
    fs::rename(&current, &old).unwrap();
    assert!(
        keep_only_copy(&staging, &current)
            .unwrap_err()
            .contains("previous version")
    );
    assert!(
        keep_only_copy(&root.join("nothing"), &current).is_ok(),
        "no staging folder: nothing to protect"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_read_only_or_missing_folder_is_not_writable_and_the_probe_leaves_no_trace() {
    let root = scratch("writable");
    assert!(is_writable(&root));
    assert!(!is_writable(&root.join("missing")));
    let bystander = root.join(".putio-adder-write-test-keep");
    fs::create_dir(&bystander).unwrap();
    assert!(is_writable(&root));
    assert!(
        bystander.is_dir(),
        "a folder that was already there is never touched"
    );
    assert_eq!(
        fs::read_dir(&root).unwrap().count(),
        1,
        "only the bystander remains"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_progress_bar_covers_the_download_then_the_unpacking() {
    assert_eq!(downloading(0, 100), DOWNLOAD_START);
    assert_eq!(downloading(100, 100), DOWNLOAD_END);
    assert_eq!(downloading(500, 100), DOWNLOAD_END, "clamped");
    assert_eq!(downloading(5, 0), DOWNLOAD_START);
    assert!(downloading(50, 100) > DOWNLOAD_START && downloading(50, 100) < DOWNLOAD_END);
}

// ---- the contract with the release pipeline ---------------------------------------------------

/// The in-app updater only installs files with exactly these names and layouts. The packaging
/// scripts and the release workflow produce them; if either drifts, updates would silently stop being
/// offered as installable. (The scripts are plain text next to the source: read them here.)
#[test]
fn the_packaging_scripts_produce_the_files_the_updater_looks_for() {
    let linux = include_str!("../../../../tools/package-linux.sh");
    assert!(
        linux.contains(r#"name="putio-adder-v$version-linux-$arch""#),
        "tarball name"
    );
    assert!(
        linux.contains(r#"install -m 755 "$binary" "$stage/putio-adder""#),
        "binary inside the folder"
    );
    assert!(
        linux.contains(r#"-C dist -cf - "$name""#),
        "the folder is the top of the tarball"
    );
    assert!(linux.contains("ARCH: x86_64 | aarch64"));

    let windows = include_str!("../../../../tools/package-windows.sh");
    assert!(
        windows.contains(r#"name="putio-adder-v$version-windows-$arch""#),
        "zip name"
    );
    assert!(
        windows.contains(r#"cp "$exe" "$stage/putio-adder.exe""#),
        "exe at the zip root"
    );
    assert!(windows.contains("ARCH: x64 | arm64"));

    let workflow = include_str!("../../../../.github/workflows/release.yml");
    assert!(
        workflow.contains(r#""dist/putio-adder-v$VERSION-macos-${{ matrix.arch }}.zip""#),
        "macOS zip name"
    );
    assert!(
        workflow.contains(r#"ditto -c -k --keepParent "dist/$TARGET/put.io adder.app""#),
        "bundle at the zip top"
    );
    for arch in ["arm64", "x64"] {
        assert!(
            workflow.contains(&format!("arch: {arch}")),
            "macOS arch {arch} in the matrix"
        );
    }
    // The names the app computes for each platform are the ones those scripts write.
    assert_eq!(
        asset_name_for("v1.2.3", "linux", "x86_64").as_deref(),
        Some("putio-adder-v1.2.3-linux-x86_64.tar.gz")
    );
    assert_eq!(
        asset_name_for("v1.2.3", "windows", "arm64").as_deref(),
        None,
        "Rust says aarch64, the file says arm64"
    );
    assert_eq!(
        asset_name_for("v1.2.3", "windows", "aarch64").as_deref(),
        Some("putio-adder-v1.2.3-windows-arm64.zip")
    );
}

/// The bundle the macOS script builds is what `check_bundle` accepts.
#[test]
fn the_macos_bundle_script_builds_what_the_updater_accepts() {
    let script = include_str!("../../../../tools/bundle-macos.sh");
    assert!(script.contains("<key>CFBundleIdentifier</key><string>es.nico.putio-adder</string>"));
    assert!(script.contains(r#"cp "$binary" "$app/Contents/MacOS/putio-adder""#));
    assert!(script.contains(r#"app="target/put.io adder.app""#));
}

/// The whole macOS path with a real zip made by `ditto`: prepare (unpack and check), then apply (swap).
#[cfg(target_os = "macos")]
#[test]
fn a_macos_update_is_prepared_and_swapped_in_end_to_end() {
    let root = scratch("macos-e2e");
    let installed = fake_app(&root.join("Applications"), "old-version");
    let source = fake_app(&root.join("release"), "new-version");
    let zip = root.join("release.zip");
    assert!(
        std::process::Command::new("/usr/bin/ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(&source)
            .arg(&zip)
            .status()
            .unwrap()
            .success()
    );
    let bytes = fs::read(&zip).unwrap();
    let seen = std::cell::RefCell::new(Vec::new());
    let prepared = prepare_with(
        &installed,
        &asset("putio-adder-v0.2.0-macos-arm64.zip", bytes.len()),
        &|stage, fraction| seen.borrow_mut().push((stage, fraction)),
        &|_url, _max, report| {
            report(bytes.len() as u64);
            Ok(bytes.clone())
        },
    )
    .unwrap();
    let exe = |app: &Path| fs::read_to_string(app.join("Contents/MacOS/putio-adder")).unwrap();
    assert_eq!(
        exe(&installed),
        "old-version",
        "nothing replaced until the user restarts"
    );
    assert!(
        seen.borrow()
            .iter()
            .any(|(stage, _)| *stage == UpdateStage::Unpacking)
    );

    apply(&prepared).unwrap();
    assert_eq!(exe(&installed), "new-version");
    let old = root
        .join("Applications")
        .join(STAGING_NAME)
        .join("old")
        .join(BUNDLE_NAME);
    assert_eq!(
        exe(&old),
        "old-version",
        "the previous version waits in staging"
    );
    let _ = fs::remove_dir_all(root);
}
