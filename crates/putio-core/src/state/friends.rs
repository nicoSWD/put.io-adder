//! Friends: the card that lists them and the requests waiting for an answer, and sends, answers and ends
//! friendships.
//!
//! Like sharing it reaches other people, so a change is a deliberate request, one at a time, and what put.io
//! answers is shown as it is. A 401 is a refusal ("put.io does not let this app do that"), never a sign-out:
//! the polls that run all the time are what notice a session that really ended. After a change both lists
//! are asked for again, so the card shows what put.io says.

use putio_api::{ApiError, Friend, FriendChange};

use super::shares::{Fetch, Problem, fetch_of, problem};
use super::{Effect, Op, OpId, State};

/// Where the last change has got to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum FriendAction {
    #[default]
    Idle,
    Working,
    /// Done: the card says so until the next change.
    Done(FriendChange),
    Failed(Problem),
}

/// The open friends card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FriendsView {
    pub friends: Fetch<Vec<Friend>>,
    /// Requests others have sent, waiting for an answer.
    pub requests: Fetch<Vec<Friend>>,
    pub action: FriendAction,
}

pub(super) fn opened(state: &mut State, fx: &mut Vec<Effect>) {
    if !state.is_signed_in() {
        return;
    }
    state.friends_card = Some(FriendsView {
        friends: Fetch::Loading,
        requests: Fetch::Loading,
        action: FriendAction::Idle,
    });
    ask_for_both(fx);
}

fn ask_for_both(fx: &mut Vec<Effect>) {
    fx.push(Effect::FetchFriendList);
    fx.push(Effect::FetchFriendRequests);
}

pub(super) fn closed(state: &mut State) {
    state.friends_card = None;
}

/// A list that is on the card keeps showing when asking for it again fails.
fn keep_or_replace(old: &Fetch<Vec<Friend>>, new: Fetch<Vec<Friend>>) -> Fetch<Vec<Friend>> {
    if matches!(new, Fetch::Ready(_)) || !matches!(old, Fetch::Ready(_)) {
        new
    } else {
        old.clone()
    }
}

pub(super) fn list_fetched(state: &mut State, result: Result<Vec<Friend>, ApiError>) {
    if !state.is_signed_in() {
        return;
    }
    let fetched = fetch_of(state, result);
    if let Some(card) = state.friends_card.as_mut() {
        card.friends = keep_or_replace(&card.friends, fetched);
    }
}

pub(super) fn requests_fetched(state: &mut State, result: Result<Vec<Friend>, ApiError>) {
    if !state.is_signed_in() {
        return;
    }
    let fetched = fetch_of(state, result);
    if let Some(card) = state.friends_card.as_mut() {
        card.requests = keep_or_replace(&card.requests, fetched);
    }
}

/// The name as it will be sent: no spaces around it, nothing that is not text.
fn clean(change: &FriendChange) -> Option<FriendChange> {
    let name = change.name().trim().to_owned();
    if name.is_empty() || name.chars().any(char::is_control) {
        return None;
    }
    Some(match change {
        FriendChange::Request(_) => FriendChange::Request(name),
        FriendChange::Approve(_) => FriendChange::Approve(name),
        FriendChange::Deny(_) => FriendChange::Deny(name),
        FriendChange::Unfriend(_) => FriendChange::Unfriend(name),
    })
}

pub(super) fn change_requested(state: &mut State, change: &FriendChange, fx: &mut Vec<Effect>) {
    if !state.is_signed_in() {
        return;
    }
    let Some(change) = clean(change) else {
        return;
    };
    if state
        .friends_card
        .as_ref()
        .is_none_or(|card| card.action == FriendAction::Working)
    {
        return;
    }
    let op = state.op(Op::Friend);
    if let Some(card) = state.friends_card.as_mut() {
        card.action = FriendAction::Working;
    }
    fx.push(Effect::ChangeFriend { op, change });
}

pub(super) fn change_finished(
    state: &mut State,
    (op, change): (OpId, FriendChange),
    result: Result<(), ApiError>,
    fx: &mut Vec<Effect>,
) {
    if !state.finish(op, &[Op::Friend]) {
        return;
    }
    let outcome = match result {
        Ok(()) => FriendAction::Done(change),
        Err(error) => FriendAction::Failed(problem(state, &error)),
    };
    let changed = matches!(outcome, FriendAction::Done(_));
    if let Some(card) = state.friends_card.as_mut() {
        card.action = outcome;
        if changed {
            ask_for_both(fx);
        }
    }
}

