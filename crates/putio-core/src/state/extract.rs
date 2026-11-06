//! Unpacking ZIP and RAR archives on put.io: starting one, following it until it is done, and asking for a
//! password when put.io needs one.
//!
//! put.io does the unpacking; the app asks for it, polls the list of running extractions every couple of
//! seconds while there is one, and says how it went. What an extraction produced is in the folder next to the
//! archive, so a finished one is followed by a fresh listing.

use putio_api::{ApiError, ExtractStatus, Extraction};

use crate::files::display_name;
use crate::i18n::Msg;
use crate::secret::Secret;

use super::refresh::Refresh;
use super::{
    ERROR_NOTICE_TIME, Effect, Millis, NOTICE_TIME, Op, OpId, State, files, session_expired,
};

/// How often the running extractions are looked at.
const POLL_INTERVAL: Millis = 2_000;
/// The longest reason shown for a failed extraction.
const MAX_REASON_CHARS: usize = 200;

/// An archive being unpacked, as far as the app knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unpacking {
    /// The archive.
    pub file_id: i64,
    pub name: String,
    pub status: ExtractStatus,
    /// put.io's id for the extraction, once it has said.
    extraction_id: Option<i64>,
    /// put.io has answered the request (before that, an empty list does not mean "finished").
    started: bool,
}

/// An archive that needs a password the app has not been given (or was given wrong).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordAsk {
    pub file_id: i64,
    pub name: String,
}

impl Unpacking {
    /// put.io is working on it (and has said so): worth asking about.
    fn is_running(&self) -> bool {
        self.started
            && matches!(
                self.status,
                ExtractStatus::InQueue | ExtractStatus::Extracting
            )
    }

    /// Still being worked on (as opposed to waiting for the user).
    fn is_active(&self) -> bool {
        !self.started
            || matches!(
                self.status,
                ExtractStatus::InQueue | ExtractStatus::Extracting
            )
    }
}

fn file_name(state: &State, id: i64) -> String {
    let files = &state.files;
    files
        .items
        .iter()
        .chain(files.search.iter().flat_map(|search| &search.items))
        .find(|file| file.id == id)
        .map_or_else(|| "…".to_owned(), display_name)
}

pub(super) fn requested(
    state: &mut State,
    id: i64,
    password: Option<Secret>,
    fx: &mut Vec<Effect>,
) {
    if !state.is_signed_in() || id < 0 {
        return;
    }
    // One extraction of an archive at a time (one waiting for a password may be asked again).
    if state
        .unpacking
        .iter()
        .any(|item| item.file_id == id && item.is_active())
    {
        return;
    }
    let name = file_name(state, id);
    state.unpacking.retain(|item| item.file_id != id);
    state.unpacking.push(Unpacking {
        file_id: id,
        name: name.clone(),
        status: ExtractStatus::InQueue,
        extraction_id: None,
        started: false,
    });
    if state
        .extract_ask
        .as_ref()
        .is_some_and(|ask| ask.file_id == id)
    {
        state.extract_ask = None;
    }
    let op = state.op(Op::Extract);
    say_progress(state, name);
    fx.push(Effect::ExtractFile {
        op,
        id,
        password: password.filter(|secret| !secret.is_empty()),
    });
}

/// What the status line says while archives are being unpacked.
fn say_progress(state: &mut State, name: String) {
    let active = state
        .unpacking
        .iter()
        .filter(|item| item.is_active())
        .count();
    let msg = if active > 1 {
        Msg::ExtractingMany { count: active }
    } else {
        Msg::Extracting { name }
    };
    state.say_until_replaced(msg);
}

pub(super) fn started(
    state: &mut State,
    now: Millis,
    (op, id): (OpId, i64),
    result: Result<Vec<Extraction>, ApiError>,
    fx: &mut Vec<Effect>,
) {
    if !state.finish(op, &[Op::Extract]) {
        return;
    }
    match result {
        Ok(list) => {
            if let Some(item) = state.unpacking.iter_mut().find(|item| item.file_id == id) {
                item.started = true;
            }
            update_from(state, &list);
            settle(state, now, &list, fx);
            state
                .extract_refresh
                .soon(now.saturating_add(POLL_INTERVAL));
        }
        Err(ApiError::Unauthorized) => session_expired(state, fx),
        Err(error) => {
            let name = state
                .unpacking
                .iter()
                .find(|item| item.file_id == id)
                .map_or_else(|| "…".to_owned(), |item| item.name.clone());
            state.unpacking.retain(|item| item.file_id != id);
            let reason = state.reason(&error);
            state.say(Msg::ExtractFailed { name, reason }, now, ERROR_NOTICE_TIME);
        }
    }
}

