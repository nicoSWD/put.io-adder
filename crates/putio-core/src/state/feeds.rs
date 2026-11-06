//! The RSS feeds part of the state machine: the list put.io holds for the account, when to fetch it
//! again, and the adding, changing, pausing and deleting that goes through it.
//!
//! Feeds live on put.io, which checks them and turns new items into transfers (those show up in the
//! transfers list like any other). All the app does is manage the list, so nothing here is polled
//! unless the Feeds tab is the one being looked at.

use putio_api::{ApiError, RssFeed, RssFeedSettings};

use crate::feeds::sort_feeds;
use crate::i18n::Msg;
use crate::settings::Tab;

use super::{
    ERROR_NOTICE_TIME, Effect, MAX_BACKOFF, Millis, NOTICE_TIME, Op, OpId, State, session_expired,
};

/// How old the list may get before it is fetched again while its tab is open.
const REFRESH_INTERVAL: Millis = 60_000;
/// Opening the tab fetches the list again when it is older than this.
const STALE_AFTER: Millis = 15_000;
/// The wait after a failed fetch, doubled for each failure in a row.
const RETRY_INTERVAL: Millis = 10_000;

/// The feeds as last fetched.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FeedList {
    /// Newest first.
    pub items: Vec<RssFeed>,
    /// The list has been fetched at least once since signing in.
    pub loaded: bool,
    /// The first fetch failed (a later one failing keeps the list that is shown).
    pub failed: bool,
}

/// Where the add / edit form's request stands. The form itself is the shell's; it reads this to know
/// whether to show a spinner, an error, or to close.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum FeedSave {
    #[default]
    Idle,
    Saving,
    /// put.io said no; the text is for the user.
    Failed(String),
    /// Done: the form closes and acknowledges with `Event::FeedSaveDismissed`.
    Saved,
}

/// When to fetch the list.
#[derive(Debug, Default)]
pub(super) struct Schedule {
    in_flight: bool,
    /// Fetch at or after this time. `None`: as soon as the list is wanted.
    next_at: Option<Millis>,
    loaded_at: Option<Millis>,
    failures: u32,
}

/// Whether the list is on screen: signed in, list open, Feeds tab chosen.
fn wanted(state: &State) -> bool {
    state.is_signed_in() && state.transfers_expanded && state.tab == Tab::Feeds
}

/// Fetch again at the next opportunity.
fn refresh_soon(state: &mut State, now: Millis) {
    state.feed_schedule.next_at = Some(now);
}

pub(super) fn select_tab(state: &mut State, now: Millis, tab: Tab, fx: &mut Vec<Effect>) {
    if state.tab == tab {
        return;
    }
    state.tab = tab;
    state.settings.tab = tab;
    fx.push(Effect::SaveSettings(state.settings.clone()));
    let stale = state
        .feed_schedule
        .loaded_at
        .is_none_or(|at| now.saturating_sub(at) >= STALE_AFTER);
    if tab == Tab::Feeds && stale {
        refresh_soon(state, now);
    }
}

/// Ask for the list when it is on screen and due. Runs after every event.
pub(super) fn maybe_load(state: &mut State, now: Millis, fx: &mut Vec<Effect>) {
    if wanted(state)
        && !state.feed_schedule.in_flight
        && state.feed_schedule.next_at.is_none_or(|at| now >= at)
    {
        state.feed_schedule.in_flight = true;
        state.feed_schedule.next_at = None;
        fx.push(Effect::LoadFeeds);
    }
}

/// The "try again" button after the first fetch failed.
pub(super) fn retry(state: &mut State, now: Millis) {
    state.feeds.failed = false;
    state.feed_schedule.failures = 0;
    refresh_soon(state, now);
}

