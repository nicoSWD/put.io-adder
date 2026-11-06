//! Response models.
//!
//! Every model is deliberately tolerant: unknown fields are ignored, missing or `null` scalars become
//! their default, and unknown enum values map to `Unknown` instead of failing the whole response.
//! put.io adds fields and enum values over time and the spec marks nothing as nullable even where
//! the API sends `null`.

use serde::{Deserialize, Deserializer};

/// `null` and missing both become `T::default()`.
/// A field that is kept when it has the expected type and dropped (as `None`) when it does not, so an
/// odd value in a detail never makes the whole transfer list unreadable.
fn lenient<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| serde_json::from_value(value).ok()))
}

/// Like [`lenient`], but a missing, `null` or wrongly typed value becomes `T::default()`.
fn lenient_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    lenient(deserializer).map(Option::unwrap_or_default)
}

fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransferStatus {
    InQueue,
    Waiting,
    PreparingDownload,
    Downloading,
    Completing,
    Seeding,
    Completed,
    Error,
    #[default]
    #[serde(other)]
    Unknown,
}

impl TransferStatus {
    /// Still on its way to being usable on put.io.
    #[must_use]
    pub fn is_pending(self) -> bool {
        matches!(
            self,
            Self::InQueue
                | Self::Waiting
                | Self::PreparingDownload
                | Self::Downloading
                | Self::Completing
        )
    }

