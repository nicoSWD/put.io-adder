//! Response models.
//!
//! Every model is deliberately tolerant: unknown fields are ignored, missing or `null` scalars become
//! their default, and unknown enum values map to `Unknown` instead of failing the whole response.
//! put.io adds fields and enum values over time and the spec marks nothing as nullable even where
//! the API sends `null`.

use serde::{Deserialize, Deserializer};

/// `null` and missing both become `T::default()`.
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

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
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
}

impl Transfer {
    /// Progress clamped to 0..=100.
    #[must_use]
    pub fn percent(&self) -> u8 {
        u8::try_from(self.percent_done.clamp(0, 100)).unwrap_or(0)
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
}

/// `GET /oauth2/oob/code`
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DeviceCode {
    /// Short code the user types at <https://put.io/link>.
    pub code: String,
}

/// What an upload turned into. For a `.torrent` put.io starts a transfer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadOutcome {
    Transfer(Transfer),
    File(File),
}

/// The fields of a GitHub release the update check needs.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub html_url: String,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub draft: bool,
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
