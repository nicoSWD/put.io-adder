//! Human-readable numbers for the transfers table and header.

use putio_api::{Transfer, TransferKind, TransferStatus};

use crate::i18n::{Locale, Msg};

const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];

/// Binary (1024-based) size with one decimal: `0 B`, `1023 B`, `1.0 KB`, `12.3 GB`.
/// Negative input is shown as `0 B`.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn bytes(value: i64) -> String {
    if value <= 0 {
        return "0 B".into();
    }
    let mut scaled = value as f64;
    let mut unit = 0;
    while scaled >= 1024.0 && unit < UNITS.len() - 1 {
        scaled /= 1024.0;
        unit += 1;
    }
    // 1023.96 KB rounds to "1024.0 KB": promote it instead.
    if unit > 0 && (scaled * 10.0).round() / 10.0 >= 1024.0 && unit < UNITS.len() - 1 {
        scaled /= 1024.0;
        unit += 1;
    }
    let unit_name = UNITS.get(unit).copied().unwrap_or("PB");
    if unit == 0 {
        format!("{value} B")
    } else {
        format!("{scaled:.1} {unit_name}")
    }
}

/// Download speed, e.g. `1.5 MB/s`.
#[must_use]
pub fn speed(bytes_per_second: i64) -> String {
    format!("{}/s", bytes(bytes_per_second))
}

/// Remaining time as `3h 05m`, `4m 07s` or `9s`.
#[must_use]
pub fn duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let (hours, minutes, secs) = (seconds / 3600, (seconds / 60) % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m {secs:02}s")
    } else {
        format!("{secs}s")
    }
}

/// The ETA column: words for finished states, a duration while downloading, a dash otherwise.
#[must_use]
pub fn eta(status: TransferStatus, estimated_seconds: Option<i64>, locale: Locale) -> String {
    match status {
        TransferStatus::Completing => Msg::EtaFinishing.text(locale),
        TransferStatus::Completed | TransferStatus::Seeding => Msg::EtaDone.text(locale),
        _ => match estimated_seconds {
            Some(seconds) if seconds >= 0 && status == TransferStatus::Downloading => {
                duration(seconds)
            }
            _ => "–".into(),
        },
    }
}

/// Longest error text shown on a row.
const MAX_ERROR_CHARS: usize = 80;

/// The second line of a transfer row: what it is doing right now.
///
/// `Downloading · 1.5 MB/s · 4m 07s left`, `Done`, `In queue`, or the error text for a failed transfer
/// (control characters removed and shortened: it comes from the server).
#[must_use]
pub fn detail(transfer: &Transfer, locale: Locale) -> String {
    match transfer.status {
        TransferStatus::Downloading => {
            let mut parts = vec![Msg::StatusDownloading.text(locale)];
            if transfer.down_speed > 0 {
                parts.push(speed(transfer.down_speed));
            }
            if let Some(seconds) = transfer.estimated_time.filter(|seconds| *seconds >= 0) {
                parts.push(
                    Msg::EtaLeft {
                        eta: duration(seconds),
                    }
                    .text(locale),
                );
            }
            parts.join(" · ")
        }
        TransferStatus::Completing => Msg::EtaFinishing.text(locale),
        TransferStatus::Completed => Msg::EtaDone.text(locale),
        TransferStatus::Seeding => Msg::StatusSeeding.text(locale),
        TransferStatus::Waiting => Msg::StatusWaiting.text(locale),
        TransferStatus::InQueue => Msg::StatusQueued.text(locale),
        TransferStatus::PreparingDownload => Msg::StatusPreparing.text(locale),
        TransferStatus::Error => match transfer
            .error_message
            .as_deref()
            .map(clean_error)
            .filter(|text| !text.is_empty())
        {
            Some(text) => text,
            None => Msg::StatusFailed.text(locale),
        },
        TransferStatus::Unknown => "–".into(),
    }
}

/// Where a detail is shown in the panel: related facts sit together under one title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailGroup {
    Transfer,
    Sharing,
    Dates,
    /// Free text from put.io or the tracker. Drawn as callouts, not as label/value rows.
    Messages,
}

/// One line of the details panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailRow {
    pub group: DetailGroup,
    pub label: Msg,
    pub value: String,
}

/// One line under the transfer's name in the panel: what it is doing right now. (A failed transfer says
/// so here; its reason is a separate message.)
#[must_use]
pub fn headline(transfer: &Transfer, locale: Locale) -> String {
    if transfer.status == TransferStatus::Error {
        Msg::StatusFailed.text(locale)
    } else {
        detail(transfer, locale)
    }
}

