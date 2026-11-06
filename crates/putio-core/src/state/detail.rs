//! The open details panel of a transfer: fetching the transfer by itself, faster than the list is polled,
//! so that peers, speed and ratio move while the panel is looked at.
//!
//! The transfers list is polled every twenty seconds; a panel that waited for that would feel stuck. While
//! one is open, `GET /transfers/{id}` is asked for every few seconds instead. A transfer that put.io no
//! longer knows (cleared or cancelled elsewhere) ends the panel.

use putio_api::{ApiError, Transfer};

use super::{Effect, Millis, State, session_expired};

/// How often the open transfer is fetched.
const REFRESH_INTERVAL: Millis = 3_000;

/// The transfer whose details panel is open.
#[derive(Debug, Clone, PartialEq)]
pub struct TransferDetail {
    pub id: i64,
    /// The transfer as last fetched on its own; the list's copy is used until it arrives.
    pub transfer: Option<Transfer>,
    /// put.io says there is no such transfer any more.
    pub gone: bool,
}

pub(super) fn opened(state: &mut State, now: Millis, id: i64) {
    if state.detail.as_ref().is_some_and(|detail| detail.id == id) {
        return;
    }
    state.detail = Some(TransferDetail {
        id,
        transfer: None,
        gone: false,
    });
    // Whatever was on its way was for another transfer: ask again, now.
    state.detail_refresh.restart(now);
    state.detail_requested = None;
}

pub(super) fn closed(state: &mut State) {
    state.detail = None;
    state.detail_requested = None;
}

/// Ask for the open transfer when it is due. Runs after every event.
pub(super) fn maybe_load(state: &mut State, now: Millis, fx: &mut Vec<Effect>) {
    let Some(detail) = state.detail.as_ref().filter(|detail| !detail.gone) else {
        return;
    };
    if state.is_signed_in() && state.detail_refresh.due(now) {
        let id = detail.id;
        state.detail_refresh.started();
        state.detail_requested = Some(id);
        fx.push(Effect::FetchTransfer { id });
    }
}

pub(super) fn fetched(
    state: &mut State,
    now: Millis,
    id: i64,
    result: Result<Transfer, ApiError>,
    fx: &mut Vec<Effect>,
) {
    // An answer for a panel that was closed or is about another transfer is of no use.
    if !state.is_signed_in()
        || state.detail_requested != Some(id)
        || state.detail.as_ref().is_none_or(|detail| detail.id != id)
    {
        return;
    }
    state.detail_requested = None;
    match result {
        Ok(transfer) => {
            if let Some(detail) = state.detail.as_mut() {
                detail.transfer = Some(transfer);
            }
            state.detail_refresh.succeeded(now, REFRESH_INTERVAL);
        }
        Err(ApiError::Unauthorized) => session_expired(state, fx),
        Err(ApiError::Api(failure)) if failure.status == 404 => {
            if let Some(detail) = state.detail.as_mut() {
                detail.gone = true;
            }
            state.detail_refresh.answered();
        }
        Err(error) => state.detail_refresh.failed(now, &error),
    }
}

/// Forget the open panel (signing out).
pub(super) fn reset(state: &mut State) {
    state.detail = None;
    state.detail_requested = None;
    state.detail_refresh = super::refresh::Refresh::default();
}

impl State {
    /// The transfer the details panel shows: the one fetched on its own when there is one, else the list's
    /// copy. `None` when there is no panel, or put.io says the transfer is gone.
    #[must_use]
    pub fn detail_transfer(&self) -> Option<&Transfer> {
        let detail = self.detail.as_ref().filter(|detail| !detail.gone)?;
        detail
            .transfer
            .as_ref()
            .or_else(|| self.transfers.iter().find(|t| t.id == detail.id))
    }
}

