//! Adding a transfer from a link the user typed or pasted into the *Add a link* card.
//!
//! The card stays open while put.io is asked and shows what it says (so an error is not lost with a notice
//! that fades), and closes when the transfer has been added. Unlike a magnet link opened with the app, this
//! never quits the app afterwards: the user is in the window.

use putio_api::ApiError;

use crate::i18n::Msg;
use crate::link::{self, LinkError};

use super::{Effect, Millis, NOTICE_TIME, Op, OpId, State, session_expired};

/// Where adding the link has got to. The card reads this and acknowledges `Done` with
/// `Event::AddLinkDismissed`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LinkAdd {
    #[default]
    Idle,
    Working,
    /// Refused (by the check here or by put.io); the text is for the user.
    Failed(String),
    Done,
}

pub(super) fn requested(state: &mut State, raw: &str, fx: &mut Vec<Effect>) {
    if !state.is_signed_in() || state.link_add == LinkAdd::Working {
        return;
    }
    match link::parse(raw) {
        Ok(link) => {
            let op = state.op(Op::AddLink);
            state.link_add = LinkAdd::Working;
            fx.push(Effect::AddMagnet {
                op,
                uri: link.uri,
                parent: state.settings.default_folder_id(),
            });
        }
        Err(error) => {
            let msg = match error {
                LinkError::Empty => Msg::LinkErrEmpty,
                LinkError::TooLong => Msg::LinkErrTooLong,
                LinkError::NotALink => Msg::LinkErrNotALink,
            };
            state.link_add = LinkAdd::Failed(state.text(&msg));
        }
    }
}

pub(super) fn finished(
    state: &mut State,
    now: Millis,
    op: OpId,
    result: Result<Option<i64>, ApiError>,
    fx: &mut Vec<Effect>,
) {
    if !state.finish(op, &[Op::AddLink]) {
        return;
    }
    match result {
        Ok(transfer_id) => {
            if let Some(id) = transfer_id {
                state.tracker.note_added(id);
            }
            state.link_add = LinkAdd::Done;
            state.say(Msg::Added, now, NOTICE_TIME);
            state.schedule_poll(now);
        }
        Err(ApiError::Unauthorized) => session_expired(state, fx),
        Err(error) => state.link_add = LinkAdd::Failed(state.reason(&error)),
    }
}

pub(super) fn dismissed(state: &mut State) {
    if state.link_add != LinkAdd::Working {
        state.link_add = LinkAdd::Idle;
    }
}

/// Forget the card (signing out).
pub(super) fn reset(state: &mut State) {
    state.link_add = LinkAdd::Idle;
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
            close_after_magnet: true,
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

    fn op_of(fx: &[Effect]) -> OpId {
        fx.iter()
            .find_map(|e| match e {
                Effect::AddMagnet { op, .. } => Some(*op),
                _ => None,
            })
            .expect("a transfer was asked for")
    }

    #[test]
    fn a_web_address_is_sent_to_put_io_as_it_is_and_the_card_waits_for_the_answer() {
        let mut state = signed_in();
        let fx = update(
            &mut state,
            10,
            Event::AddLinkRequested(" https://example.com/a.mkv?key=1 ".into()),
        );
        assert!(
            fx.iter().any(|e| matches!(e, Effect::AddMagnet { uri, .. } if uri == "https://example.com/a.mkv?key=1")),
            "{fx:?}"
        );
        assert_eq!(state.link_add, LinkAdd::Working);
        assert!(state.is_busy());
        // A second request while the first is on its way does nothing.
        let again = update(
            &mut state,
            11,
            Event::AddLinkRequested("https://example.com/b".into()),
        );
        assert!(again.is_empty(), "{again:?}");
    }

    #[test]
    fn a_text_that_is_no_link_is_refused_in_the_users_words_without_asking_put_io() {
        let mut state = signed_in();
        for (text, said) in [
            ("", "Paste a link first."),
            (
                "hello",
                "That is not a magnet link or a web address (http or https).",
            ),
        ] {
            let fx = update(&mut state, 10, Event::AddLinkRequested(text.into()));
            assert!(fx.is_empty(), "{fx:?}");
            assert_eq!(state.link_add, LinkAdd::Failed(said.into()));
        }
        assert!(!state.is_busy());
        // Putting the message away.
        update(&mut state, 11, Event::AddLinkDismissed);
        assert_eq!(state.link_add, LinkAdd::Idle);
    }

    #[test]
    fn a_transfer_that_was_added_closes_the_card_and_never_quits_the_app() {
        let mut state = signed_in();
        let op = op_of(&update(
            &mut state,
            10,
            Event::AddLinkRequested("https://example.com/a.mkv".into()),
        ));
        let fx = update(
            &mut state,
            20,
            Event::AddFinished {
                op,
                result: Ok(Some(5)),
            },
        );
        assert_eq!(state.link_add, LinkAdd::Done);
        assert_eq!(state.status_text(20), "Added to your transfers!");
        assert!(!state.is_busy());
        // The setting that quits after a magnet link is for links opened from outside, not for this card.
        assert!(state.close_at.is_none(), "must not schedule a quit");
        assert!(!fx.contains(&Effect::Quit), "{fx:?}");
        // The transfer list is asked for at once.
        assert!(fx.contains(&Effect::Poll) || state.poll_in_flight || state.next_poll_at.is_some());
    }

    #[test]
    fn what_put_io_refuses_stays_in_the_card_and_an_expired_session_signs_out() {
        let mut state = signed_in();
        let op = op_of(&update(
            &mut state,
            10,
            Event::AddLinkRequested("https://example.com/a.mkv".into()),
        ));
        update(
            &mut state,
            20,
            Event::AddFinished {
                op,
                result: Err(ApiError::Api(ApiFailure {
                    status: 400,
                    error_type: None,
                    message: "Could not download the link".into(),
                })),
            },
        );
        assert_eq!(
            state.link_add,
            LinkAdd::Failed("Could not download the link".into())
        );
        // It can be tried again.
        let op = op_of(&update(
            &mut state,
            21,
            Event::AddLinkRequested("https://example.com/a.mkv".into()),
        ));
        let fx = update(
            &mut state,
            22,
            Event::AddFinished {
                op,
                result: Err(ApiError::Unauthorized),
            },
        );
        assert!(fx.contains(&Effect::DeleteToken), "{fx:?}");
        assert_eq!(state.link_add, LinkAdd::Idle);
    }

    #[test]
    fn nothing_is_asked_when_signed_out() {
        let mut state = State::new(Settings::default(), Locale::En, "0.1.0");
        let fx = update(
            &mut state,
            0,
            Event::AddLinkRequested("https://example.com/a".into()),
        );
        assert!(fx.is_empty());
        assert_eq!(state.link_add, LinkAdd::Idle);
    }
}