/// The facts about a transfer worth showing in the details panel, grouped, in display order. Anything the
/// server did not send (or sent as nonsense) is left out rather than shown as `0` or `–`. The status is not
/// a row: the panel shows it as [`headline`].
#[must_use]
pub fn details(transfer: &Transfer, locale: Locale) -> Vec<DetailRow> {
    let mut rows = Vec::new();
    let mut add = |group: DetailGroup, label: Msg, value: String| {
        rows.push(DetailRow {
            group,
            label,
            value,
        });
    };
    let transfer_group = DetailGroup::Transfer;
    let sharing = DetailGroup::Sharing;

    let kind = match transfer.kind {
        TransferKind::Torrent => Some(Msg::KindTorrent),
        TransferKind::Url => Some(Msg::KindUrl),
        TransferKind::Playlist => Some(Msg::KindPlaylist),
        TransferKind::LiveStream => Some(Msg::KindLiveStream),
        TransferKind::Unknown => None,
    };
    if let Some(kind) = kind {
        add(transfer_group, Msg::DetailType, kind.text(locale));
    }
    if transfer.size > 0 {
        add(transfer_group, Msg::ColSize, bytes(transfer.size));
    }
    if let Some(value) = transfer.downloaded.filter(|v| *v > 0) {
        add(transfer_group, Msg::DetailDownloaded, bytes(value));
    }

    if let Some(value) = transfer.uploaded.filter(|v| *v > 0) {
        add(sharing, Msg::DetailUploaded, bytes(value));
    }
    if let Some(ratio) = transfer
        .current_ratio
        .filter(|r| r.is_finite() && *r >= 0.0)
    {
        add(sharing, Msg::DetailRatio, format!("{ratio:.2}"));
    }
    if let Some(seconds) = transfer.seconds_seeding.filter(|s| *s > 0) {
        add(sharing, Msg::DetailSeedingFor, duration(seconds));
    }
    if let Some(peers) = transfer.peers.filter(|p| *p >= 0) {
        add(sharing, Msg::DetailPeers, peers.to_string());
    }
    if let Some(percent) = transfer.availability.filter(|a| (0..=100).contains(a)) {
        add(sharing, Msg::DetailAvailability, format!("{percent}%"));
    }
    if let Some(private) = transfer.is_private {
        let answer = if private {
            Msg::DetailYes
        } else {
            Msg::DetailNo
        };
        add(sharing, Msg::DetailPrivate, answer.text(locale));
    }

    for (label, stamp) in [
        (Msg::DetailAdded, &transfer.created_at),
        (Msg::DetailFinished, &transfer.finished_at),
    ] {
        if let Some(text) = stamp.as_deref().and_then(timestamp) {
            add(DetailGroup::Dates, label, text);
        }
    }

    if transfer.status == TransferStatus::Error {
        add_message(
            &mut rows,
            Msg::DetailError,
            transfer.error_message.as_deref(),
        );
    }
    add_message(
        &mut rows,
        Msg::DetailTrackerMessage,
        transfer.tracker_message.as_deref(),
    );
    rows
}

fn add_message(rows: &mut Vec<DetailRow>, label: Msg, text: Option<&str>) {
    if let Some(text) = text.map(clean_error).filter(|t| !t.is_empty()) {
        rows.push(DetailRow {
            group: DetailGroup::Messages,
            label,
            value: text,
        });
    }
}

/// `2024-05-01T12:34:56` (optionally with fraction and zone) as `2024-05-01 12:34 UTC`.
/// Anything that is not a plain ISO 8601 timestamp yields `None`: the text comes from the server.
fn timestamp(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if !raw.is_ascii() {
        return None;
    }
    let bytes = raw.as_bytes();
    let digits = |range: std::ops::Range<usize>| {
        bytes
            .get(range)
            .is_some_and(|part| part.iter().all(u8::is_ascii_digit))
    };
    let shape = digits(0..4)
        && bytes.get(4) == Some(&b'-')
        && digits(5..7)
        && bytes.get(7) == Some(&b'-')
        && digits(8..10)
        && matches!(bytes.get(10), Some(b'T' | b' '))
        && digits(11..13)
        && bytes.get(13) == Some(&b':')
        && digits(14..16);
    if !shape {
        return None;
    }
    let mut rest = raw.get(16..)?;
    if let Some(after) = rest.strip_prefix(':') {
        if !after.get(..2)?.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        rest = after.get(2..)?;
        if let Some(fraction) = rest.strip_prefix('.') {
            let tail = fraction.trim_start_matches(|c: char| c.is_ascii_digit());
            if tail.len() == fraction.len() {
                return None;
            }
            rest = tail;
        }
    }
    let zone = match rest {
        "" | "Z" | "+00:00" | "-00:00" => " UTC".to_owned(),
        other => {
            let offset = other.as_bytes();
            let valid = other.len() == 6
                && matches!(offset.first(), Some(b'+' | b'-'))
                && offset
                    .get(1..3)
                    .is_some_and(|h| h.iter().all(u8::is_ascii_digit))
                && offset.get(3) == Some(&b':')
                && offset
                    .get(4..6)
                    .is_some_and(|m| m.iter().all(u8::is_ascii_digit));
            if !valid {
                return None;
            }
            format!(" {other}")
        }
    };
    Some(format!("{} {}{zone}", raw.get(..10)?, raw.get(11..16)?))
}

