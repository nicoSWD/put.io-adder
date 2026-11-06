//! Sharing files with friends: the card that shows who a file is shared with and shares it with more,
//! and the set of shared files that marks them in the Files list.
//!
//! Sharing reaches other people, so nothing happens without a deliberate request, one request at a time,
//! and what put.io answers is shown as it is. A refused call (put.io answers 401 when a token may not make
//! it) is told apart from a failed one, and never signs the user out: the polls that run all the time are
//! what find out that a session has really ended.

use std::collections::HashSet;

use putio_api::{ApiError, Friend, Share, ShareWith, SharedFile};

use crate::settings::Tab;

use super::refresh::Refresh;
use super::{Effect, Millis, Op, OpId, State};

/// How old the set of shared files may get while the Files tab is open.
const REFRESH_INTERVAL: Millis = 60_000;

/// Why something about sharing did not work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// put.io does not let this app do it.
    NotAllowed,
    /// Anything else, in the user's words.
    Failed(String),
}

/// Something fetched from put.io for the card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetch<T> {
    Loading,
    Ready(T),
    Failed(Problem),
}

/// What sharing or unsharing from the card has come to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ShareAction {
    #[default]
    Idle,
    Working,
    Failed(Problem),
}

/// The open sharing card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharingView {
    pub file_id: i64,
    pub file_name: String,
    /// Who the file is shared with now.
    pub shares: Fetch<Vec<Share>>,
    /// Who it could be shared with.
    pub friends: Fetch<Vec<Friend>>,
    pub action: ShareAction,
}

/// The files the account shares, to mark them in the list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SharedFiles {
    ids: HashSet<i64>,
    /// put.io refuses to say: not asked again.
    denied: bool,
}

impl State {
    /// Whether put.io says the file is shared with anyone.
    #[must_use]
    pub fn is_shared(&self, file_id: i64) -> bool {
        self.shared.ids.contains(&file_id)
    }
}

fn problem(state: &State, error: &ApiError) -> Problem {
    match error {
        ApiError::Unauthorized => Problem::NotAllowed,
        other => Problem::Failed(state.reason(other)),
    }
}

fn fetch_of<T>(state: &State, result: Result<T, ApiError>) -> Fetch<T> {
    match result {
        Ok(value) => Fetch::Ready(value),
        Err(error) => Fetch::Failed(problem(state, &error)),
    }
}

pub(super) fn opened(state: &mut State, file_id: i64, name: &str, fx: &mut Vec<Effect>) {
    if !state.is_signed_in() || file_id <= 0 {
        return;
    }
    state.sharing = Some(SharingView {
        file_id,
        file_name: name.chars().filter(|c| !c.is_control()).collect(),
        shares: Fetch::Loading,
        friends: Fetch::Loading,
        action: ShareAction::Idle,
    });
    fx.push(Effect::FetchSharedWith { file_id });
    fx.push(Effect::FetchFriends { file_id });
}

pub(super) fn closed(state: &mut State) {
    state.sharing = None;
}

pub(super) fn shares_fetched(
    state: &mut State,
    file_id: i64,
    result: Result<Vec<Share>, ApiError>,
) {
    if !state.is_signed_in() || state.sharing.as_ref().is_none_or(|v| v.file_id != file_id) {
        return;
    }
    let fetched = fetch_of(state, result);
    if let Some(view) = state.sharing.as_mut() {
        // A card that has the list already keeps it when asking again fails.
        if matches!(fetched, Fetch::Ready(_)) || !matches!(view.shares, Fetch::Ready(_)) {
            view.shares = fetched;
        }
    }
    // What the card learned about this file is what the Files list should say about it.
    let shared = match state.sharing.as_ref().map(|v| &v.shares) {
        Some(Fetch::Ready(list)) => Some(!list.is_empty()),
        _ => None,
    };
    match shared {
        Some(true) => {
            state.shared.ids.insert(file_id);
        }
        Some(false) => {
            state.shared.ids.remove(&file_id);
        }
        None => {}
    }
}

pub(super) fn friends_fetched(
    state: &mut State,
    file_id: i64,
    result: Result<Vec<Friend>, ApiError>,
) {
    if !state.is_signed_in() || state.sharing.as_ref().is_none_or(|v| v.file_id != file_id) {
        return;
    }
    let fetched = fetch_of(state, result);
    if let Some(view) = state.sharing.as_mut() {
        view.friends = fetched;
    }
}

pub(super) fn share_requested(state: &mut State, with: ShareWith, fx: &mut Vec<Effect>) {
    if !state.is_signed_in() || with.as_field().is_none() {
        return;
    }
    let Some(file_id) = state
        .sharing
        .as_ref()
        .filter(|view| view.action != ShareAction::Working)
        .map(|view| view.file_id)
    else {
        return;
    };
    let op = state.op(Op::Share);
    if let Some(view) = state.sharing.as_mut() {
        view.action = ShareAction::Working;
    }
    fx.push(Effect::ShareFile { op, file_id, with });
}

