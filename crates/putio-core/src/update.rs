//! Update check logic. The app only *tells* the user about a newer release; it never downloads or
//! installs anything by itself.

use putio_api::Release;
use semver::Version;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UpdateStatus {
    #[default]
    Unknown,
    Checking,
    UpToDate,
    Available {
        version: String,
        url: String,
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

/// Decide what a fetched release means for the running version. Drafts and prereleases are ignored.
#[must_use]
pub fn evaluate(current: &str, release: &Release) -> UpdateStatus {
    if release.draft || release.prerelease || !is_newer(current, &release.tag_name) {
        return UpdateStatus::UpToDate;
    }
    UpdateStatus::Available {
        version: release.tag_name.trim().trim_start_matches('v').to_owned(),
        url: release.html_url.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, prerelease: bool, draft: bool) -> Release {
        Release {
            tag_name: tag.into(),
            html_url: format!("https://github.com/x/y/releases/tag/{tag}"),
            prerelease,
            draft,
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
            evaluate("0.1.0", &release("v0.2.0", false, false)),
            UpdateStatus::Available {
                version: "0.2.0".into(),
                url: "https://github.com/x/y/releases/tag/v0.2.0".into()
            }
        );
        assert_eq!(
            evaluate("0.2.0", &release("v0.2.0", false, false)),
            UpdateStatus::UpToDate
        );
        assert_eq!(
            evaluate("0.1.0", &release("v0.2.0", true, false)),
            UpdateStatus::UpToDate
        );
        assert_eq!(
            evaluate("0.1.0", &release("v0.2.0", false, true)),
            UpdateStatus::UpToDate
        );
    }
}
