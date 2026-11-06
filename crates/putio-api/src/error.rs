//! Error type for everything the API layer can fail at.

use std::time::Duration;

use crate::transport::TransportError;

/// Smallest and largest wait after a 429, whatever the server says.
const MIN_RETRY: Duration = Duration::from_secs(1);
const MAX_RETRY: Duration = Duration::from_secs(300);

/// An error response from put.io, as described by the `Error` schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiFailure {
    pub status: u16,
    /// `error_type`, e.g. `"NotFound"`. Absent when the body was not the documented JSON.
    pub error_type: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ApiError {
    /// Could not reach put.io (DNS, connect, TLS, timeout…).
    #[error("network error: {0}")]
    Network(String),
    /// 401: the token is missing, revoked or expired. The UI should ask the user to sign in again.
    #[error("not authorized")]
    Unauthorized,
    /// 429: back off for `retry_after`.
    #[error("rate limited, retry in {retry_after:?}")]
    RateLimited { retry_after: Duration },
    /// Any other error status, or a 2xx body that says `"status": "ERROR"`.
    #[error("put.io returned {}: {}", .0.status, .0.message)]
    Api(ApiFailure),
    /// The response was not what the documentation promises.
    #[error("unexpected response: {0}")]
    Decode(String),
    /// A response grew past the size limit.
    #[error("response too large")]
    TooLarge,
    /// The file being uploaded could not be read, or changed while it was read.
    #[error("file error: {0}")]
    File(String),
    /// The caller passed something that must never be sent.
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),
}

impl From<TransportError> for ApiError {
    fn from(error: TransportError) -> Self {
        match error {
            TransportError::Network(message) => Self::Network(message),
            TransportError::TooLarge => Self::TooLarge,
            TransportError::File(message) => Self::File(message),
        }
    }
}

impl ApiError {
    /// The `error_type` put.io reported, if any.
    #[must_use]
    pub fn error_type(&self) -> Option<&str> {
        match self {
            Self::Api(failure) => failure.error_type.as_deref(),
            _ => None,
        }
    }

    /// Whether retrying the same request later can succeed.
    #[must_use]
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Network(_) | Self::RateLimited { .. } => true,
            Self::Api(failure) => failure.status >= 500,
            _ => false,
        }
    }
}

/// How long to wait after a 429.
///
/// put.io documents `X-RateLimit-Reset` without saying whether it is an epoch timestamp or a number
/// of seconds, so both are handled: a value that looks like a Unix time (after year 2001) is made
/// relative to `now_epoch_secs`. The result is always clamped to 1 s..=5 min so a bad header can
/// neither hot-loop the API nor park the app for hours.
#[must_use]
pub fn retry_delay(reset: Option<u64>, retry_after: Option<u64>, now_epoch_secs: u64) -> Duration {
    const EPOCH_THRESHOLD: u64 = 1_000_000_000;
    let secs = match (retry_after, reset) {
        (Some(secs), _) => secs,
        (None, Some(value)) if value >= EPOCH_THRESHOLD => value.saturating_sub(now_epoch_secs),
        (None, Some(value)) => value,
        (None, None) => 0,
    };
    Duration::from_secs(secs).clamp(MIN_RETRY, MAX_RETRY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_delay_handles_relative_and_absolute_resets() {
        assert_eq!(
            retry_delay(Some(30), None, 1_700_000_000),
            Duration::from_secs(30)
        );
        assert_eq!(
            retry_delay(Some(1_700_000_042), None, 1_700_000_000),
            Duration::from_secs(42)
        );
    }

    #[test]
    fn retry_after_wins_and_everything_is_clamped() {
        assert_eq!(retry_delay(Some(99), Some(7), 0), Duration::from_secs(7));
        assert_eq!(retry_delay(None, None, 0), MIN_RETRY);
        assert_eq!(retry_delay(Some(0), None, 0), MIN_RETRY);
        assert_eq!(
            retry_delay(Some(1_700_000_000), None, 1_800_000_000),
            MIN_RETRY
        );
        assert_eq!(retry_delay(Some(9_999_999_999), None, 0), MAX_RETRY);
        assert_eq!(retry_delay(None, Some(100_000), 0), MAX_RETRY);
    }

    #[test]
    fn transient_classification() {
        assert!(ApiError::Network("x".into()).is_transient());
        assert!(
            ApiError::RateLimited {
                retry_after: MIN_RETRY
            }
            .is_transient()
        );
        let api = |status| {
            ApiError::Api(ApiFailure {
                status,
                error_type: None,
                message: String::new(),
            })
        };
        assert!(api(503).is_transient());
        assert!(!api(400).is_transient());
        assert!(!ApiError::Unauthorized.is_transient());
    }
}