pub(super) fn share_finished(
    state: &mut State,
    (op, file_id): (OpId, i64),
    result: Result<(), ApiError>,
    fx: &mut Vec<Effect>,
) {
    if !state.finish(op, &[Op::Share]) {
        return;
    }
    settle(state, file_id, result, fx);
}

pub(super) fn unshare_requested(state: &mut State, share_id: i64, fx: &mut Vec<Effect>) {
    if !state.is_signed_in() {
        return;
    }
    let Some(file_id) = state
        .sharing
        .as_ref()
        .filter(|view| view.action != ShareAction::Working)
        .filter(|view| matches!(&view.shares, Fetch::Ready(list) if list.iter().any(|s| s.share_id == share_id)))
        .map(|view| view.file_id)
    else {
        return;
    };
    let op = state.op(Op::Unshare);
    if let Some(view) = state.sharing.as_mut() {
        view.action = ShareAction::Working;
    }
    fx.push(Effect::UnshareFile {
        op,
        file_id,
        share_id,
    });
}

pub(super) fn unshare_finished(
    state: &mut State,
    (op, file_id): (OpId, i64),
    result: Result<(), ApiError>,
    fx: &mut Vec<Effect>,
) {
    if !state.finish(op, &[Op::Unshare]) {
        return;
    }
    settle(state, file_id, result, fx);
}

/// A share or an unshare is over: say how it went, and look again at who the file is shared with, since
/// that is what put.io now says (and what is shown is not guessed).
fn settle(state: &mut State, file_id: i64, result: Result<(), ApiError>, fx: &mut Vec<Effect>) {
    let outcome = match result {
        Ok(()) => ShareAction::Idle,
        Err(error) => ShareAction::Failed(problem(state, &error)),
    };
    let refused = !matches!(outcome, ShareAction::Idle);
    if let Some(view) = state.sharing.as_mut().filter(|v| v.file_id == file_id) {
        view.action = outcome;
        if !refused {
            // Keep the old list on screen until the new one arrives.
            fx.push(Effect::FetchSharedWith { file_id });
        }
    }
    if !refused {
        state.shared_refresh.soon(0);
    }
}

// ---- the set of shared files -----------------------------------------------------------------

fn wanted(state: &State) -> bool {
    state.is_signed_in()
        && state.transfers_expanded
        && state.tab == Tab::Files
        && !state.shared.denied
}

/// Ask for the shared files when the Files tab is on screen and it is time. Runs after every event.
pub(super) fn maybe_load(state: &mut State, now: Millis, fx: &mut Vec<Effect>) {
    if wanted(state) && state.shared_refresh.due(now) {
        state.shared_refresh.started();
        fx.push(Effect::ListShared);
    }
}

pub(super) fn listed(state: &mut State, now: Millis, result: Result<Vec<SharedFile>, ApiError>) {
    if !state.is_signed_in() {
        return;
    }
    match result {
        Ok(list) => {
            state.shared.ids = list.iter().map(|f| f.id).collect();
            state.shared_refresh.succeeded(now, REFRESH_INTERVAL);
        }
        // The marks are a courtesy: when put.io does not let this app see them, stop asking.
        Err(ApiError::Unauthorized) => {
            state.shared.denied = true;
            state.shared_refresh.answered();
        }
        Err(error) => state.shared_refresh.failed(now, &error),
    }
}

/// Forget everything about sharing (signing out).
pub(super) fn reset(state: &mut State) {
    state.sharing = None;
    state.shared = SharedFiles::default();
    state.shared_refresh = Refresh::default();
}

#[cfg(test)]
mod tests {
    use putio_api::{ApiFailure, Transfer};

    use super::super::{Auth, Event};
    use super::*;
    use crate::i18n::Locale;
    use crate::settings::Settings;
    use crate::state::update;

    fn signed_in() -> State {
        let settings = Settings {
            check_for_updates: false,
            ..Settings::default()
        };
        let mut state = State::new(settings, Locale::En, "0.1.0");
        update(&mut state, 0, Event::Started { has_token: true });
        update(
            &mut state,
            0,
            Event::Polled {
                account: Ok(putio_api::Account::default()),
                transfers: Ok(Vec::<Transfer>::new()),
            },
        );
        assert_eq!(state.auth, Auth::SignedIn);
        state
    }

    fn share(id: i64, name: &str) -> Share {
        Share {
            share_id: id,
            user_name: name.into(),
        }
    }

    fn friend(id: i64, name: &str) -> Friend {
        Friend {
            id,
            name: name.into(),
        }
    }