#[cfg(test)]
mod tests {
    use putio_api::{ApiFailure, TransferStatus};

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
        let listed = Transfer {
            id: 5,
            name: "from the list".into(),
            status: TransferStatus::Downloading,
            ..Transfer::default()
        };
        update(
            &mut state,
            0,
            Event::Polled {
                account: Ok(putio_api::Account::default()),
                transfers: Ok(vec![listed]),
            },
        );
        assert_eq!(state.auth, Auth::SignedIn);
        state
    }

    fn fetched_transfer(id: i64, peers: i64) -> Transfer {
        Transfer {
            id,
            name: "fetched".into(),
            peers: Some(peers),
            ..Transfer::default()
        }
    }

    fn fetches(fx: &[Effect]) -> usize {
        fx.iter()
            .filter(|e| matches!(e, Effect::FetchTransfer { .. }))
            .count()
    }

    #[test]
    fn nothing_is_fetched_without_a_panel_and_an_open_panel_fetches_once_at_once() {
        let mut state = signed_in();
        assert_eq!(fetches(&update(&mut state, 10_000, Event::Tick)), 0);
        let fx = update(&mut state, 10_001, Event::TransferDetailsOpened(5));
        assert!(fx.contains(&Effect::FetchTransfer { id: 5 }), "{fx:?}");
        // One request at a time.
        assert_eq!(fetches(&update(&mut state, 10_002, Event::Tick)), 0);
    }

    #[test]
    fn the_list_copy_is_shown_until_the_fetched_one_arrives_and_it_is_refreshed_every_few_seconds()
    {
        let mut state = signed_in();
        update(&mut state, 100, Event::TransferDetailsOpened(5));
        assert_eq!(
            state.detail_transfer().map(|t| t.name.as_str()),
            Some("from the list")
        );
        update(
            &mut state,
            150,
            Event::TransferDetailsFetched {
                id: 5,
                result: Ok(fetched_transfer(5, 12)),
            },
        );
        let shown = state.detail_transfer().unwrap();
        assert_eq!((shown.name.as_str(), shown.peers), ("fetched", Some(12)));
        assert_eq!(fetches(&update(&mut state, 3_000, Event::Tick)), 0);
        assert_eq!(fetches(&update(&mut state, 3_150, Event::Tick)), 1);
        // Closing the panel stops it.
        update(
            &mut state,
            3_200,
            Event::TransferDetailsFetched {
                id: 5,
                result: Ok(fetched_transfer(5, 13)),
            },
        );
        update(&mut state, 3_300, Event::TransferDetailsClosed);
        assert!(state.detail_transfer().is_none());
        assert_eq!(fetches(&update(&mut state, 60_000, Event::Tick)), 0);
    }

    #[test]
    fn an_answer_for_another_transfer_or_a_closed_panel_changes_nothing() {
        let mut state = signed_in();
        update(&mut state, 100, Event::TransferDetailsOpened(5));
        // The user looks at another transfer before the first answer arrives.
        let fx = update(&mut state, 110, Event::TransferDetailsOpened(6));
        assert!(fx.contains(&Effect::FetchTransfer { id: 6 }), "{fx:?}");
        update(
            &mut state,
            120,
            Event::TransferDetailsFetched {
                id: 5,
                result: Ok(fetched_transfer(5, 99)),
            },
        );
        assert!(state.detail.as_ref().unwrap().transfer.is_none());
        update(&mut state, 130, Event::TransferDetailsClosed);
        update(
            &mut state,
            140,
            Event::TransferDetailsFetched {
                id: 6,
                result: Ok(fetched_transfer(6, 1)),
            },
        );
        assert!(state.detail.is_none());
        // Opening the panel that is already open does not start over.
        update(&mut state, 150, Event::TransferDetailsOpened(6));
        assert_eq!(
            fetches(&update(&mut state, 151, Event::TransferDetailsOpened(6))),
            0
        );
    }

    #[test]
    fn a_transfer_put_io_no_longer_knows_ends_the_panel_without_more_requests() {
        let mut state = signed_in();
        update(&mut state, 100, Event::TransferDetailsOpened(5));
        update(
            &mut state,
            110,
            Event::TransferDetailsFetched {
                id: 5,
                result: Err(ApiError::Api(ApiFailure {
                    status: 404,
                    error_type: Some("NotFound".into()),
                    message: "gone".into(),
                })),
            },
        );
        assert!(state.detail.as_ref().unwrap().gone);
        assert!(state.detail_transfer().is_none());
        assert_eq!(fetches(&update(&mut state, 60_000, Event::Tick)), 0);
    }

    #[test]
    fn a_failed_fetch_keeps_what_was_shown_and_is_retried_with_a_wait() {
        let mut state = signed_in();
        update(&mut state, 100, Event::TransferDetailsOpened(5));
        update(
            &mut state,
            110,
            Event::TransferDetailsFetched {
                id: 5,
                result: Ok(fetched_transfer(5, 7)),
            },
        );
        assert_eq!(fetches(&update(&mut state, 3_200, Event::Tick)), 1);
        update(
            &mut state,
            3_400,
            Event::TransferDetailsFetched {
                id: 5,
                result: Err(ApiError::Network("x".into())),
            },
        );
        assert_eq!(
            state.detail_transfer().unwrap().peers,
            Some(7),
            "the last good copy stays"
        );
        assert_eq!(fetches(&update(&mut state, 8_000, Event::Tick)), 0);
        assert_eq!(fetches(&update(&mut state, 13_500, Event::Tick)), 1);
    }

    #[test]
    fn an_expired_session_closes_the_panel_and_stops_fetching() {
        let mut state = signed_in();
        update(&mut state, 100, Event::TransferDetailsOpened(5));
        let fx = update(
            &mut state,
            110,
            Event::TransferDetailsFetched {
                id: 5,
                result: Err(ApiError::Unauthorized),
            },
        );
        assert!(fx.contains(&Effect::DeleteToken));
        assert!(state.detail.is_none());
        assert_eq!(fetches(&update(&mut state, 60_000, Event::Tick)), 0);
        // Not signed in: opening a panel asks for nothing.
        assert_eq!(
            fetches(&update(&mut state, 61_000, Event::TransferDetailsOpened(5))),
            0
        );
    }
}