fn clean_error(message: &str) -> String {
    let cleaned: String = message.chars().filter(|c| !c.is_control()).collect();
    let cleaned = cleaned.trim();
    let mut out: String = cleaned.chars().take(MAX_ERROR_CHARS).collect();
    if cleaned.chars().count() > MAX_ERROR_CHARS {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transfer(status: TransferStatus) -> Transfer {
        let mut transfer: Transfer = serde_json::from_str(r#"{"id": 1, "name": "x"}"#).unwrap();
        transfer.status = status;
        transfer
    }

    fn labelled(rows: &[DetailRow]) -> Vec<(DetailGroup, Msg, &str)> {
        rows.iter()
            .map(|r| (r.group, r.label.clone(), r.value.as_str()))
            .collect()
    }

    #[test]
    fn a_bare_transfer_has_no_details_and_a_status_headline() {
        let t = transfer(TransferStatus::Waiting);
        assert!(details(&t, Locale::En).is_empty());
        assert_eq!(headline(&t, Locale::En), "Waiting…");
    }

    #[test]
    fn a_failed_transfers_headline_is_short_and_its_reason_is_a_message() {
        let mut t = transfer(TransferStatus::Error);
        t.error_message = Some("Tracker unreachable".into());
        assert_eq!(headline(&t, Locale::En), "Failed");
        assert_eq!(headline(&t, Locale::De), "Fehlgeschlagen");
        let rows = details(&t, Locale::En);
        assert_eq!(
            labelled(&rows),
            [(
                DetailGroup::Messages,
                Msg::DetailError,
                "Tracker unreachable"
            )]
        );
    }

    #[test]
    fn details_of_a_seeding_torrent_are_grouped_in_display_order() {
        use DetailGroup::{Dates, Messages, Sharing, Transfer as Facts};
        let mut t = transfer(TransferStatus::Seeding);
        t.kind = putio_api::TransferKind::Torrent;
        t.size = 1024 * 1024;
        t.downloaded = Some(1024 * 1024);
        t.uploaded = Some(3 * 1024 * 1024);
        t.current_ratio = Some(3.0);
        t.seconds_seeding = Some(3_700);
        t.peers = Some(8);
        t.availability = Some(100);
        t.is_private = Some(true);
        t.tracker_message = Some("Announce OK".into());
        t.created_at = Some("2024-05-01T12:34:56".into());
        t.finished_at = Some("2024-05-01T13:00:00Z".into());
        assert_eq!(
            labelled(&details(&t, Locale::En)),
            [
                (Facts, Msg::DetailType, "Torrent"),
                (Facts, Msg::ColSize, "1.0 MB"),
                (Facts, Msg::DetailDownloaded, "1.0 MB"),
                (Sharing, Msg::DetailUploaded, "3.0 MB"),
                (Sharing, Msg::DetailRatio, "3.00"),
                (Sharing, Msg::DetailSeedingFor, "1h 01m"),
                (Sharing, Msg::DetailPeers, "8"),
                (Sharing, Msg::DetailAvailability, "100%"),
                (Sharing, Msg::DetailPrivate, "Yes"),
                (Dates, Msg::DetailAdded, "2024-05-01 12:34 UTC"),
                (Dates, Msg::DetailFinished, "2024-05-01 13:00 UTC"),
                (Messages, Msg::DetailTrackerMessage, "Announce OK"),
            ]
        );
    }

    #[test]
    fn the_error_is_only_shown_for_a_failed_transfer_and_is_cleaned() {
        let mut t = transfer(TransferStatus::Error);
        t.error_message = Some("Tracker\nunreachable\u{1b}[31m".into());
        t.tracker_message = Some("  ".into());
        let rows = details(&t, Locale::En);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].value, "Trackerunreachable[31m");
        let mut fine = transfer(TransferStatus::Downloading);
        fine.error_message = Some("old".into());
        assert!(details(&fine, Locale::En).is_empty());
    }

    #[test]
    fn nonsense_numbers_are_left_out() {
        let mut t = transfer(TransferStatus::Seeding);
        t.peers = Some(-1);
        t.availability = Some(250);
        t.current_ratio = Some(f64::NAN);
        t.uploaded = Some(0);
        t.seconds_seeding = Some(-5);
        assert!(details(&t, Locale::En).is_empty());
        t.current_ratio = Some(f64::INFINITY);
        assert!(details(&t, Locale::En).is_empty());
        t.current_ratio = Some(-1.0);
        assert!(details(&t, Locale::En).is_empty());
        t.current_ratio = Some(0.0);
        assert_eq!(details(&t, Locale::En).len(), 1);
    }

    #[test]
    fn a_long_tracker_message_is_bounded() {
        let mut t = transfer(TransferStatus::Seeding);
        t.tracker_message = Some("x".repeat(10_000));
        let rows = details(&t, Locale::En);
        assert!(rows.last().unwrap().value.chars().count() <= MAX_ERROR_CHARS + 1);
    }

    #[test]
    fn timestamps_are_shown_without_seconds_and_with_their_zone() {
        let shown = |raw: &str| timestamp(raw).unwrap_or_default();
        assert_eq!(shown("2024-05-01T12:34:56"), "2024-05-01 12:34 UTC");
        assert_eq!(shown("2024-05-01 12:34:56.789Z"), "2024-05-01 12:34 UTC");
        assert_eq!(shown("2024-05-01T12:34:56+00:00"), "2024-05-01 12:34 UTC");
        assert_eq!(
            shown("2024-05-01T12:34:56+02:00"),
            "2024-05-01 12:34 +02:00"
        );
        assert_eq!(shown("2024-05-01T12:34"), "2024-05-01 12:34 UTC");
        assert_eq!(shown(" 2024-05-01T12:34:56 "), "2024-05-01 12:34 UTC");
    }

    #[test]
    fn anything_that_is_not_a_timestamp_is_dropped() {
        for bad in [
            "",
            "yesterday",
            "2024-05-01",
            "2024/05/01T12:34:56",
            "2024-05-01T12-34-56",
            "2024-05-01T12:34:5x",
            "2024-05-01T12:34:56+2",
            "2024-05-01T12:34:56 UTC",
            "2024-05-01T12:34:56.Z",
            "２０２４-05-01T12:34:56",
            "2024-05-01T12:34:56\u{1b}[0m",
            "日本語日本語日本語日本語日本語日本語",
        ] {
            assert_eq!(timestamp(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn every_detail_label_is_translated() {
        for locale in [Locale::En, Locale::De, Locale::Es] {
            for msg in [
                Msg::MenuDetails,
                Msg::DetailGroupTransfer,
                Msg::DetailGroupSharing,
                Msg::DetailGroupDates,
                Msg::DetailsTitle,
                Msg::DetailStatus,
                Msg::DetailType,
                Msg::DetailDownloaded,
                Msg::DetailUploaded,
                Msg::DetailRatio,
                Msg::DetailSeedingFor,
                Msg::DetailPeers,
                Msg::DetailAvailability,
                Msg::DetailPrivate,
                Msg::DetailTrackerMessage,
                Msg::DetailError,
                Msg::DetailAdded,
                Msg::DetailFinished,
                Msg::DetailYes,
                Msg::DetailNo,
                Msg::KindTorrent,
                Msg::KindUrl,
                Msg::KindPlaylist,
                Msg::KindLiveStream,
            ] {
                assert!(!msg.text(locale).trim().is_empty(), "{msg:?} {locale:?}");
            }
        }
    }

    #[test]
    fn detail_for_a_running_download() {
        let mut t = transfer(TransferStatus::Downloading);
        t.down_speed = 1_572_864;
        t.estimated_time = Some(247);
        assert_eq!(
            detail(&t, Locale::En),
            "Downloading · 1.5 MB/s · 4m 07s left"
        );
        assert_eq!(
            detail(&t, Locale::De),
            "Lädt herunter · 1.5 MB/s · noch 4m 07s"
        );
    }

    #[test]
    fn detail_leaves_out_what_is_unknown() {
        let mut t = transfer(TransferStatus::Downloading);
        assert_eq!(detail(&t, Locale::En), "Downloading");
        t.down_speed = 2048;
        assert_eq!(detail(&t, Locale::En), "Downloading · 2.0 KB/s");
        t.estimated_time = Some(-1);
        assert_eq!(detail(&t, Locale::En), "Downloading · 2.0 KB/s");
    }

    #[test]
    fn detail_for_every_other_state() {
        let cases = [
            (TransferStatus::Completed, "Done"),
            (TransferStatus::Seeding, "Done · seeding"),
            (TransferStatus::Completing, "Finishing"),
            (TransferStatus::Waiting, "Waiting…"),
            (TransferStatus::InQueue, "In queue"),
            (TransferStatus::PreparingDownload, "Preparing…"),
            (TransferStatus::Error, "Failed"),
            (TransferStatus::Unknown, "–"),
        ];
        for (status, expected) in cases {
            assert_eq!(
                detail(&transfer(status), Locale::En),
                expected,
                "{status:?}"
            );
        }
    }

    #[test]
    fn a_failed_transfer_shows_the_server_message_sanitised_and_bounded() {
        let mut t = transfer(TransferStatus::Error);
        t.error_message = Some("Tracker\u{1b}[31m unreachable".into());
        assert_eq!(detail(&t, Locale::En), "Tracker[31m unreachable");
        t.error_message = Some("x".repeat(500));
        let text = detail(&t, Locale::En);
        assert_eq!(text.chars().count(), MAX_ERROR_CHARS + 1);
        assert!(text.ends_with('…'));
        t.error_message = Some("   ".into());
        assert_eq!(detail(&t, Locale::En), "Failed");
    }

    #[test]
    fn bytes_cover_units_and_edges() {
        assert_eq!(bytes(-5), "0 B");
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1), "1 B");
        assert_eq!(bytes(1023), "1023 B");
        assert_eq!(bytes(1024), "1.0 KB");
        assert_eq!(bytes(1536), "1.5 KB");
        assert_eq!(bytes(1024 * 1024), "1.0 MB");
        assert_eq!(bytes(13_194_139_533_312), "12.0 TB");
        assert_eq!(bytes(i64::MAX), "8192.0 PB");
    }

    #[test]
    fn rounding_never_shows_1024_of_a_unit() {
        // 1 MiB - 1 byte is 1023.999 KB, which must read 1.0 MB, not 1024.0 KB.
        assert_eq!(bytes(1024 * 1024 - 1), "1.0 MB");
    }

    #[test]
    fn speed_has_a_suffix() {
        assert_eq!(speed(0), "0 B/s");
        assert_eq!(speed(1_572_864), "1.5 MB/s");
    }

    #[test]
    fn durations() {
        assert_eq!(duration(-3), "0s");
        assert_eq!(duration(9), "9s");
        assert_eq!(duration(247), "4m 07s");
        assert_eq!(duration(3 * 3600 + 5 * 60 + 59), "3h 05m");
        assert_eq!(duration(100 * 3600), "100h 00m");
    }

    #[test]
    fn eta_by_status() {
        let en = Locale::En;
        assert_eq!(eta(TransferStatus::Downloading, Some(90), en), "1m 30s");
        assert_eq!(eta(TransferStatus::Downloading, None, en), "–");
        assert_eq!(eta(TransferStatus::Downloading, Some(-1), en), "–");
        assert_eq!(eta(TransferStatus::Waiting, Some(90), en), "–");
        assert_eq!(eta(TransferStatus::Completing, None, en), "Finishing");
        assert_eq!(eta(TransferStatus::Completed, None, en), "Done");
        assert_eq!(eta(TransferStatus::Seeding, Some(5), en), "Done");
    }

    mod props {
        use proptest::prelude::*;

        use super::super::*;

        proptest! {
            #[test]
            fn bytes_never_panics_and_never_shows_four_digit_units(n in any::<i64>()) {
                let text = bytes(n);
                prop_assert!(!text.is_empty());
                if let Some((number, unit)) = text.split_once(' ')
                    && unit != "B"
                    && unit != "PB"
                {
                    let parsed: f64 = number.parse().unwrap();
                    prop_assert!(parsed < 1024.0, "{text}");
                }
            }

            #[test]
            fn duration_never_panics(n in any::<i64>()) {
                let _ = duration(n);
            }
        }
    }
}