    /// Downloaded to put.io (seeding counts: the files are there).
    #[must_use]
    pub fn is_finished(self) -> bool {
        matches!(self, Self::Seeding | Self::Completed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransferKind {
    Torrent,
    Url,
    Playlist,
    LiveStream,
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct Transfer {
    pub id: i64,
    #[serde(default, deserialize_with = "null_default")]
    pub name: String,
    #[serde(default, deserialize_with = "null_default")]
    pub status: TransferStatus,
    #[serde(default, rename = "type", deserialize_with = "null_default")]
    pub kind: TransferKind,
    /// 0..=100 as sent; use [`Transfer::percent`] for a clamped value.
    #[serde(default, deserialize_with = "null_default")]
    pub percent_done: i64,
    #[serde(default, deserialize_with = "null_default")]
    pub down_speed: i64,
    #[serde(default, deserialize_with = "null_default")]
    pub up_speed: i64,
    /// Seconds; `None` when put.io has no estimate.
    #[serde(default)]
    pub estimated_time: Option<i64>,
    #[serde(default, deserialize_with = "null_default")]
    pub size: i64,
    /// The file or folder the finished transfer produced; `None` while it is still running.
    #[serde(default)]
    pub file_id: Option<i64>,
    #[serde(default)]
    pub error_message: Option<String>,
    /// What the transfer was added from: a magnet link or a URL. Untrusted text.
    #[serde(default)]
    pub source: Option<String>,

    // Details. All optional and read leniently: put.io leaves out what does not apply to a transfer.
    /// Connected peers.
    #[serde(default, deserialize_with = "lenient")]
    pub peers: Option<i64>,
    /// How much of the content the swarm has, in percent.
    #[serde(default, deserialize_with = "lenient")]
    pub availability: Option<i64>,
    #[serde(default, deserialize_with = "lenient")]
    pub downloaded: Option<i64>,
    #[serde(default, deserialize_with = "lenient")]
    pub uploaded: Option<i64>,
    /// Uploaded divided by downloaded.
    #[serde(default, deserialize_with = "lenient")]
    pub current_ratio: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub seconds_seeding: Option<i64>,
    #[serde(default, deserialize_with = "lenient")]
    pub is_private: Option<bool>,
    /// What the tracker last said. Untrusted text.
    #[serde(default, deserialize_with = "lenient")]
    pub tracker_message: Option<String>,
    /// ISO 8601 timestamps as sent. Untrusted text.
    #[serde(default, deserialize_with = "lenient")]
    pub created_at: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub finished_at: Option<String>,
    /// The folder the transfer saves into.
    #[serde(default, deserialize_with = "lenient")]
    pub save_parent_id: Option<i64>,
    /// The RSS feed that added the transfer, when one did.
    #[serde(default, deserialize_with = "lenient")]
    pub subscription_id: Option<i64>,
}

impl Transfer {
    /// Progress clamped to 0..=100.
    #[must_use]
    pub fn percent(&self) -> u8 {
        u8::try_from(self.percent_done.clamp(0, 100)).unwrap_or(0)
    }
}

/// A file put.io could not put into a ZIP.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct MissingFile {
    #[serde(default, deserialize_with = "lenient")]
    pub name: Option<String>,
}

/// Progress of a ZIP that put.io builds from a folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ZipState {
    New,
    Processing,
    Done,
    Error,
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct ZipStatus {
    #[serde(default, deserialize_with = "null_default")]
    pub zip_status: ZipState,
    /// Where to download the ZIP. Only present once it is done. Untrusted text; check it before use.
    #[serde(default, deserialize_with = "lenient")]
    pub url: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub size: Option<i64>,
    /// Files put.io left out of the ZIP. Only the number matters here.
    #[serde(default, deserialize_with = "lenient")]
    pub missing_files: Option<Vec<MissingFile>>,
}

impl ZipStatus {
    /// How many files are not in the ZIP.
    #[must_use]
    pub fn missing_count(&self) -> usize {
        self.missing_files.as_ref().map_or(0, Vec::len)
    }
}

/// Disk quota. The 2.8.14 spec no longer lists it, but the live API may still send it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub struct Disk {
    #[serde(default, deserialize_with = "null_default")]
    pub size: i64,
    #[serde(default, deserialize_with = "null_default")]
    pub used: i64,
    #[serde(default, deserialize_with = "null_default")]
    pub avail: i64,
}

impl Disk {
    /// Fraction used in `0.0..=1.0`, or `None` when the quota is unknown or zero.
    #[must_use]
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)] // result is clamped to 0..=1
    pub fn used_fraction(&self) -> Option<f32> {
        (self.size > 0).then(|| (self.used as f64 / self.size as f64).clamp(0.0, 1.0) as f32)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct Account {
    #[serde(default, deserialize_with = "null_default")]
    pub user_id: i64,
    #[serde(default, deserialize_with = "null_default")]
    pub username: String,
    #[serde(default, deserialize_with = "null_default")]
    pub mail: String,
    #[serde(default, deserialize_with = "null_default")]
    pub monthly_bandwidth_usage: i64,
    #[serde(default)]
    pub disk: Option<Disk>,
}

/// A friend: someone files can be shared with. Text fields are untrusted server text.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct Friend {
    pub id: i64,
    #[serde(default, deserialize_with = "lenient_default")]
    pub name: String,
}

/// One person a file is shared with. `share_id` is what ends the share.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct Share {
    pub share_id: i64,
    #[serde(default, deserialize_with = "lenient_default")]
    pub user_name: String,
}

/// One entry of the list of files the account shares.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct SharedFile {
    pub id: i64,
    #[serde(default, deserialize_with = "lenient_default")]
    pub name: String,
}

/// Who a file is shared with when it is shared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareWith {
    /// Every friend, now and later.
    Everyone,
    /// These friends, by name.
    Friends(Vec<String>),
}

