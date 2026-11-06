//! When to fetch a list from put.io again: the bookkeeping the feeds and the files share.
//!
//! A list is asked for when it is on screen, not before; a good answer is good for a while; a bad one is
//! retried after a wait that doubles for each failure in a row (or as long as put.io asks, when it
//! rate-limits). Nothing here talks to anyone: the owner asks it "is it time?" and tells it how things went.

use putio_api::ApiError;

use super::{MAX_BACKOFF, Millis};

/// The wait after a first failure, doubled for each failure in a row.
const RETRY_INTERVAL: Millis = 10_000;

#[derive(Debug, Default)]
pub(super) struct Refresh {
    in_flight: bool,
    /// Fetch at or after this time. `None`: as soon as the list is wanted.
    next_at: Option<Millis>,
    loaded_at: Option<Millis>,
    failures: u32,
}

impl Refresh {
    /// Whether a request should go out now (when the list is wanted).
    pub(super) fn due(&self, now: Millis) -> bool {
        !self.in_flight && self.next_at.is_none_or(|at| now >= at)
    }

    /// A request has gone out.
    pub(super) fn started(&mut self) {
        self.in_flight = true;
        self.next_at = None;
    }

    /// Ask again at the next opportunity.
    pub(super) fn soon(&mut self, now: Millis) {
        self.next_at = Some(now);
    }

    /// The list is older than `after`, or has never arrived.
    pub(super) fn is_older_than(&self, now: Millis, after: Millis) -> bool {
        self.loaded_at
            .is_none_or(|at| now.saturating_sub(at) >= after)
    }

    /// An answer arrived and the list is good for `every` milliseconds.
    pub(super) fn succeeded(&mut self, now: Millis, every: Millis) {
        self.in_flight = false;
        self.failures = 0;
        self.loaded_at = Some(now);
        self.next_at = Some(now.saturating_add(every));
    }

    /// An answer arrived and it was a failure.
    pub(super) fn failed(&mut self, now: Millis, error: &ApiError) {
        self.in_flight = false;
        let wait = retry_wait(error, self.failures);
        self.failures = self.failures.saturating_add(1);
        self.next_at = Some(now.saturating_add(wait));
    }

    /// An answer arrived that nobody wants any more (the list was left): the request is over, nothing else
    /// changes.
    pub(super) fn answered(&mut self) {
        self.in_flight = false;
    }

    /// What the list shows has changed (another folder): whatever is on its way is of no interest, ask
    /// again now.
    pub(super) fn restart(&mut self, now: Millis) {
        self.in_flight = false;
        self.failures = 0;
        self.next_at = Some(now);
    }

    /// The user asked to try again: forget the failures and go now.
    pub(super) fn retry(&mut self, now: Millis) {
        self.failures = 0;
        self.next_at = Some(now);
    }
}

fn retry_wait(error: &ApiError, failures: u32) -> Millis {
    if let ApiError::RateLimited { retry_after } = error {
        let ms = u64::try_from(retry_after.as_millis()).unwrap_or(MAX_BACKOFF);
        return ms.min(MAX_BACKOFF);
    }
    RETRY_INTERVAL
        .saturating_mul(1u64 << failures.min(5))
        .min(MAX_BACKOFF)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use putio_api::ApiFailure;

    use super::*;

    fn error() -> ApiError {
        ApiError::Api(ApiFailure {
            status: 503,
            error_type: None,
            message: String::new(),
        })
    }

    #[test]
    fn a_new_list_is_due_at_once_and_then_only_one_request_at_a_time() {
        let mut refresh = Refresh::default();
        assert!(refresh.due(0));
        refresh.started();
        assert!(!refresh.due(1_000_000), "one request at a time");
        refresh.succeeded(10, 60_000);
        assert!(!refresh.due(59_999) && refresh.due(60_010));
    }

    #[test]
    fn failures_back_off_and_a_retry_starts_over() {
        let mut refresh = Refresh::default();
        let mut waits = Vec::new();
        let mut now = 0;
        for _ in 0..8 {
            refresh.started();
            refresh.failed(now, &error());
            let due_at = (now..now + 1_000_000).find(|t| refresh.due(*t)).unwrap();
            waits.push(due_at - now);
            now = due_at;
        }
        assert_eq!(waits[..3], [10_000, 20_000, 40_000]);
        assert!(waits.iter().all(|w| *w <= MAX_BACKOFF), "{waits:?}");
        assert!(waits.windows(2).all(|pair| pair[0] <= pair[1]));
        refresh.retry(now);
        assert!(refresh.due(now));
        refresh.started();
        refresh.failed(now, &error());
        assert!(!refresh.due(now + 9_999) && refresh.due(now + 10_000));
    }

    #[test]
    fn a_rate_limit_is_waited_out_but_never_longer_than_the_cap() {
        let mut refresh = Refresh::default();
        let limited = |secs| ApiError::RateLimited {
            retry_after: Duration::from_secs(secs),
        };
        refresh.started();
        refresh.failed(0, &limited(45));
        assert!(!refresh.due(44_999) && refresh.due(45_000));
        refresh.started();
        refresh.failed(0, &limited(86_400));
        assert!(!refresh.due(MAX_BACKOFF - 1) && refresh.due(MAX_BACKOFF));
    }

    #[test]
    fn age_counts_from_the_last_good_answer() {
        let mut refresh = Refresh::default();
        assert!(
            refresh.is_older_than(0, 15_000),
            "never loaded counts as old"
        );
        refresh.started();
        refresh.succeeded(100, 60_000);
        assert!(!refresh.is_older_than(14_000, 15_000));
        assert!(refresh.is_older_than(15_100, 15_000));
        // An answer nobody wanted ends the request and nothing else.
        refresh.started();
        refresh.answered();
        assert!(refresh.due(100), "the request is over");
        assert!(!refresh.is_older_than(14_000, 15_000));
    }
}
