//! Update check logic: which release is on offer, and which of its files this computer can install.
//!
//! The pieces that decide *whether an update may be installed in place* live here, as pure functions:
//! the release must come from the project's own account, and the file must be the one this build
//! expects, at the project's own download address, with a sane size. What is **not** checked is the
//! content of the file: there is no checksum, tag-signature or code-signature check (a deliberate
//! choice, see `docs/updates.md`). Transport limits (HTTPS, GitHub hosts only, size caps, safe
//! unpacking) are enforced by the layers that fetch and unpack.

use putio_api::Release;
use semver::Version;

/// Accounts GitHub may record as a release's author: the release workflow's bot, and the maintainer
/// for a release they published by hand.
const RELEASE_AUTHORS: [&str; 2] = ["github-actions[bot]", "nicoSWD"];
/// Where this project's release files live.
const DOWNLOAD_PREFIX: &str = "https://github.com/nicoSWD/putio-adder-rs/releases/download/";
/// The largest file the updater will download (the real ones are about 10 MB).
pub const MAX_ASSET_BYTES: u64 = 128 * 1024 * 1024;

/// A release file this computer can install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateAsset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UpdateStatus {
    #[default]
    Unknown,
    Checking,
    UpToDate,
    Available {
        version: String,
        /// The release page.
        url: String,
        /// This computer's download, when the release has one that may be installed in place.
        asset: Option<UpdateAsset>,
    },
}

/// Parse `v1.2.3` or `1.2.3`.
fn parse(tag: &str) -> Option<Version> {
    Version::parse(tag.trim().trim_start_matches('v')).ok()
}

/// Whether `candidate` is a strictly newer version than `current` (semantic-version comparison, not
/// string inequality: `0.10.0` is newer than `0.9.0`). Unparsable versions are never "newer".
#[must_use]
pub fn is_newer(current: &str, candidate: &str) -> bool {
    match (parse(current), parse(candidate)) {
        (Some(current), Some(candidate)) => candidate > current,
        _ => false,
    }
}

/// Decide what a fetched release means for the running version. Drafts and prereleases are ignored
/// (this build only follows the stable channel). `asset_name` says which file this computer
/// installs for a given tag, or `None` when it has none.
#[must_use]
pub fn evaluate(
    current: &str,
    release: &Release,
    asset_name: &dyn Fn(&str) -> Option<String>,
) -> UpdateStatus {
    if release.draft || release.prerelease || !is_newer(current, &release.tag_name) {
        return UpdateStatus::UpToDate;
    }
    UpdateStatus::Available {
        version: release.tag_name.trim().trim_start_matches('v').to_owned(),
        url: release.html_url.clone(),
        asset: pick_asset(release, asset_name),
    }
}

/// The file to install, if the release is the project's own and has exactly the file this build
/// expects. Anything doubtful gives `None`: the release is then only offered for manual download.
fn pick_asset(
    release: &Release,
    asset_name: &dyn Fn(&str) -> Option<String>,
) -> Option<UpdateAsset> {
    let tag = release.tag_name.as_str();
    let tag_is_plain = !tag.is_empty()
        && tag.len() <= 64
        && tag
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'));
    let author = release.author.as_ref()?;
    if !tag_is_plain || !RELEASE_AUTHORS.contains(&author.login.as_str()) {
        return None;
    }
    let wanted = asset_name(tag)?;
    let asset = release.assets.iter().find(|asset| asset.name == wanted)?;
    let expected_url = format!("{DOWNLOAD_PREFIX}{tag}/{wanted}");
    let genuine = asset.browser_download_url == expected_url
        && asset.size > 0
        && asset.size <= MAX_ASSET_BYTES;
    genuine.then_some(UpdateAsset {
        name: wanted,
        url: expected_url,
        size: asset.size,
    })
}

#[cfg(test)]
mod tests {
    use putio_api::{ReleaseAsset, ReleaseAuthor};

    use super::*;

    fn release(tag: &str, prerelease: bool, draft: bool) -> Release {
        Release {
            tag_name: tag.into(),
            html_url: format!("https://github.com/x/y/releases/tag/{tag}"),
            prerelease,
            draft,
            author: None,
            assets: Vec::new(),
        }
    }

    #[allow(clippy::unnecessary_wraps)] // the shape the real function has
    fn namer(tag: &str) -> Option<String> {
        Some(format!("putio-adder-{tag}-test.zip"))
    }

    fn installable(tag: &str) -> Release {
        let name = format!("putio-adder-{tag}-test.zip");
        Release {
            author: Some(ReleaseAuthor {
                login: "github-actions[bot]".into(),
            }),
            assets: vec![ReleaseAsset {
                browser_download_url: format!("{DOWNLOAD_PREFIX}{tag}/{name}"),
                name,
                size: 10_000_000,
            }],
            ..release(tag, false, false)
        }
    }