pub(super) fn loaded(
    state: &mut State,
    now: Millis,
    result: Result<Vec<RssFeed>, ApiError>,
    fx: &mut Vec<Effect>,
) {
    state.feed_schedule.in_flight = false;
    if !state.is_signed_in() {
        return;
    }
    match result {
        Ok(mut items) => {
            sort_feeds(&mut items);
            state.feeds = FeedList {
                items,
                loaded: true,
                failed: false,
            };
            state.feed_schedule.failures = 0;
            state.feed_schedule.loaded_at = Some(now);
            state.feed_schedule.next_at = Some(now + REFRESH_INTERVAL);
        }
        Err(ApiError::Unauthorized) => session_expired(state, fx),
        Err(error) => {
            // A list that was fetched before stays on screen; only the very first fetch has nothing to show.
            state.feeds.failed = !state.feeds.loaded;
            let wait = retry_wait(&error, state.feed_schedule.failures);
            state.feed_schedule.failures = state.feed_schedule.failures.saturating_add(1);
            state.feed_schedule.next_at = Some(now.saturating_add(wait));
        }
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

pub(super) fn save_requested(
    state: &mut State,
    id: Option<i64>,
    settings: RssFeedSettings,
    fx: &mut Vec<Effect>,
) {
    if !state.is_signed_in() || state.feed_save == FeedSave::Saving {
        return;
    }
    let op = state.op(if id.is_some() {
        Op::FeedUpdate
    } else {
        Op::FeedCreate
    });
    state.feed_save = FeedSave::Saving;
    fx.push(Effect::SaveFeed { op, id, settings });
}

pub(super) fn save_finished(
    state: &mut State,
    now: Millis,
    op: OpId,
    result: Result<(), ApiError>,
    fx: &mut Vec<Effect>,
) {
    let created = state.ops.get(&op) == Some(&Op::FeedCreate);
    if !state.finish(op, &[Op::FeedCreate, Op::FeedUpdate]) {
        return;
    }
    match result {
        Ok(()) => {
            state.feed_save = FeedSave::Saved;
            let said = if created {
                Msg::FeedAdded
            } else {
                Msg::FeedUpdated
            };
            state.say(said, now, NOTICE_TIME);
            refresh_soon(state, now);
        }
        Err(ApiError::Unauthorized) => session_expired(state, fx),
        Err(error) => state.feed_save = FeedSave::Failed(save_failure(state, &error)),
    }
}

/// What to tell the user when put.io refuses a feed: its own reasons come first, in the user's language.
fn save_failure(state: &State, error: &ApiError) -> String {
    let known = match error.error_type() {
        Some("URL_NOT_VALID" | "URL_REQUIRED") => Some(Msg::FeedErrUrlInvalid),
        Some("FEED_CANNOT_FETCHED") => Some(Msg::FeedErrCannotFetch),
        Some("FEED_CANNOT_PARSED") => Some(Msg::FeedErrCannotParse),
        Some("TOO_MANY_RSS_FEEDS") => Some(Msg::FeedErrTooMany),
        _ => None,
    };
    known.map_or_else(|| state.reason(error), |msg| state.text(&msg))
}

pub(super) fn save_dismissed(state: &mut State) {
    if matches!(state.feed_save, FeedSave::Saved | FeedSave::Failed(_)) {
        state.feed_save = FeedSave::Idle;
    }
}

pub(super) fn pause_requested(state: &mut State, id: i64, paused: bool, fx: &mut Vec<Effect>) {
    if state.is_signed_in() {
        let op = state.op(Op::FeedPause);
        fx.push(Effect::SetFeedPaused { op, id, paused });
    }
}

pub(super) fn pause_finished(
    state: &mut State,
    now: Millis,
    (op, id, paused): (OpId, i64, bool),
    result: Result<(), ApiError>,
    fx: &mut Vec<Effect>,
) {
    if !state.finish(op, &[Op::FeedPause]) {
        return;
    }
    match result {
        Ok(()) => {
            if let Some(feed) = state.feeds.items.iter_mut().find(|feed| feed.id == id) {
                feed.paused = paused;
            }
            let said = if paused {
                Msg::FeedPausedNotice
            } else {
                Msg::FeedResumedNotice
            };
            state.say(said, now, NOTICE_TIME);
            refresh_soon(state, now);
        }
        Err(ApiError::Unauthorized) => session_expired(state, fx),
        Err(error) => failed(state, now, &error),
    }
}

pub(super) fn delete_requested(state: &mut State, id: i64, fx: &mut Vec<Effect>) {
    if state.is_signed_in() {
        let op = state.op(Op::FeedDelete);
        fx.push(Effect::DeleteFeed { op, id });
    }
}

pub(super) fn delete_finished(
    state: &mut State,
    now: Millis,
    (op, id): (OpId, i64),
    result: Result<(), ApiError>,
    fx: &mut Vec<Effect>,
) {
    if !state.finish(op, &[Op::FeedDelete]) {
        return;
    }
    match result {
        Ok(()) => {
            state.feeds.items.retain(|feed| feed.id != id);
            state.say(Msg::FeedDeletedNotice, now, NOTICE_TIME);
            refresh_soon(state, now);
        }
        Err(ApiError::Unauthorized) => session_expired(state, fx),
        Err(error) => failed(state, now, &error),
    }
}

fn failed(state: &mut State, now: Millis, error: &ApiError) {
    let reason = state.reason(error);
    state.say(Msg::FeedActionFailed { reason }, now, ERROR_NOTICE_TIME);
}

pub(super) fn copy_address(state: &mut State, now: Millis, id: i64, fx: &mut Vec<Effect>) {
    // Only a web address is put on the clipboard, never whatever text the server stored.
    let address = state
        .feeds
        .items
        .iter()
        .find(|feed| feed.id == id)
        .map(|feed| feed.rss_source_url.trim().to_owned())
        .filter(|url| crate::feeds::host_of(url).is_some());
    if let Some(address) = address {
        fx.push(Effect::CopyText(address));
        state.say(Msg::Copied, now, NOTICE_TIME);
    }
}

/// Forget everything about the account's feeds (signing out).
pub(super) fn reset(state: &mut State) {
    state.feeds = FeedList::default();
    state.feed_save = FeedSave::Idle;
    state.feed_schedule = Schedule::default();
}

#[cfg(test)]
mod tests {
    use putio_api::ApiFailure;

    use super::super::{Auth, Event};
    use super::*;
    use crate::i18n::Locale;
    use crate::settings::Settings;
    use crate::state::update;

    fn signed_in() -> State {
        let settings = Settings {
            check_for_updates: false,
            transfers_expanded: true,
            ..Settings::default()
        };
        let mut state = State::new(settings, Locale::En, "0.1.0");
        update(&mut state, 0, Event::Started { has_token: true });
        // The first transfers poll has been answered.
        update(
            &mut state,
            0,
            Event::Polled {
                account: Ok(putio_api::Account::default()),
                transfers: Ok(Vec::new()),
            },
        );
        assert_eq!(state.auth, Auth::SignedIn);
        state
    }

    fn feed(id: i64, title: &str) -> RssFeed {
        RssFeed {
            id,
            title: title.into(),
            rss_source_url: "https://example.invalid/feed".into(),
            ..RssFeed::default()
        }
    }

    fn api_error(status: u16, kind: &str, message: &str) -> ApiError {
        ApiError::Api(ApiFailure {
            status,
            error_type: Some(kind.into()),
            message: message.into(),
        })
    }

    fn settings() -> RssFeedSettings {
        RssFeedSettings {
            title: "T".into(),
            url: "https://example.invalid/feed".into(),
            ..RssFeedSettings::default()
        }
    }

    fn op_of(fx: &[Effect]) -> OpId {
        fx.iter()
            .find_map(|effect| match effect {
                Effect::SaveFeed { op, .. }
                | Effect::SetFeedPaused { op, .. }
                | Effect::DeleteFeed { op, .. } => Some(*op),
                _ => None,
            })
            .expect("a feed effect with an op id")
    }

    /// Open the Feeds tab and answer the first fetch with `feeds`.
    fn with_feeds(state: &mut State, now: Millis, feeds: Vec<RssFeed>) {
        let fx = update(state, now, Event::SelectTab(Tab::Feeds));
        assert!(fx.contains(&Effect::LoadFeeds), "{fx:?}");
        update(state, now, Event::FeedsLoaded(Ok(feeds)));
    }

    #[test]
    fn nothing_is_fetched_until_the_feeds_tab_is_open() {
        let mut state = signed_in();
        for now in [1_000, 100_000, 1_000_000] {
            assert!(!update(&mut state, now, Event::Tick).contains(&Effect::LoadFeeds));
        }
        let fx = update(&mut state, 2_000_000, Event::SelectTab(Tab::Feeds));
        assert_eq!(
            fx.iter().filter(|e| **e == Effect::LoadFeeds).count(),
            1,
            "{fx:?}"
        );
        // Asked once; not again while the answer is on its way.
        assert!(!update(&mut state, 2_000_100, Event::Tick).contains(&Effect::LoadFeeds));
    }

    #[test]
    fn the_tab_is_remembered_and_saved() {
        let mut state = signed_in();
        let fx = update(&mut state, 0, Event::SelectTab(Tab::Feeds));
        assert_eq!(state.tab, Tab::Feeds);
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::SaveSettings(s) if s.tab == Tab::Feeds))
        );
        // Choosing the tab that is already open does nothing (no save, no fetch).
        assert!(
            update(&mut state, 1, Event::SelectTab(Tab::Feeds))
                .iter()
                .all(|e| !matches!(e, Effect::SaveSettings(_)))
        );
        let back = State::new(
            Settings {
                tab: Tab::Feeds,
                ..Settings::default()
            },
            Locale::En,
            "0.1.0",
        );
        assert_eq!(back.tab, Tab::Feeds);
    }

    #[test]
    fn a_closed_list_fetches_nothing_and_opening_it_does() {
        let mut state = signed_in();
        update(&mut state, 0, Event::SelectTab(Tab::Feeds));
        update(&mut state, 1, Event::FeedsLoaded(Ok(vec![feed(1, "a")])));
        update(&mut state, 2, Event::ToggleTransfers); // collapsed
        assert!(!state.transfers_expanded);
        assert!(!update(&mut state, 1_000_000, Event::Tick).contains(&Effect::LoadFeeds));
        // Opening the list again fetches the (old) list straight away, once.
        let fx = update(&mut state, 1_000_001, Event::ToggleTransfers);
        assert_eq!(
            fx.iter().filter(|e| **e == Effect::LoadFeeds).count(),
            1,
            "{fx:?}"
        );
        assert!(!update(&mut state, 1_000_002, Event::Tick).contains(&Effect::LoadFeeds));
    }

    #[test]
    fn the_list_is_kept_fresh_only_while_it_is_looked_at() {
        let mut state = signed_in();
        with_feeds(&mut state, 0, vec![feed(1, "a"), feed(7, "b")]);
        assert_eq!(
            state.feeds.items.iter().map(|f| f.id).collect::<Vec<_>>(),
            [7, 1],
            "newest first"
        );
        assert!(state.feeds.loaded && !state.feeds.failed);
        assert!(
            !update(&mut state, REFRESH_INTERVAL - 1, Event::Tick).contains(&Effect::LoadFeeds)
        );
        assert!(update(&mut state, REFRESH_INTERVAL, Event::Tick).contains(&Effect::LoadFeeds));
        // Switching away and back soon does not fetch again; after a while it does.
        update(
            &mut state,
            REFRESH_INTERVAL + 1,
            Event::FeedsLoaded(Ok(vec![])),
        );
        update(&mut state, 70_000, Event::SelectTab(Tab::Transfers));
        let soon = update(&mut state, 71_000, Event::SelectTab(Tab::Feeds));
        assert!(!soon.contains(&Effect::LoadFeeds), "{soon:?}");
        update(&mut state, 72_000, Event::SelectTab(Tab::Transfers));
        let later = update(&mut state, 100_000, Event::SelectTab(Tab::Feeds));
        assert!(later.contains(&Effect::LoadFeeds), "{later:?}");
    }

    #[test]
    fn a_failed_first_fetch_says_so_and_backs_off_a_failed_later_one_keeps_the_list() {
        let mut state = signed_in();
        update(&mut state, 0, Event::SelectTab(Tab::Feeds));
        update(
            &mut state,
            10,
            Event::FeedsLoaded(Err(api_error(503, "x", "down"))),
        );
        assert!(state.feeds.failed && !state.feeds.loaded);
        assert!(!update(&mut state, 5_000, Event::Tick).contains(&Effect::LoadFeeds));
        assert!(update(&mut state, 10_000 + 10, Event::Tick).contains(&Effect::LoadFeeds));
        // The retry button asks again at once and clears the error.
        update(
            &mut state,
            10_020,
            Event::FeedsLoaded(Err(ApiError::Network("x".into()))),
        );
        assert!(state.feeds.failed);
        let fx = update(&mut state, 10_030, Event::FeedsRefreshRequested);
        assert!(!state.feeds.failed);
        assert!(fx.contains(&Effect::LoadFeeds));
        update(
            &mut state,
            10_040,
            Event::FeedsLoaded(Ok(vec![feed(1, "a")])),
        );
        // Later failures leave what was fetched on screen.
        update(
            &mut state,
            80_000,
            Event::FeedsLoaded(Err(ApiError::Network("x".into()))),
        );
        assert!(!state.feeds.failed);
        assert_eq!(state.feeds.items.len(), 1);
    }

    #[test]
    fn a_rate_limit_is_waited_out() {
        let mut state = signed_in();
        update(&mut state, 0, Event::SelectTab(Tab::Feeds));
        let limited = ApiError::RateLimited {
            retry_after: std::time::Duration::from_secs(45),
        };
        update(&mut state, 0, Event::FeedsLoaded(Err(limited)));
        assert!(!update(&mut state, 44_000, Event::Tick).contains(&Effect::LoadFeeds));
        assert!(update(&mut state, 45_000, Event::Tick).contains(&Effect::LoadFeeds));
    }

    #[test]
    fn adding_a_feed_goes_through_saving_to_saved_and_refreshes_the_list() {
        let mut state = signed_in();
        with_feeds(&mut state, 0, vec![]);
        let fx = update(
            &mut state,
            100,
            Event::FeedSaveRequested {
                id: None,
                settings: settings(),
            },
        );
        assert!(matches!(&fx[..], [Effect::SaveFeed { id: None, .. }]));
        assert_eq!(*state.feed_save(), FeedSave::Saving);
        assert!(state.is_busy());
        // A second click while it is saving is ignored.
        assert!(
            update(
                &mut state,
                101,
                Event::FeedSaveRequested {
                    id: None,
                    settings: settings()
                }
            )
            .is_empty()
        );
        let fx = update(
            &mut state,
            200,
            Event::FeedSaved {
                op: op_of(&fx),
                result: Ok(()),
            },
        );
        assert_eq!(*state.feed_save(), FeedSave::Saved);
        assert_eq!(*state.status(200), Msg::FeedAdded);
        assert!(
            fx.contains(&Effect::LoadFeeds),
            "the list is fetched again: {fx:?}"
        );
        assert!(!state.is_busy());
        update(&mut state, 300, Event::FeedSaveDismissed);
        assert_eq!(*state.feed_save(), FeedSave::Idle);
    }

    #[test]
    fn changing_a_feed_is_told_apart_from_adding_one() {
        let mut state = signed_in();
        with_feeds(&mut state, 0, vec![feed(5, "a")]);
        let fx = update(
            &mut state,
            1,
            Event::FeedSaveRequested {
                id: Some(5),
                settings: settings(),
            },
        );
        assert!(matches!(&fx[..], [Effect::SaveFeed { id: Some(5), .. }]));
        update(
            &mut state,
            2,
            Event::FeedSaved {
                op: op_of(&fx),
                result: Ok(()),
            },
        );
        assert_eq!(*state.status(2), Msg::FeedUpdated);
    }

    #[test]
    fn put_io_refusing_a_feed_is_explained_in_the_users_words_and_the_form_stays() {
        for (kind, expected) in [
            ("URL_NOT_VALID", Msg::FeedErrUrlInvalid),
            ("FEED_CANNOT_FETCHED", Msg::FeedErrCannotFetch),
            ("FEED_CANNOT_PARSED", Msg::FeedErrCannotParse),
            ("TOO_MANY_RSS_FEEDS", Msg::FeedErrTooMany),
        ] {
            let mut state = signed_in();
            let fx = update(
                &mut state,
                1,
                Event::FeedSaveRequested {
                    id: None,
                    settings: settings(),
                },
            );
            update(
                &mut state,
                2,
                Event::FeedSaved {
                    op: op_of(&fx),
                    result: Err(api_error(400, kind, "raw server text")),
                },
            );
            assert_eq!(
                *state.feed_save(),
                FeedSave::Failed(expected.text(Locale::En)),
                "{kind}"
            );
        }
        // Anything else: the server's own words, cleaned; nothing is lost on the way.
        let mut state = signed_in();
        let fx = update(
            &mut state,
            1,
            Event::FeedSaveRequested {
                id: None,
                settings: settings(),
            },
        );
        update(
            &mut state,
            2,
            Event::FeedSaved {
                op: op_of(&fx),
                result: Err(api_error(400, "OTHER", "bad\u{7} feed")),
            },
        );
        assert_eq!(*state.feed_save(), FeedSave::Failed("bad feed".into()));
        // After a refusal the user can try again.
        let again = update(
            &mut state,
            3,
            Event::FeedSaveRequested {
                id: None,
                settings: settings(),
            },
        );
        assert!(matches!(&again[..], [Effect::SaveFeed { .. }]));
    }

    #[test]
    fn pausing_and_resuming_change_the_row_at_once_and_say_so() {
        let mut state = signed_in();
        with_feeds(&mut state, 0, vec![feed(5, "a")]);
        let fx = update(
            &mut state,
            1,
            Event::FeedPauseRequested {
                id: 5,
                paused: true,
            },
        );
        assert!(matches!(
            &fx[..],
            [Effect::SetFeedPaused {
                id: 5,
                paused: true,
                ..
            }]
        ));
        update(
            &mut state,
            2,
            Event::FeedPauseFinished {
                op: op_of(&fx),
                id: 5,
                paused: true,
                result: Ok(()),
            },
        );
        assert!(state.feeds.items[0].paused);
        assert_eq!(*state.status(2), Msg::FeedPausedNotice);
        let fx = update(
            &mut state,
            3,
            Event::FeedPauseRequested {
                id: 5,
                paused: false,
            },
        );
        update(
            &mut state,
            4,
            Event::FeedPauseFinished {
                op: op_of(&fx),
                id: 5,
                paused: false,
                result: Ok(()),
            },
        );
        assert!(!state.feeds.items[0].paused);
        assert_eq!(*state.status(4), Msg::FeedResumedNotice);
        // A failure changes nothing and says why.
        let fx = update(
            &mut state,
            5,
            Event::FeedPauseRequested {
                id: 5,
                paused: true,
            },
        );
        update(
            &mut state,
            6,
            Event::FeedPauseFinished {
                op: op_of(&fx),
                id: 5,
                paused: true,
                result: Err(api_error(404, "NotFound", "no such feed")),
            },
        );
        assert!(!state.feeds.items[0].paused);
        assert_eq!(
            *state.status(6),
            Msg::FeedActionFailed {
                reason: "no such feed".into()
            }
        );
    }

    #[test]
    fn deleting_removes_the_row_only_when_put_io_agrees() {
        let mut state = signed_in();
        with_feeds(&mut state, 0, vec![feed(5, "a"), feed(6, "b")]);
        let fx = update(&mut state, 1, Event::FeedDeleteRequested(5));
        assert!(matches!(&fx[..], [Effect::DeleteFeed { id: 5, .. }]));
        update(
            &mut state,
            2,
            Event::FeedDeleteFinished {
                op: op_of(&fx),
                id: 5,
                result: Err(ApiError::Network("x".into())),
            },
        );
        assert_eq!(state.feeds.items.len(), 2);
        let fx = update(&mut state, 3, Event::FeedDeleteRequested(5));
        update(
            &mut state,
            4,
            Event::FeedDeleteFinished {
                op: op_of(&fx),
                id: 5,
                result: Ok(()),
            },
        );
        assert_eq!(
            state.feeds.items.iter().map(|f| f.id).collect::<Vec<_>>(),
            [6]
        );
        assert_eq!(*state.status(4), Msg::FeedDeletedNotice);
    }

    #[test]
    fn only_a_web_address_is_copied() {
        let mut state = signed_in();
        let mut odd = feed(6, "odd");
        odd.rss_source_url = "javascript:alert(1)".into();
        with_feeds(&mut state, 0, vec![feed(5, "a"), odd]);
        assert_eq!(
            update(&mut state, 1, Event::FeedCopyAddressRequested(5)),
            [Effect::CopyText("https://example.invalid/feed".into())]
        );
        assert_eq!(*state.status(1), Msg::Copied);
        assert!(update(&mut state, 2, Event::FeedCopyAddressRequested(6)).is_empty());
        assert!(update(&mut state, 3, Event::FeedCopyAddressRequested(404)).is_empty());
    }

    #[test]
    fn unknown_and_repeated_answers_are_ignored() {
        let mut state = signed_in();
        with_feeds(&mut state, 0, vec![feed(5, "a")]);
        let fx = update(&mut state, 1, Event::FeedDeleteRequested(5));
        let op = op_of(&fx);
        let done = || Event::FeedDeleteFinished {
            op,
            id: 5,
            result: Ok(()),
        };
        update(&mut state, 2, done());
        update(&mut state, 3, Event::FeedsLoaded(Ok(vec![feed(5, "a")])));
        // The same answer again must not remove the feed that is back.
        update(&mut state, 4, done());
        assert_eq!(state.feeds.items.len(), 1);
        update(
            &mut state,
            5,
            Event::FeedSaved {
                op: OpId(9_999),
                result: Ok(()),
            },
        );
        assert_eq!(*state.feed_save(), FeedSave::Idle);
    }

    #[test]
    fn an_expired_session_signs_out_and_forgets_the_feeds() {
        let mut state = signed_in();
        with_feeds(&mut state, 0, vec![feed(5, "a")]);
        let fx = update(
            &mut state,
            1,
            Event::FeedsLoaded(Err(ApiError::Unauthorized)),
        );
        assert!(fx.contains(&Effect::DeleteToken));
        assert_eq!(state.feeds, FeedList::default());
        assert_eq!(*state.feed_save(), FeedSave::Idle);
        assert!(!update(&mut state, 10_000_000, Event::Tick).contains(&Effect::LoadFeeds));
        // An answer for the account that is gone does not bring the feeds back.
        update(
            &mut state,
            10_000_001,
            Event::FeedsLoaded(Ok(vec![feed(1, "x")])),
        );
        assert!(state.feeds.items.is_empty());
        // Nor does a save that was on its way when the session ended.
        let fx2 = update(
            &mut state,
            10_000_002,
            Event::FeedSaveRequested {
                id: None,
                settings: settings(),
            },
        );
        assert!(fx2.is_empty(), "not signed in: nothing is sent");
    }

    #[test]
    fn a_save_that_hits_an_expired_session_closes_the_form_for_good() {
        let mut state = signed_in();
        let fx = update(
            &mut state,
            1,
            Event::FeedSaveRequested {
                id: None,
                settings: settings(),
            },
        );
        let out = update(
            &mut state,
            2,
            Event::FeedSaved {
                op: op_of(&fx),
                result: Err(ApiError::Unauthorized),
            },
        );
        assert!(out.contains(&Effect::DeleteToken));
        assert_eq!(*state.feed_save(), FeedSave::Idle);
    }
}
