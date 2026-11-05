//! Human-readable numbers for the transfers table and header.

use putio_api::TransferStatus;

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

#[cfg(test)]
mod tests {
    use super::*;

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
