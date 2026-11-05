//! Derived views of the transfer list: summary, completion detection and sorting.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use putio_api::{Transfer, TransferStatus};

/// Counts shown in the header ("2 pending transfers | 5 finished").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Summary {
    pub pending: usize,
    pub finished: usize,
    pub failed: usize,
    /// Mean progress of the pending transfers (integer percent); `None` when nothing is pending.
    pub average_percent: Option<u8>,
}

#[must_use]
pub fn summarize(transfers: &[Transfer]) -> Summary {
    let mut summary = Summary::default();
    let mut percent_sum = 0usize;
    for transfer in transfers {
        if transfer.status.is_pending() {
            summary.pending += 1;
            percent_sum += usize::from(transfer.percent());
        } else if transfer.status.is_finished() {
            summary.finished += 1;
        } else if transfer.status == TransferStatus::Error {
            summary.failed += 1;
        }
    }
    summary.average_percent = percent_sum
        .checked_div(summary.pending)
        .and_then(|mean| u8::try_from(mean).ok());
    summary
}

/// A transfer that finished since the previous poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub id: i64,
    pub name: String,
    pub file_id: Option<i64>,
}

/// Remembers the previous poll to find transfers that just finished.
///
/// * The first poll only records state: transfers that were already done when the app started never notify.
/// * A transfer notifies when it moves from not-finished to finished between two polls.
/// * A transfer *this session added* that is first seen already finished (it completed between two
///   polls) also notifies.
#[derive(Debug, Default)]
pub struct Tracker {
    known: Option<HashMap<i64, TransferStatus>>,
    added: HashSet<i64>,
}

impl Tracker {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a transfer id returned by an add call.
    pub fn note_added(&mut self, id: i64) {
        self.added.insert(id);
    }

    /// Forget everything (sign-out).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn observe(&mut self, transfers: &[Transfer]) -> Vec<Completion> {
        let current: HashMap<i64, TransferStatus> =
            transfers.iter().map(|t| (t.id, t.status)).collect();
        let mut completed = Vec::new();
        if let Some(previous) = &self.known {
            for transfer in transfers {
                let was_added_here = self.added.remove(&transfer.id);
                let just_finished = transfer.status.is_finished()
                    && match previous.get(&transfer.id) {
                        Some(before) => !before.is_finished(),
                        None => was_added_here,
                    };
                if just_finished {
                    completed.push(Completion {
                        id: transfer.id,
                        name: transfer.name.clone(),
                        file_id: transfer.file_id,
                    });
                }
            }
        }
        self.known = Some(current);
        completed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    /// Running transfers first, then finished, then failed; newest first within a group.
    #[default]
    Default,
    Name,
    Progress,
    Speed,
    Eta,
    Size,
}

fn rank(status: TransferStatus) -> u8 {
    match status {
        TransferStatus::Downloading => 0,
        TransferStatus::Completing => 1,
        TransferStatus::PreparingDownload => 2,
        TransferStatus::Waiting => 3,
        TransferStatus::InQueue => 4,
        TransferStatus::Seeding | TransferStatus::Completed => 5,
        TransferStatus::Error => 6,
        TransferStatus::Unknown => 7,
    }
}