/// Ask for the running extractions when there are any. Runs after every event.
pub(super) fn maybe_poll(state: &mut State, now: Millis, fx: &mut Vec<Effect>) {
    if state.is_signed_in()
        && state.unpacking.iter().any(Unpacking::is_running)
        && state.extract_refresh.due(now)
    {
        state.extract_refresh.started();
        fx.push(Effect::ListExtractions);
    }
}

pub(super) fn listed(
    state: &mut State,
    now: Millis,
    result: Result<Vec<Extraction>, ApiError>,
    fx: &mut Vec<Effect>,
) {
    if !state.is_signed_in() {
        state.extract_refresh.answered();
        return;
    }
    match result {
        Ok(list) => {
            state.extract_refresh.succeeded(now, POLL_INTERVAL);
            update_from(state, &list);
            settle(state, now, &list, fx);
        }
        Err(ApiError::Unauthorized) => session_expired(state, fx),
        Err(error) => state.extract_refresh.failed(now, &error),
    }
}

/// Take what put.io says about the extractions we follow.
fn update_from(state: &mut State, list: &[Extraction]) {
    for item in &mut state.unpacking {
        let known = list.iter().find(|extraction| {
            item.extraction_id == Some(extraction.id) || extraction.files.contains(&item.file_id)
        });
        if let Some(extraction) = known {
            item.extraction_id = Some(extraction.id);
            item.status = extraction.status;
        }
    }
}

/// Finish what is finished: an extraction that is done, failed, gone from put.io's list, or waiting for a
/// password, is no longer being followed (or is asked about).
fn settle(state: &mut State, now: Millis, list: &[Extraction], fx: &mut Vec<Effect>) {
    let mut finished = false;
    let mut said = false;
    let items = std::mem::take(&mut state.unpacking);
    for item in items {
        let listed = list.iter().find(|extraction| {
            item.extraction_id == Some(extraction.id) || extraction.files.contains(&item.file_id)
        });
        match (item.status, listed) {
            (ExtractStatus::Extracted, _) | (_, None) if item.started => {
                // Done (or no longer in put.io's list of running ones, which is the same thing).
                state.say(Msg::Extracted { name: item.name }, now, NOTICE_TIME);
                finished = true;
                said = true;
            }
            (ExtractStatus::Error, extraction) => {
                let reason = extraction
                    .and_then(|extraction| extraction.message.as_deref())
                    .map(|message| {
                        message
                            .chars()
                            .filter(|c| !c.is_control())
                            .take(MAX_REASON_CHARS)
                            .collect::<String>()
                    })
                    .filter(|reason| !reason.trim().is_empty())
                    .unwrap_or_else(|| state.text(&Msg::ServerError));
                state.say(
                    Msg::ExtractFailed {
                        name: item.name,
                        reason,
                    },
                    now,
                    ERROR_NOTICE_TIME,
                );
                said = true;
            }
            (ExtractStatus::Password, _) => {
                if state.extract_ask.is_none() {
                    state.extract_ask = Some(PasswordAsk {
                        file_id: item.file_id,
                        name: item.name.clone(),
                    });
                }
                state.unpacking.push(item);
            }
            _ => state.unpacking.push(item),
        }
    }
    if finished {
        files::refresh_everything(state, now, fx);
    }
    // "Extracting…" is not true any more when nothing is being worked on (what is left waits for a
    // password, or there is nothing left) and nothing else was said.
    if !said && !state.unpacking.iter().any(Unpacking::is_active) {
        state.notice = None;
    }
}

/// The user gave up on a password: the archive is not followed any more.
pub(super) fn password_dismissed(state: &mut State) {
    if let Some(ask) = state.extract_ask.take() {
        state.unpacking.retain(|item| item.file_id != ask.file_id);
        if !state.unpacking.iter().any(Unpacking::is_active) {
            state.notice = None;
        }
    }
}