impl ShareWith {
    /// The value of the `friends` form field, or `None` when there is nobody to share with (or a name
    /// that could not be told apart from a list).
    #[must_use]
    pub fn as_field(&self) -> Option<String> {
        match self {
            Self::Everyone => Some("everyone".into()),
            Self::Friends(names) => {
                let clean = |name: &String| !name.trim().is_empty() && !name.contains(',');
                (!names.is_empty() && names.iter().all(clean)).then(|| names.join(","))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FileType {
    Folder,
    File,
    Audio,
    Video,
    Image,
    Archive,
    Pdf,
    Text,
    Swf,
    #[default]
    #[serde(other)]
    Unknown,
}

impl FileType {
    /// The value of the `file_type` query parameter.
    #[must_use]
    pub fn as_query(self) -> &'static str {
        match self {
            Self::Folder => "FOLDER",
            Self::Audio => "AUDIO",
            Self::Video => "VIDEO",
            Self::Image => "IMAGE",
            Self::Archive => "ARCHIVE",
            Self::Pdf => "PDF",
            Self::Text => "TEXT",
            Self::Swf => "SWF",
            Self::File | Self::Unknown => "FILE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct File {
    pub id: i64,
    #[serde(default, deserialize_with = "null_default")]
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<i64>,
    #[serde(default, deserialize_with = "null_default")]
    pub file_type: FileType,
    #[serde(default, deserialize_with = "null_default")]
    pub content_type: String,
    #[serde(default, deserialize_with = "null_default")]
    pub size: i64,
    #[serde(default)]
    pub screenshot: Option<String>,
    /// ISO 8601 as sent, without a time zone. Untrusted text.
    #[serde(default, deserialize_with = "lenient")]
    pub created_at: Option<String>,
}

/// `GET /oauth2/oob/code`
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DeviceCode {
    /// Short code the user types at <https://put.io/link>.
    pub code: String,
}

/// What an upload turned into. For a `.torrent` put.io starts a transfer.
#[derive(Debug, Clone, PartialEq)]
pub enum UploadOutcome {
    Transfer(Box<Transfer>),
    File(File),
}

/// The fields of a GitHub release the update check needs. Everything but the tag and the page is
/// optional or defaulted: GitHub adds fields, and a release without assets is simply not installable.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub html_url: String,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub draft: bool,
    /// Who published it (the account GitHub records).
    #[serde(default, deserialize_with = "lenient")]
    pub author: Option<ReleaseAuthor>,
    #[serde(default, deserialize_with = "null_default")]
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ReleaseAuthor {
    #[serde(default, deserialize_with = "null_default")]
    pub login: String,
}

/// A file attached to a release. Untrusted text: names and addresses are checked before use.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct ReleaseAsset {
    #[serde(default, deserialize_with = "null_default")]
    pub name: String,
    #[serde(default, deserialize_with = "null_default")]
    pub browser_download_url: String,
    #[serde(default, deserialize_with = "null_default")]
    pub size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_tolerates_nulls_unknowns_and_extra_fields() {
        let transfer: Transfer = serde_json::from_str(
            r#"{"id": 5, "name": null, "status": "SOMETHING_NEW", "type": "TELEPORT", "percent_done": null,
                "estimated_time": null, "file_id": null, "brand_new_field": [1,2,3]}"#,
        )
        .unwrap();
        assert_eq!(transfer.id, 5);
        assert_eq!(transfer.name, "");
        assert_eq!(transfer.status, TransferStatus::Unknown);
        assert_eq!(transfer.kind, TransferKind::Unknown);
        assert_eq!(transfer.percent(), 0);
        assert_eq!(transfer.estimated_time, None);
        assert_eq!(transfer.file_id, None);
    }

    #[test]
    fn percent_is_clamped() {
        let mut transfer: Transfer = serde_json::from_str(r#"{"id": 1}"#).unwrap();
        transfer.percent_done = 250;
        assert_eq!(transfer.percent(), 100);
        transfer.percent_done = -4;
        assert_eq!(transfer.percent(), 0);
    }

    #[test]
    fn status_classification() {
        use TransferStatus::*;
        for status in [InQueue, Waiting, PreparingDownload, Downloading, Completing] {
            assert!(status.is_pending() && !status.is_finished(), "{status:?}");
        }
        for status in [Seeding, Completed] {
            assert!(status.is_finished() && !status.is_pending(), "{status:?}");
        }
        for status in [Error, Unknown] {
            assert!(!status.is_finished() && !status.is_pending(), "{status:?}");
        }
    }

    #[test]
    fn account_without_disk_is_fine() {
        let account: Account = serde_json::from_str(
            r#"{"user_id":1,"username":"cenk","mail":"a@b.c","monthly_bandwidth_usage":9}"#,
        )
        .unwrap();
        assert_eq!(account.disk, None);
        assert_eq!(account.username, "cenk");
    }

    #[test]
    fn disk_fraction_handles_zero_and_overflow() {
        assert_eq!(
            Disk {
                size: 0,
                used: 5,
                avail: 0
            }
            .used_fraction(),
            None
        );
        assert_eq!(
            Disk {
                size: 100,
                used: 25,
                avail: 75
            }
            .used_fraction(),
            Some(0.25)
        );
        assert_eq!(
            Disk {
                size: 100,
                used: 500,
                avail: 0
            }
            .used_fraction(),
            Some(1.0)
        );
        assert_eq!(
            Disk {
                size: 100,
                used: -5,
                avail: 0
            }
            .used_fraction(),
            Some(0.0)
        );
    }

    #[test]
    fn file_type_query_values_round_trip_through_serde() {
        for kind in [
            FileType::Folder,
            FileType::File,
            FileType::Audio,
            FileType::Video,
            FileType::Image,
            FileType::Archive,
            FileType::Pdf,
            FileType::Text,
            FileType::Swf,
        ] {
            let parsed: FileType =
                serde_json::from_str(&format!("\"{}\"", kind.as_query())).unwrap();
            assert_eq!(parsed, kind);
        }
    }
}

/// An RSS feed that put.io watches: new items in it become transfers, saved in `parent_dir_id`.
///
/// Every text field is **untrusted** server text. Nothing here may fail the whole feed list: a value of an
/// unexpected type is dropped (or replaced by the default) on its own.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct RssFeed {
    pub id: i64,
    #[serde(default, deserialize_with = "lenient_default")]
    pub title: String,
    /// The address of the feed itself.
    #[serde(default, deserialize_with = "lenient_default")]
    pub rss_source_url: String,
    /// The folder new items are saved in (`0` is the top level).
    #[serde(default, deserialize_with = "lenient")]
    pub parent_dir_id: Option<i64>,
    #[serde(default, deserialize_with = "lenient_default")]
    pub delete_old_files: bool,
    /// Only items whose title contains one of these comma-separated words are taken.
    #[serde(default, deserialize_with = "lenient")]
    pub keyword: Option<String>,
    /// Items whose title contains one of these comma-separated words are skipped.
    #[serde(default, deserialize_with = "lenient")]
    pub unwanted_keywords: Option<String>,
    #[serde(default, deserialize_with = "lenient_default")]
    pub paused: bool,
    /// Timestamps are ISO 8601 as sent, without a time zone.
    #[serde(default, deserialize_with = "lenient")]
    pub created_at: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub updated_at: Option<String>,
    /// When put.io last looked at the feed.
    #[serde(default, deserialize_with = "lenient")]
    pub last_fetch: Option<String>,
    /// Why the last look failed, when it did.
    #[serde(default, deserialize_with = "lenient")]
    pub last_error: Option<String>,
}

/// What the user chooses for a feed: the fields of `POST /rss/create` and `POST /rss/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RssFeedSettings {
    pub title: String,
    /// The address of the feed (`rss_source_url`).
    pub url: String,
    /// The folder new items are saved in (`0` is the top level).
    pub parent_dir_id: i64,
    pub delete_old_files: bool,
    /// Comma-separated words; empty for no filter.
    pub keyword: String,
    /// Comma-separated words; empty for no filter.
    pub unwanted_keywords: String,
    /// Only when creating: ignore the items already in the feed.
    pub skip_existing: bool,
}

/// Where an archive extraction stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExtractStatus {
    InQueue,
    Extracting,
    Extracted,
    /// put.io cannot open the archive without a (right) password.
    Password,
    Error,
    #[default]
    #[serde(other)]
    Unknown,
}

/// A ZIP or RAR being unpacked into its folder on put.io. Text fields are untrusted server text.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct Extraction {
    pub id: i64,
    #[serde(default, deserialize_with = "lenient_default")]
    pub name: String,
    #[serde(default, deserialize_with = "lenient_default")]
    pub status: ExtractStatus,
    /// Why it failed, when it did.
    #[serde(default, deserialize_with = "lenient")]
    pub message: Option<String>,
    /// The archive files being unpacked (more than one for a multi-part RAR).
    #[serde(default, deserialize_with = "lenient_default")]
    pub files: Vec<i64>,
}