    #[test]
    fn semver_not_string_comparison() {
        assert!(is_newer("0.9.0", "0.10.0"));
        assert!(is_newer("1.0.0", "v1.0.1"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("1.2.0", "1.1.9"));
        assert!(is_newer("1.0.0-rc.1", "1.0.0"));
        assert!(!is_newer("1.0.0", "1.0.0-rc.2"));
    }

    #[test]
    fn unparsable_is_never_newer() {
        assert!(!is_newer("1.0.0", "latest"));
        assert!(!is_newer("dev", "1.0.0"));
        assert!(!is_newer("", ""));
    }

    #[test]
    fn evaluate_reports_only_real_newer_releases() {
        assert_eq!(
            evaluate("0.1.0", &release("v0.2.0", false, false), &namer),
            UpdateStatus::Available {
                version: "0.2.0".into(),
                url: "https://github.com/x/y/releases/tag/v0.2.0".into(),
                asset: None,
            }
        );
        for (current, release) in [
            ("0.2.0", release("v0.2.0", false, false)),
            ("0.1.0", release("v0.2.0", true, false)),
            ("0.1.0", release("v0.2.0", false, true)),
        ] {
            assert_eq!(evaluate(current, &release, &namer), UpdateStatus::UpToDate);
        }
    }

    #[test]
    fn the_projects_own_release_with_the_expected_file_is_installable() {
        let UpdateStatus::Available { asset, .. } =
            evaluate("0.1.0", &installable("v0.2.0"), &namer)
        else {
            panic!("expected an update");
        };
        let asset = asset.expect("installable");
        assert_eq!(asset.name, "putio-adder-v0.2.0-test.zip");
        assert_eq!(
            asset.url,
            "https://github.com/nicoSWD/putio-adder-rs/releases/download/v0.2.0/putio-adder-v0.2.0-test.zip"
        );
        assert_eq!(asset.size, 10_000_000);
        // A maintainer who published it by hand is accepted too.
        let mut by_hand = installable("v0.2.0");
        by_hand.author = Some(ReleaseAuthor {
            login: "nicoSWD".into(),
        });
        assert!(matches!(
            evaluate("0.1.0", &by_hand, &namer),
            UpdateStatus::Available { asset: Some(_), .. }
        ));
    }

    /// Only the file this build expects, from the project's account, at the project's address, with a
    /// plausible size, may be installed. Anything else leaves the release for manual download.
    #[test]
    fn a_release_that_does_not_check_out_is_not_installable() {
        let manual = |release: &Release| {
            matches!(
                evaluate("0.1.0", release, &namer),
                UpdateStatus::Available { asset: None, .. }
            )
        };
        let mut r = installable("v0.2.0");
        r.author = None;
        assert!(manual(&r), "no author");
        let mut r = installable("v0.2.0");
        r.author = Some(ReleaseAuthor {
            login: "someone-else".into(),
        });
        assert!(manual(&r), "foreign author");
        let mut r = installable("v0.2.0");
        r.author = Some(ReleaseAuthor {
            login: "NICOSWD".into(),
        });
        assert!(manual(&r), "logins are compared exactly");
        let mut r = installable("v0.2.0");
        r.assets[0].name = "putio-adder-v0.2.0-other.zip".into();
        assert!(manual(&r), "wrong file name");
        for url in [
            "https://evil.example/nicoSWD/putio-adder-rs/releases/download/v0.2.0/putio-adder-v0.2.0-test.zip",
            "http://github.com/nicoSWD/putio-adder-rs/releases/download/v0.2.0/putio-adder-v0.2.0-test.zip",
            "https://github.com/nicoSWD/putio-adder-rs-evil/releases/download/v0.2.0/putio-adder-v0.2.0-test.zip",
            "https://github.com/nicoSWD/putio-adder-rs/releases/download/v0.1.0/putio-adder-v0.2.0-test.zip",
            "https://github.com/nicoSWD/putio-adder-rs/releases/download/v0.2.0/putio-adder-v0.2.0-test.zip?x=1",
            "",
        ] {
            let mut r = installable("v0.2.0");
            r.assets[0].browser_download_url = url.into();
            assert!(manual(&r), "{url}");
        }
        for size in [0, MAX_ASSET_BYTES + 1, u64::MAX] {
            let mut r = installable("v0.2.0");
            r.assets[0].size = size;
            assert!(manual(&r), "size {size}");
        }
        // A computer with no file for it, or a release with no files at all.
        assert!(matches!(
            evaluate("0.1.0", &installable("v0.2.0"), &|_: &str| None),
            UpdateStatus::Available { asset: None, .. }
        ));
        let mut r = installable("v0.2.0");
        r.assets.clear();
        assert!(manual(&r));
    }

    #[test]
    fn a_tag_with_odd_characters_never_builds_a_download_address() {
        for tag in [
            "v0.2.0/../x",
            "v0.2.0?x=1",
            "v0.2.0 ",
            "v0.2.0\n",
            "v0.2.0#",
        ] {
            let mut r = installable("v0.2.0");
            r.tag_name = tag.into();
            // The tag is not a version either, but if it were, the address must not be built from it.
            assert_eq!(pick_asset(&r, &namer), None, "{tag:?}");
        }
    }
}