    fn api_error(status: u16) -> ApiError {
        ApiError::Api(ApiFailure {
            status,
            error_type: None,
            message: "nope".into(),
        })
    }

    /// A card for file 5 whose lists have arrived.
    fn open_card(state: &mut State) {
        update(
            state,
            10,
            Event::ShareOpened {
                file_id: 5,
                name: "Movies".into(),
            },
        );
        update(
            state,
            11,
            Event::SharedWithFetched {
                file_id: 5,
                result: Ok(vec![share(30, "jane")]),
            },
        );
        update(
            state,
            12,
            Event::FriendsFetched {
                file_id: 5,
                result: Ok(vec![friend(1, "jane"), friend(2, "john")]),
            },
        );
    }

    fn op_of(fx: &[Effect]) -> OpId {
        fx.iter()
            .find_map(|e| match e {
                Effect::ShareFile { op, .. } | Effect::UnshareFile { op, .. } => Some(*op),
                _ => None,
            })
            .expect("a share or unshare was asked for")
    }

    #[test]
    fn opening_the_card_asks_for_who_it_is_shared_with_and_for_the_friends() {
        let mut state = signed_in();
        let fx = update(
            &mut state,
            10,
            Event::ShareOpened {
                file_id: 5,
                name: "Mo\u{7}vies".into(),
            },
        );
        assert!(
            fx.contains(&Effect::FetchSharedWith { file_id: 5 }),
            "{fx:?}"
        );
        assert!(fx.contains(&Effect::FetchFriends { file_id: 5 }), "{fx:?}");
        let view = state.sharing.as_ref().unwrap();
        assert_eq!(view.file_name, "Movies");
        assert_eq!(
            (&view.shares, &view.friends),
            (&Fetch::Loading, &Fetch::Loading)
        );
        // Signed out: nothing opens.
        let mut out = State::new(Settings::default(), Locale::En, "0.1.0");
        let fx = update(
            &mut out,
            0,
            Event::ShareOpened {
                file_id: 5,
                name: "x".into(),
            },
        );
        assert!(fx.is_empty() && out.sharing.is_none());
    }

    #[test]
    fn answers_for_another_file_or_a_closed_card_are_ignored() {
        let mut state = signed_in();
        update(
            &mut state,
            10,
            Event::ShareOpened {
                file_id: 5,
                name: "a".into(),
            },
        );
        update(
            &mut state,
            11,
            Event::SharedWithFetched {
                file_id: 9,
                result: Ok(vec![share(1, "x")]),
            },
        );
        assert_eq!(state.sharing.as_ref().unwrap().shares, Fetch::Loading);
        assert!(!state.is_shared(9));
        update(&mut state, 12, Event::ShareClosed);
        update(
            &mut state,
            13,
            Event::FriendsFetched {
                file_id: 5,
                result: Ok(vec![friend(1, "jane")]),
            },
        );
        assert!(state.sharing.is_none());
    }

    #[test]
    fn sharing_asks_once_asks_again_who_it_is_shared_with_and_shows_what_put_io_says() {
        let mut state = signed_in();
        open_card(&mut state);
        let with = ShareWith::Friends(vec!["john".into()]);
        let fx = update(&mut state, 20, Event::ShareRequested(with.clone()));
        let op = op_of(&fx);
        assert_eq!(state.sharing.as_ref().unwrap().action, ShareAction::Working);
        // A second request while the first is on its way does nothing.
        let again = update(&mut state, 21, Event::ShareRequested(ShareWith::Everyone));
        assert!(again.is_empty(), "{again:?}");
        let fx = update(
            &mut state,
            30,
            Event::ShareFinished {
                op,
                file_id: 5,
                result: Ok(()),
            },
        );
        assert_eq!(state.sharing.as_ref().unwrap().action, ShareAction::Idle);
        assert!(
            fx.contains(&Effect::FetchSharedWith { file_id: 5 }),
            "{fx:?}"
        );
        // The old list stays until the new one arrives.
        assert!(matches!(
            state.sharing.as_ref().unwrap().shares,
            Fetch::Ready(_)
        ));
        update(
            &mut state,
            31,
            Event::SharedWithFetched {
                file_id: 5,
                result: Ok(vec![share(30, "jane"), share(31, "john")]),
            },
        );
        let Fetch::Ready(list) = &state.sharing.as_ref().unwrap().shares else {
            panic!("the list is there")
        };
        assert_eq!(list.len(), 2);
        assert!(state.is_shared(5));
    }

    #[test]
    fn nobody_is_not_shared_with() {
        let mut state = signed_in();
        open_card(&mut state);
        for with in [
            ShareWith::Friends(vec![]),
            ShareWith::Friends(vec!["a,b".into()]),
        ] {
            let fx = update(&mut state, 20, Event::ShareRequested(with));
            assert!(fx.is_empty(), "{fx:?}");
        }
        assert_eq!(state.sharing.as_ref().unwrap().action, ShareAction::Idle);
    }