/// Forget everything about the account's extractions (signing out).
pub(super) fn reset(state: &mut State) {
    state.unpacking.clear();
    state.extract_ask = None;
    state.extract_refresh = Refresh::default();
}

#[cfg(test)]
mod tests {
    use putio_api::{ApiFailure, File, FileType};

    use super::super::{Auth, Event};
    use super::*;
    use crate::i18n::Locale;
    use crate::settings::{Settings, Tab};
    use crate::state::update;

    fn signed_in() -> State {
        let settings = Settings {
            check_for_updates: false,
            transfers_expanded: true,
            ..Settings::default()
        };
        let mut state = State::new(settings, Locale::En, "0.1.0");
        update(&mut state, 0, Event::Started { has_token: true });
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

    fn archive(id: i64, name: &str) -> File {
        File {
            id,
            name: name.into(),
            parent_id: Some(0),
            file_type: FileType::Archive,
            content_type: String::new(),
            size: 10,
            screenshot: None,
            created_at: None,
        }
    }

    fn extraction(id: i64, status: ExtractStatus, files: &[i64]) -> Extraction {
        Extraction {
            id,
            name: "x".into(),
            status,
            message: None,
            files: files.to_vec(),
        }
    }

    fn op_of(fx: &[Effect]) -> OpId {
        fx.iter()
            .find_map(|e| match e {
                Effect::ExtractFile { op, .. } => Some(*op),
                _ => None,
            })
            .expect("an extract effect")
    }

    fn polls(fx: &[Effect]) -> usize {
        fx.iter().filter(|e| **e == Effect::ListExtractions).count()
    }

    /// Show a folder with `a.zip` (id 7) in it, then start extracting it; returns the start's effect list.
    fn started_extraction(state: &mut State) -> Vec<Effect> {
        update(state, 0, Event::SelectTab(Tab::Files));
        update(
            state,
            0,
            Event::FilesListed {
                parent: 0,
                result: Ok(putio_api::client::FileListing {
                    parent: None,
                    files: vec![archive(7, "a.zip")],
                }),
            },
        );
        update(
            state,
            10,
            Event::FileExtractRequested {
                id: 7,
                password: None,
            },
        )
    }

    #[test]
    fn extracting_names_the_archive_and_is_followed_until_it_is_done() {
        let mut state = signed_in();
        let fx = started_extraction(&mut state);
        assert!(
            matches!(
                &fx[..],
                [Effect::ExtractFile {
                    id: 7,
                    password: None,
                    ..
                }]
            ),
            "{fx:?}"
        );
        assert_eq!(
            *state.status(10),
            Msg::Extracting {
                name: "a.zip".into()
            }
        );
        assert_eq!(state.unpacking.len(), 1);
        // put.io has taken it: it is polled every two seconds.
        let fx = update(
            &mut state,
            20,
            Event::FileExtractStarted {
                op: op_of(&fx),
                id: 7,
                result: Ok(vec![extraction(1, ExtractStatus::InQueue, &[7])]),
            },
        );
        assert_eq!(polls(&fx), 0);
        assert_eq!(polls(&update(&mut state, 1_000, Event::Tick)), 0);
        assert_eq!(polls(&update(&mut state, 2_020, Event::Tick)), 1);
        update(
            &mut state,
            2_100,
            Event::ExtractionsListed(Ok(vec![extraction(1, ExtractStatus::Extracting, &[7])])),
        );
        assert_eq!(state.unpacking[0].status, ExtractStatus::Extracting);
        // Gone from the list: done. The folder is listed again so the result shows.
        let fx = update(&mut state, 4_200, Event::ExtractionsListed(Ok(vec![])));
        assert!(state.unpacking.is_empty());
        assert_eq!(
            *state.status(4_200),
            Msg::Extracted {
                name: "a.zip".into()
            }
        );
        assert!(fx.contains(&Effect::ListFiles { parent: 0 }), "{fx:?}");
        // Nothing left to follow: no more polling.
        assert_eq!(polls(&update(&mut state, 60_000, Event::Tick)), 0);
    }

    #[test]
    fn an_extraction_that_says_extracted_is_done_at_once() {
        let mut state = signed_in();
        let fx = started_extraction(&mut state);
        update(
            &mut state,
            20,
            Event::FileExtractStarted {
                op: op_of(&fx),
                id: 7,
                result: Ok(vec![extraction(1, ExtractStatus::Extracted, &[7])]),
            },
        );
        assert!(state.unpacking.is_empty());
        assert_eq!(
            *state.status(20),
            Msg::Extracted {
                name: "a.zip".into()
            }
        );
    }

    #[test]
    fn an_empty_answer_to_the_request_does_not_mean_finished() {
        let mut state = signed_in();
        let fx = started_extraction(&mut state);
        // Before put.io has answered, an empty list (an earlier poll) must not finish it.
        update(&mut state, 15, Event::ExtractionsListed(Ok(vec![])));
        assert_eq!(state.unpacking.len(), 1);
        update(
            &mut state,
            20,
            Event::FileExtractStarted {
                op: op_of(&fx),
                id: 7,
                result: Ok(vec![]),
            },
        );
        // put.io said nothing about it and it is not in the list: it is over (and was quick).
        assert!(state.unpacking.is_empty());
    }

    #[test]
    fn a_failed_extraction_says_why_in_the_users_words_and_is_forgotten() {
        let mut state = signed_in();
        let fx = started_extraction(&mut state);
        update(
            &mut state,
            20,
            Event::FileExtractStarted {
                op: op_of(&fx),
                id: 7,
                result: Ok(vec![extraction(1, ExtractStatus::InQueue, &[7])]),
            },
        );
        let mut failing = extraction(1, ExtractStatus::Error, &[7]);
        failing.message = Some("corrupt\u{7} archive".into());
        update(
            &mut state,
            2_100,
            Event::ExtractionsListed(Ok(vec![failing])),
        );
        assert!(state.unpacking.is_empty());
        assert_eq!(
            *state.status(2_100),
            Msg::ExtractFailed {
                name: "a.zip".into(),
                reason: "corrupt archive".into()
            }
        );
    }

    #[test]
    fn a_refused_request_is_reported_and_nothing_is_followed() {
        let mut state = signed_in();
        let fx = started_extraction(&mut state);
        update(
            &mut state,
            20,
            Event::FileExtractStarted {
                op: op_of(&fx),
                id: 7,
                result: Err(ApiError::Api(ApiFailure {
                    status: 400,
                    error_type: Some("X".into()),
                    message: "not an archive".into(),
                })),
            },
        );
        assert!(state.unpacking.is_empty());
        assert_eq!(
            *state.status(20),
            Msg::ExtractFailed {
                name: "a.zip".into(),
                reason: "not an archive".into()
            }
        );
        // And it can be tried again.
        let fx = update(
            &mut state,
            30,
            Event::FileExtractRequested {
                id: 7,
                password: None,
            },
        );
        assert!(!fx.is_empty());
    }

    #[test]
    fn a_password_is_asked_for_once_and_sent_with_the_next_request() {
        let mut state = signed_in();
        let fx = started_extraction(&mut state);
        update(
            &mut state,
            20,
            Event::FileExtractStarted {
                op: op_of(&fx),
                id: 7,
                result: Ok(vec![extraction(1, ExtractStatus::Password, &[7])]),
            },
        );
        assert_eq!(
            state.extract_ask(),
            Some(&PasswordAsk {
                file_id: 7,
                name: "a.zip".into()
            })
        );
        // Waiting for the user is not polled.
        assert_eq!(polls(&update(&mut state, 60_000, Event::Tick)), 0);
        let fx = update(
            &mut state,
            61_000,
            Event::FileExtractRequested {
                id: 7,
                password: Some(Secret::new("hunter2")),
            },
        );
        assert!(state.extract_ask().is_none());
        assert!(
            matches!(&fx[..], [Effect::ExtractFile { id: 7, password: Some(p), .. }] if p.expose() == "hunter2"),
            "{fx:?}"
        );
        // An empty password is no password.
        let mut other = signed_in();
        let _ = started_extraction(&mut other);
        other.unpacking.clear();
        let fx = update(
            &mut other,
            50,
            Event::FileExtractRequested {
                id: 7,
                password: Some(Secret::new("")),
            },
        );
        assert!(
            matches!(&fx[..], [Effect::ExtractFile { password: None, .. }]),
            "{fx:?}"
        );
    }

    #[test]
    fn the_status_does_not_go_on_saying_extracting_when_only_a_password_is_awaited() {
        let mut state = signed_in();
        let fx = started_extraction(&mut state);
        assert_eq!(
            *state.status(11),
            Msg::Extracting {
                name: "a.zip".into()
            }
        );
        update(
            &mut state,
            20,
            Event::FileExtractStarted {
                op: op_of(&fx),
                id: 7,
                result: Ok(vec![extraction(1, ExtractStatus::Password, &[7])]),
            },
        );
        assert_ne!(
            *state.status(30),
            Msg::Extracting {
                name: "a.zip".into()
            }
        );
        // The same when the user gives the password up.
        let mut other = signed_in();
        let fx = started_extraction(&mut other);
        update(
            &mut other,
            20,
            Event::FileExtractStarted {
                op: op_of(&fx),
                id: 7,
                result: Ok(vec![extraction(1, ExtractStatus::Password, &[7])]),
            },
        );
        update(&mut other, 30, Event::FileExtractPasswordDismissed);
        assert_ne!(
            *other.status(31),
            Msg::Extracting {
                name: "a.zip".into()
            }
        );
    }

    #[test]
    fn giving_up_on_a_password_forgets_the_archive() {
        let mut state = signed_in();
        let fx = started_extraction(&mut state);
        update(
            &mut state,
            20,
            Event::FileExtractStarted {
                op: op_of(&fx),
                id: 7,
                result: Ok(vec![extraction(1, ExtractStatus::Password, &[7])]),
            },
        );
        update(&mut state, 30, Event::FileExtractPasswordDismissed);
        assert!(state.extract_ask().is_none() && state.unpacking.is_empty());
    }

    #[test]
    fn an_archive_already_being_extracted_is_not_started_twice() {
        let mut state = signed_in();
        started_extraction(&mut state);
        let again = update(
            &mut state,
            11,
            Event::FileExtractRequested {
                id: 7,
                password: None,
            },
        );
        assert!(again.is_empty(), "{again:?}");
        assert_eq!(state.unpacking.len(), 1);
    }

    #[test]
    fn several_archives_are_summed_up_in_the_status() {
        let mut state = signed_in();
        started_extraction(&mut state);
        update(
            &mut state,
            12,
            Event::FileExtractRequested {
                id: 8,
                password: None,
            },
        );
        assert_eq!(*state.status(12), Msg::ExtractingMany { count: 2 });
    }

    #[test]
    fn an_expired_session_signs_out_and_forgets_the_extractions() {
        let mut state = signed_in();
        started_extraction(&mut state);
        let fx = update(
            &mut state,
            2_100,
            Event::ExtractionsListed(Err(ApiError::Unauthorized)),
        );
        assert!(fx.contains(&Effect::DeleteToken));
        assert!(state.unpacking.is_empty() && state.extract_ask().is_none());
        assert_eq!(polls(&update(&mut state, 60_000, Event::Tick)), 0);
        // Nothing starts without an account.
        assert!(
            update(
                &mut state,
                61_000,
                Event::FileExtractRequested {
                    id: 7,
                    password: None
                }
            )
            .is_empty()
        );
    }

    #[test]
    fn a_failed_poll_is_retried_with_a_wait_and_the_extraction_is_kept() {
        let mut state = signed_in();
        let fx = started_extraction(&mut state);
        update(
            &mut state,
            20,
            Event::FileExtractStarted {
                op: op_of(&fx),
                id: 7,
                result: Ok(vec![extraction(1, ExtractStatus::Extracting, &[7])]),
            },
        );
        update(
            &mut state,
            2_100,
            Event::ExtractionsListed(Err(ApiError::Network("x".into()))),
        );
        assert_eq!(state.unpacking.len(), 1);
        assert_eq!(polls(&update(&mut state, 5_000, Event::Tick)), 0);
        assert_eq!(polls(&update(&mut state, 12_200, Event::Tick)), 1);
    }
}
