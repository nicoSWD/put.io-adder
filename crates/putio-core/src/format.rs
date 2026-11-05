//! Human-readable numbers for the transfers table and header.

use putio_api::{Transfer, TransferStatus};

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