    #[test]
    fn unsharing_works_only_for_a_share_the_card_shows_and_ends_with_the_file_unmarked() {
        let mut state = signed_in();
        open_card(&mut state);
        assert!(state.is_shared(5));
        let fx = update(&mut state, 20, Event::UnshareRequested(999));
        assert!(
            fx.is_empty(),
            "a share that is not listed is not sent: {fx:?}"
        );
        let fx = update(&mut state, 21, Event::UnshareRequested(30));
        let op = op_of(&fx);
        assert!(
            fx.contains(&Effect::UnshareFile {
                op,
                file_id: 5,
                share_id: 30
            }),
            "{fx:?}"
        );
        update(
            &mut state,
            30,
            Event::UnshareFinished {
                op,
                file_id: 5,
                result: Ok(()),
            },
        );
        update(
            &mut state,
            31,
            Event::SharedWithFetched {
                file_id: 5,
                result: Ok(vec![]),
            },
        );
        assert!(!state.is_shared(5));
    }

    #[test]
    fn a_refusal_is_told_apart_from_a_failure_and_never_signs_the_user_out() {
        let mut state = signed_in();
        open_card(&mut state);
        let op = op_of(&update(
            &mut state,
            20,
            Event::ShareRequested(ShareWith::Everyone),
        ));
        let fx = update(
            &mut state,
            21,
            Event::ShareFinished {
                op,
                file_id: 5,
                result: Err(ApiError::Unauthorized),
            },
        );
        assert!(!fx.contains(&Effect::DeleteToken), "{fx:?}");
        assert_eq!(state.auth, Auth::SignedIn);
        assert_eq!(
            state.sharing.as_ref().unwrap().action,
            ShareAction::Failed(Problem::NotAllowed)
        );
        // Nothing was changed, so nothing is asked again.
        assert!(!fx.contains(&Effect::FetchSharedWith { file_id: 5 }));
        let op = op_of(&update(
            &mut state,
            22,
            Event::ShareRequested(ShareWith::Everyone),
        ));
        update(
            &mut state,
            23,
            Event::ShareFinished {
                op,
                file_id: 5,
                result: Err(api_error(400)),
            },
        );
        assert_eq!(
            state.sharing.as_ref().unwrap().action,
            ShareAction::Failed(Problem::Failed("nope".into()))
        );
        // A card whose lists were refused says so; one that has its list keeps it when asking again fails.
        update(
            &mut state,
            24,
            Event::SharedWithFetched {
                file_id: 5,
                result: Err(ApiError::Network("x".into())),
            },
        );
        assert!(matches!(
            state.sharing.as_ref().unwrap().shares,
            Fetch::Ready(_)
        ));
        update(
            &mut state,
            25,
            Event::FriendsFetched {
                file_id: 5,
                result: Err(ApiError::Unauthorized),
            },
        );
        assert_eq!(
            state.sharing.as_ref().unwrap().friends,
            Fetch::Failed(Problem::NotAllowed)
        );
    }

    #[test]
    fn the_shared_files_are_asked_for_only_while_the_files_tab_is_open_and_not_after_a_refusal() {
        use crate::settings::Tab;
        let mut state = signed_in();
        state.transfers_expanded = true;
        let asks = |fx: &[Effect]| fx.iter().filter(|e| **e == Effect::ListShared).count();
        assert_eq!(asks(&update(&mut state, 1_000, Event::Tick)), 0);
        let fx = update(&mut state, 2_000, Event::SelectTab(Tab::Files));
        assert_eq!(asks(&fx), 1, "{fx:?}");
        update(
            &mut state,
            2_100,
            Event::SharedListed(Ok(vec![SharedFile {
                id: 7,
                name: "x".into(),
            }])),
        );
        assert!(state.is_shared(7) && !state.is_shared(8));
        assert_eq!(asks(&update(&mut state, 30_000, Event::Tick)), 0);
        assert_eq!(asks(&update(&mut state, 63_000, Event::Tick)), 1);
        // put.io refusing to say stops it for good, and keeps the user signed in.
        let fx = update(
            &mut state,
            63_100,
            Event::SharedListed(Err(ApiError::Unauthorized)),
        );
        assert!(!fx.contains(&Effect::DeleteToken), "{fx:?}");
        assert_eq!(asks(&update(&mut state, 500_000, Event::Tick)), 0);
        assert_eq!(state.auth, Auth::SignedIn);
    }

    #[test]
    fn signing_out_forgets_who_is_shared_with() {
        let mut state = signed_in();
        open_card(&mut state);
        assert!(state.is_shared(5));
        update(&mut state, 50, Event::SignOutRequested);
        assert!(state.sharing.is_none() && !state.is_shared(5));
    }
}