/// Forget the card (signing out).
pub(super) fn reset(state: &mut State) {
    state.friends_card = None;
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

    fn friend(id: i64, name: &str) -> Friend {
        Friend {
            id,
            name: name.into(),
        }
    }

    fn open_card(state: &mut State) {
        update(state, 10, Event::FriendsCardOpened);
        update(
            state,
            11,
            Event::FriendListFetched(Ok(vec![friend(1, "jane")])),
        );
        update(
            state,
            12,
            Event::FriendRequestsFetched(Ok(vec![friend(4, "sam")])),
        );
    }

    fn op_of(fx: &[Effect]) -> OpId {
        fx.iter()
            .find_map(|e| match e {
                Effect::ChangeFriend { op, .. } => Some(*op),
                _ => None,
            })
            .expect("a change was asked for")
    }

    fn card(state: &State) -> &FriendsView {
        state.friends_card.as_ref().expect("the card is open")
    }

    #[test]
    fn opening_the_card_asks_for_the_friends_and_the_requests() {
        let mut state = signed_in();
        let fx = update(&mut state, 10, Event::FriendsCardOpened);
        assert!(fx.contains(&Effect::FetchFriendList), "{fx:?}");
        assert!(fx.contains(&Effect::FetchFriendRequests), "{fx:?}");
        assert_eq!(card(&state).friends, Fetch::Loading);
        update(
            &mut state,
            11,
            Event::FriendListFetched(Ok(vec![friend(1, "jane")])),
        );
        assert_eq!(card(&state).friends, Fetch::Ready(vec![friend(1, "jane")]));
        // Signed out: nothing opens.
        let mut out = State::new(Settings::default(), Locale::En, "0.1.0");
        assert!(update(&mut out, 0, Event::FriendsCardOpened).is_empty());
        assert!(out.friends_card.is_none());
    }

    #[test]
    fn a_change_is_sent_once_and_both_lists_are_asked_for_again_when_put_io_agrees() {
        let mut state = signed_in();
        open_card(&mut state);
        let change = FriendChange::Approve("sam".into());
        let fx = update(&mut state, 20, Event::FriendChangeRequested(change.clone()));
        let op = op_of(&fx);
        assert_eq!(card(&state).action, FriendAction::Working);
        // While it is on its way, another one waits.
        let again = update(
            &mut state,
            21,
            Event::FriendChangeRequested(FriendChange::Deny("x".into())),
        );
        assert!(again.is_empty(), "{again:?}");
        let fx = update(
            &mut state,
            30,
            Event::FriendChangeFinished {
                op,
                change: change.clone(),
                result: Ok(()),
            },
        );
        assert_eq!(card(&state).action, FriendAction::Done(change));
        assert!(fx.contains(&Effect::FetchFriendList), "{fx:?}");
        assert!(fx.contains(&Effect::FetchFriendRequests), "{fx:?}");
        // What is on the card stays until the new lists arrive.
        assert!(matches!(card(&state).requests, Fetch::Ready(_)));
    }

    #[test]
    fn a_name_is_trimmed_and_nothing_is_sent_for_nobody() {
        let mut state = signed_in();
        open_card(&mut state);
        for name in ["", "   ", "a\u{7}b"] {
            let fx = update(
                &mut state,
                20,
                Event::FriendChangeRequested(FriendChange::Request(name.into())),
            );
            assert!(fx.is_empty(), "{name:?}: {fx:?}");
        }
        let fx = update(
            &mut state,
            21,
            Event::FriendChangeRequested(FriendChange::Request("  jane  ".into())),
        );
        assert!(
            fx.iter().any(
                |e| matches!(e, Effect::ChangeFriend { change, .. } if change.name() == "jane")
            ),
            "{fx:?}"
        );
    }

    #[test]
    fn a_refusal_is_told_apart_from_a_failure_and_never_signs_the_user_out() {
        let mut state = signed_in();
        open_card(&mut state);
        let change = FriendChange::Request("jane".into());
        let op = op_of(&update(
            &mut state,
            20,
            Event::FriendChangeRequested(change.clone()),
        ));
        let fx = update(
            &mut state,
            21,
            Event::FriendChangeFinished {
                op,
                change: change.clone(),
                result: Err(ApiError::Unauthorized),
            },
        );
        assert!(!fx.contains(&Effect::DeleteToken), "{fx:?}");
        assert_eq!(state.auth, Auth::SignedIn);
        assert_eq!(
            card(&state).action,
            FriendAction::Failed(Problem::NotAllowed)
        );
        assert!(
            !fx.contains(&Effect::FetchFriendList),
            "nothing changed: {fx:?}"
        );
        let op = op_of(&update(
            &mut state,
            22,
            Event::FriendChangeRequested(change.clone()),
        ));
        update(
            &mut state,
            23,
            Event::FriendChangeFinished {
                op,
                change,
                result: Err(ApiError::Api(ApiFailure {
                    status: 400,
                    error_type: Some("USER_NOT_FOUND".into()),
                    message: "No such user".into(),
                })),
            },
        );
        assert_eq!(
            card(&state).action,
            FriendAction::Failed(Problem::Failed("No such user".into()))
        );
        // A list that is on the card survives a failed refresh; a card without one says why.
        update(
            &mut state,
            24,
            Event::FriendListFetched(Err(ApiError::Network("x".into()))),
        );
        assert!(matches!(card(&state).friends, Fetch::Ready(_)));
        update(&mut state, 25, Event::FriendsCardClosed);
        update(&mut state, 26, Event::FriendsCardOpened);
        update(
            &mut state,
            27,
            Event::FriendRequestsFetched(Err(ApiError::Unauthorized)),
        );
        assert_eq!(card(&state).requests, Fetch::Failed(Problem::NotAllowed));
    }

    #[test]
    fn a_stale_answer_changes_nothing_and_signing_out_forgets_the_card() {
        let mut state = signed_in();
        open_card(&mut state);
        update(
            &mut state,
            20,
            Event::FriendChangeFinished {
                op: OpId(999),
                change: FriendChange::Unfriend("jane".into()),
                result: Ok(()),
            },
        );
        assert_eq!(card(&state).action, FriendAction::Idle);
        update(&mut state, 30, Event::FriendsCardClosed);
        update(&mut state, 31, Event::FriendListFetched(Ok(vec![])));
        assert!(state.friends_card.is_none());
        open_card(&mut state);
        update(&mut state, 40, Event::SignOutRequested);
        assert!(state.friends_card.is_none());
    }
}