/// Stable sort; ties always fall back to newest-first so the order never flickers between polls.
pub fn sort(transfers: &mut [Transfer], key: SortKey, ascending: bool) {
    let newest_first = |a: &Transfer, b: &Transfer| b.id.cmp(&a.id);
    transfers.sort_by(|a, b| {
        let primary = match key {
            SortKey::Default => {
                return rank(a.status)
                    .cmp(&rank(b.status))
                    .then_with(|| newest_first(a, b));
            }
            SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortKey::Progress => a.percent_done.cmp(&b.percent_done),
            SortKey::Speed => a.down_speed.cmp(&b.down_speed),
            SortKey::Size => a.size.cmp(&b.size),
            SortKey::Eta => {
                // No estimate sorts last in both directions.
                return match (a.estimated_time, b.estimated_time) {
                    (Some(x), Some(y)) => {
                        if ascending {
                            x.cmp(&y)
                        } else {
                            y.cmp(&x)
                        }
                    }
                    (Some(_), None) => Ordering::Less,
                    (None, Some(_)) => Ordering::Greater,
                    (None, None) => Ordering::Equal,
                }
                .then_with(|| newest_first(a, b));
            }
        };
        let primary = if ascending {
            primary
        } else {
            primary.reverse()
        };
        primary.then_with(|| newest_first(a, b))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn transfer(id: i64, name: &str, status: TransferStatus, percent: i64) -> Transfer {
        let json = format!(r#"{{"id": {id}, "name": "{name}", "percent_done": {percent}}}"#);
        let mut transfer: Transfer = serde_json::from_str(&json).unwrap();
        transfer.status = status;
        transfer
    }

    use TransferStatus::*;

    #[test]
    fn summary_counts_and_averages() {
        let list = [
            transfer(1, "a", Downloading, 40),
            transfer(2, "b", Waiting, 0),
            transfer(3, "c", Completed, 100),
            transfer(4, "d", Seeding, 100),
            transfer(5, "e", Error, 3),
            transfer(6, "f", Unknown, 9),
        ];
        let s = summarize(&list);
        assert_eq!(
            (s.pending, s.finished, s.failed, s.average_percent),
            (2, 2, 1, Some(20))
        );
    }

    #[test]
    fn summary_of_nothing() {
        assert_eq!(summarize(&[]), Summary::default());
        assert_eq!(
            summarize(&[transfer(1, "a", Completed, 100)]).average_percent,
            None
        );
    }

    #[test]
    fn first_poll_never_notifies() {
        let mut tracker = Tracker::new();
        assert!(
            tracker
                .observe(&[transfer(1, "done", Completed, 100)])
                .is_empty()
        );
    }

    #[test]
    fn notifies_once_when_a_known_transfer_finishes() {
        let mut tracker = Tracker::new();
        tracker.observe(&[
            transfer(1, "a", Downloading, 50),
            transfer(2, "b", Downloading, 10),
        ]);
        let done = tracker.observe(&[
            transfer(1, "a", Completing, 99),
            transfer(2, "b", Downloading, 20),
        ]);
        assert!(done.is_empty());
        let done = tracker.observe(&[
            transfer(1, "a", Completed, 100),
            transfer(2, "b", Downloading, 30),
        ]);
        assert_eq!(done.iter().map(|c| c.id).collect::<Vec<_>>(), [1]);
        assert!(
            tracker
                .observe(&[
                    transfer(1, "a", Seeding, 100),
                    transfer(2, "b", Downloading, 40)
                ])
                .is_empty()
        );
    }

    #[test]
    fn completed_to_seeding_is_not_a_second_completion() {
        let mut tracker = Tracker::new();
        tracker.observe(&[transfer(1, "a", Completed, 100)]);
        assert!(
            tracker
                .observe(&[transfer(1, "a", Seeding, 100)])
                .is_empty()
        );
    }

    #[test]
    fn a_transfer_added_here_that_is_first_seen_finished_notifies() {
        let mut tracker = Tracker::new();
        tracker.observe(&[]);
        tracker.note_added(9);
        let done = tracker.observe(&[transfer(9, "fast", Completed, 100)]);
        assert_eq!(done.len(), 1);
        // …but an unrelated, already-finished newcomer (e.g. added from the website) does not.
        assert!(
            tracker
                .observe(&[
                    transfer(9, "fast", Completed, 100),
                    transfer(10, "other", Completed, 100)
                ])
                .is_empty()
        );
    }

    #[test]
    fn added_here_then_seen_unfinished_notifies_exactly_when_it_finishes() {
        let mut tracker = Tracker::new();
        tracker.observe(&[]);
        tracker.note_added(9);
        assert!(
            tracker
                .observe(&[transfer(9, "x", Downloading, 5)])
                .is_empty()
        );
        assert_eq!(
            tracker.observe(&[transfer(9, "x", Completed, 100)]).len(),
            1
        );
    }

    #[test]
    fn reset_forgets_state() {
        let mut tracker = Tracker::new();
        tracker.observe(&[transfer(1, "a", Downloading, 1)]);
        tracker.reset();
        assert!(
            tracker
                .observe(&[transfer(1, "a", Completed, 100)])
                .is_empty()
        );
    }

    #[test]
    fn default_sort_groups_by_state_then_newest() {
        let mut list = vec![
            transfer(1, "done-old", Completed, 100),
            transfer(2, "queued", InQueue, 0),
            transfer(3, "err", Error, 0),
            transfer(4, "dl-old", Downloading, 10),
            transfer(5, "done-new", Seeding, 100),
            transfer(6, "dl-new", Downloading, 70),
        ];
        sort(&mut list, SortKey::Default, true);
        let names: Vec<_> = list.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            ["dl-new", "dl-old", "queued", "done-new", "done-old", "err"]
        );
    }

    #[test]
    fn column_sorts_in_both_directions() {
        let mut list = vec![
            transfer(1, "banana", Downloading, 50),
            transfer(2, "Apple", Downloading, 90),
            transfer(3, "cherry", Downloading, 10),
        ];
        sort(&mut list, SortKey::Name, true);
        assert_eq!(list.iter().map(|t| t.id).collect::<Vec<_>>(), [2, 1, 3]);
        sort(&mut list, SortKey::Name, false);
        assert_eq!(list.iter().map(|t| t.id).collect::<Vec<_>>(), [3, 1, 2]);
        sort(&mut list, SortKey::Progress, false);
        assert_eq!(list.iter().map(|t| t.id).collect::<Vec<_>>(), [2, 1, 3]);
    }

    #[test]
    fn eta_sort_puts_unknown_last_both_ways() {
        let mut a = transfer(1, "a", Downloading, 0);
        a.estimated_time = Some(50);
        let mut b = transfer(2, "b", Downloading, 0);
        b.estimated_time = None;
        let mut c = transfer(3, "c", Downloading, 0);
        c.estimated_time = Some(10);
        let mut list = vec![a, b, c];
        sort(&mut list, SortKey::Eta, true);
        assert_eq!(list.iter().map(|t| t.id).collect::<Vec<_>>(), [3, 1, 2]);
        sort(&mut list, SortKey::Eta, false);
        assert_eq!(list.iter().map(|t| t.id).collect::<Vec<_>>(), [1, 3, 2]);
    }

    #[test]
    fn ties_are_newest_first_for_a_stable_order() {
        let mut list = vec![
            transfer(1, "same", Downloading, 5),
            transfer(2, "same", Downloading, 5),
        ];
        sort(&mut list, SortKey::Name, true);
        assert_eq!(list.iter().map(|t| t.id).collect::<Vec<_>>(), [2, 1]);
    }
}
