//! Uploading files dropped on the window into the folder the Files tab shows.
//!
//! The shell looks at what was dropped (it has the disk) and tells the core what each item is. Files go
//! one at a time to put.io in the order they were dropped (the worker takes them in turn), each with a
//! row of its own while it waits and is sent, and a notice when it is done or did not work. Folders and
//! empty files are not uploaded and the user is told so. A 401 is a refusal, never a sign-out: the polls
//! that run all the time are what notice a session that really ended.

use std::path::PathBuf;

use putio_api::ApiError;

use crate::i18n::Msg;

use super::{ERROR_NOTICE_TIME, Effect, Millis, NOTICE_TIME, Op, OpId, State};

/// How many files one drop may start. More is almost surely a mistake (a whole folder's contents).
pub const MAX_PER_DROP: usize = 100;

/// What a dropped item is, as the shell found out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DroppedKind {
    /// A regular file.
    File,
    /// A folder, or anything else that is not a regular file.
    Other,
}

/// One dropped item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    pub path: PathBuf,
    /// The file's name, as it will be on put.io.
    pub name: String,
    pub size: u64,
    pub kind: DroppedKind,
}

/// A file waiting to be sent or being sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upload {
    pub op: OpId,
    pub name: String,
    pub size: u64,
    /// How many bytes of the file have gone.
    pub sent: u64,
    /// Why it did not finish, in the user's words. The row stays (with this) until it is put away, so that
    /// the reason is not lost with a notice that fades.
    pub failed: Option<String>,
}

impl Upload {
    /// How far along it is, `0..=100`.
    #[must_use]
    pub fn percent(&self) -> u8 {
        if self.size == 0 {
            return 0;
        }
        u8::try_from((u128::from(self.sent) * 100 / u128::from(self.size)).min(100)).unwrap_or(100)
    }
}

/// Why an upload did not finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadFailure {
    /// The user stopped it.
    Cancelled,
    /// put.io (or the network) said no.
    Api(ApiError),
    /// The file could not be read, or changed while it was being read.
    Local(String),
}

/// A name as it will be shown and sent: no control characters, and not empty.
fn clean_name(name: &str) -> String {
    name.chars().filter(|c| !c.is_control()).collect()
}

pub(super) fn dropped(state: &mut State, items: Vec<Dropped>, fx: &mut Vec<Effect>) {
    if !state.is_signed_in() {
        return;
    }
    let parent = state.files.folder_id();
    let mut skipped = 0;
    let mut started = 0;
    for item in items.into_iter().take(MAX_PER_DROP.saturating_mul(4)) {
        let name = clean_name(&item.name);
        if item.kind != DroppedKind::File || item.size == 0 || name.trim().is_empty() {
            skipped += 1;
            continue;
        }
        if started == MAX_PER_DROP {
            skipped += 1;
            continue;
        }
        started += 1;
        let op = state.op(Op::UploadFile);
        state.uploads.push(Upload {
            op,
            name: name.clone(),
            size: item.size,
            sent: 0,
            failed: None,
        });
        fx.push(Effect::UploadFile {
            op,
            path: item.path,
            name,
            size: item.size,
            parent,
        });
    }
    if skipped > 0 {
        state.say_until_replaced(Msg::UploadSkipped { count: skipped });
    }
}

pub(super) fn progress(state: &mut State, op: OpId, sent: u64) {
    if let Some(upload) = state
        .uploads
        .iter_mut()
        .find(|u| u.op == op && u.failed.is_none())
    {
        // Progress only goes forward and never past the end.
        upload.sent = upload.sent.max(sent.min(upload.size));
    }
}

pub(super) fn finished(
    state: &mut State,
    now: Millis,
    op: OpId,
    result: Result<(), UploadFailure>,
) {
    if !state.finish(op, &[Op::UploadFile]) {
        return;
    }
    let Some(at) = state.uploads.iter().position(|u| u.op == op) else {
        return;
    };
    match result {
        Ok(()) => {
            let upload = state.uploads.remove(at);
            state.say(Msg::UploadDone { name: upload.name }, now, NOTICE_TIME);
            // What was sent is in the folder now.
            state.file_refresh.soon(now);
        }
        Err(UploadFailure::Cancelled) => {
            let upload = state.uploads.remove(at);
            state.say(Msg::UploadCancelled { name: upload.name }, now, NOTICE_TIME);
        }
        Err(failure) => {
            let reason = match failure {
                UploadFailure::Api(ApiError::Unauthorized) => {
                    Msg::ShareNotAllowed.text(state.locale)
                }
                UploadFailure::Api(error) => state.reason(&error),
                UploadFailure::Local(why) => why,
                UploadFailure::Cancelled => String::new(),
            };
            // The notice is for the moment; the row keeps the reason until it is put away.
            let name = state
                .uploads
                .get(at)
                .map(|u| u.name.clone())
                .unwrap_or_default();
            if let Some(upload) = state.uploads.get_mut(at) {
                upload.failed = Some(reason.clone());
            }
            state.say(
                Msg::UploadFileFailed { name, reason },
                now,
                ERROR_NOTICE_TIME,
            );
        }
    }
}

/// A failed upload's row was put away.
pub(super) fn dismissed(state: &mut State, op: OpId) {
    state.uploads.retain(|u| u.op != op || u.failed.is_none());
}

pub(super) fn cancel_requested(state: &State, op: OpId, fx: &mut Vec<Effect>) {
    if state
        .uploads
        .iter()
        .any(|u| u.op == op && u.failed.is_none())
    {
        fx.push(Effect::CancelUpload { op });
    }
}

/// Signing out: what is still going is stopped, and the rows go.
pub(super) fn reset(state: &mut State, fx: &mut Vec<Effect>) {
    fx.extend(
        state
            .uploads
            .drain(..)
            .map(|upload| Effect::CancelUpload { op: upload.op }),
    );
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

    fn file(name: &str, size: u64) -> Dropped {
        Dropped {
            path: format!("/tmp/{name}").into(),
            name: name.into(),
            size,
            kind: DroppedKind::File,
        }
    }

    fn uploads(fx: &[Effect]) -> Vec<(OpId, String, i64)> {
        fx.iter()
            .filter_map(|e| match e {
                Effect::UploadFile {
                    op, name, parent, ..
                } => Some((*op, name.clone(), *parent)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn dropped_files_go_into_the_folder_on_show_one_row_each_in_the_order_dropped() {
        let mut state = signed_in();
        update(
            &mut state,
            1,
            Event::FilesOpenFolder {
                id: 42,
                name: "Movies".into(),
            },
        );
        let fx = update(
            &mut state,
            2,
            Event::FilesDropped(vec![file("a.mkv", 100), file("b.zip", 50)]),
        );
        let sent = uploads(&fx);
        assert_eq!(
            sent.iter()
                .map(|(_, n, p)| (n.as_str(), *p))
                .collect::<Vec<_>>(),
            [("a.mkv", 42), ("b.zip", 42)]
        );
        assert_eq!(state.uploads.len(), 2);
        assert_eq!(state.uploads[0].name, "a.mkv");
        assert!(state.is_busy());
    }

    #[test]
    fn folders_empty_files_and_nameless_ones_are_skipped_and_the_user_is_told() {
        let mut state = signed_in();
        let folder = Dropped {
            kind: DroppedKind::Other,
            ..file("dir", 0)
        };
        let fx = update(
            &mut state,
            1,
            Event::FilesDropped(vec![
                folder,
                file("empty", 0),
                file("\u{7}", 5),
                file("ok", 5),
            ]),
        );
        assert_eq!(uploads(&fx).len(), 1);
        assert_eq!(
            state.status_text(1),
            "3 items were not uploaded: folders and empty files cannot be."
        );
    }

    #[test]
    fn a_drop_is_limited_and_the_rest_is_said_to_be_skipped() {
        let mut state = signed_in();
        let many: Vec<Dropped> = (0..MAX_PER_DROP + 5)
            .map(|i| file(&format!("f{i}"), 1))
            .collect();
        let fx = update(&mut state, 1, Event::FilesDropped(many));
        assert_eq!(uploads(&fx).len(), MAX_PER_DROP);
        assert!(
            state
                .status_text(1)
                .starts_with("5 items were not uploaded")
        );
    }

    #[test]
    fn nothing_is_uploaded_when_signed_out() {
        let mut state = State::new(Settings::default(), Locale::En, "0.1.0");
        let fx = update(&mut state, 0, Event::FilesDropped(vec![file("a", 1)]));
        assert!(fx.is_empty() && state.uploads.is_empty());
    }

    #[test]
    fn progress_only_goes_forward_and_never_past_the_end() {
        let mut state = signed_in();
        let fx = update(&mut state, 1, Event::FilesDropped(vec![file("a", 200)]));
        let (op, _, _) = uploads(&fx).remove(0);
        update(&mut state, 2, Event::UploadProgress { op, sent: 100 });
        assert_eq!(state.uploads[0].percent(), 50);
        update(&mut state, 3, Event::UploadProgress { op, sent: 40 });
        assert_eq!(state.uploads[0].sent, 100, "never back");
        update(&mut state, 4, Event::UploadProgress { op, sent: 9_999 });
        assert_eq!(state.uploads[0].percent(), 100);
        // An answer for nothing known changes nothing.
        update(
            &mut state,
            5,
            Event::UploadProgress {
                op: OpId(999),
                sent: 1,
            },
        );
        assert_eq!(state.uploads.len(), 1);
    }

    #[test]
    fn a_finished_upload_says_so_and_asks_for_the_folder_again() {
        let mut state = signed_in();
        state.transfers_expanded = true;
        update(&mut state, 1, Event::SelectTab(crate::settings::Tab::Files));
        // The folder's first listing has come back.
        update(
            &mut state,
            1,
            Event::FilesListed {
                parent: 0,
                result: Ok(putio_api::client::FileListing {
                    parent: None,
                    files: vec![],
                }),
            },
        );
        let fx = update(&mut state, 2, Event::FilesDropped(vec![file("a.mkv", 10)]));
        let (op, _, _) = uploads(&fx).remove(0);
        let fx = update(
            &mut state,
            100_000,
            Event::UploadFinished { op, result: Ok(()) },
        );
        assert!(state.uploads.is_empty() && !state.is_busy());
        assert_eq!(state.status_text(100_000), "Uploaded a.mkv");
        assert!(fx.contains(&Effect::ListFiles { parent: 0 }), "{fx:?}");
        // The same answer twice is ignored.
        update(
            &mut state,
            100_001,
            Event::UploadFinished { op, result: Ok(()) },
        );
    }

    #[test]
    fn a_failed_upload_says_why_and_a_401_is_not_a_sign_out() {
        let mut state = signed_in();
        let fx = update(
            &mut state,
            1,
            Event::FilesDropped(vec![file("a", 10), file("b", 10)]),
        );
        let ops: Vec<OpId> = uploads(&fx).into_iter().map(|(op, _, _)| op).collect();
        let fx = update(
            &mut state,
            2,
            Event::UploadFinished {
                op: ops[0],
                result: Err(UploadFailure::Api(ApiError::Unauthorized)),
            },
        );
        assert!(!fx.contains(&Effect::DeleteToken), "{fx:?}");
        assert_eq!(state.auth, Auth::SignedIn);
        assert_eq!(
            state.status_text(2),
            "Could not upload a: put.io does not let this app do that."
        );
        update(
            &mut state,
            3,
            Event::UploadFinished {
                op: ops[1],
                result: Err(UploadFailure::Api(ApiError::Api(ApiFailure {
                    status: 413,
                    error_type: None,
                    message: "File too large".into(),
                }))),
            },
        );
        assert_eq!(state.status_text(3), "Could not upload b: File too large");
        // The notice fades; the rows keep the reasons until they are put away.
        let reasons: Vec<_> = state.uploads.iter().map(|u| u.failed.clone()).collect();
        assert_eq!(
            reasons,
            [
                Some("put.io does not let this app do that.".into()),
                Some("File too large".into())
            ]
        );
        assert!(!state.is_busy(), "a failed row is not work in flight");
        // A failed row cannot be cancelled, and its progress does not move.
        let fx = update(&mut state, 3, Event::UploadCancelRequested(ops[0]));
        assert!(!fx.iter().any(|e| matches!(e, Effect::CancelUpload { .. })));
        update(
            &mut state,
            3,
            Event::UploadProgress {
                op: ops[0],
                sent: 5,
            },
        );
        assert_eq!(state.uploads[0].sent, 0);
        // Putting a row away takes it out, and only a failed one.
        update(&mut state, 3, Event::UploadDismissed(ops[0]));
        update(&mut state, 3, Event::UploadDismissed(ops[1]));
        assert!(state.uploads.is_empty());
        update(&mut state, 3, Event::UploadDismissed(ops[1]));
        let fx = update(&mut state, 4, Event::FilesDropped(vec![file("c", 10)]));
        let op = uploads(&fx).remove(0).0;
        update(
            &mut state,
            5,
            Event::UploadFinished {
                op,
                result: Err(UploadFailure::Local("the file changed".into())),
            },
        );
        assert_eq!(state.status_text(5), "Could not upload c: the file changed");
    }

    #[test]
    fn a_row_that_is_still_going_cannot_be_put_away() {
        let mut state = signed_in();
        let fx = update(&mut state, 1, Event::FilesDropped(vec![file("a", 10)]));
        let op = uploads(&fx).remove(0).0;
        update(&mut state, 2, Event::UploadDismissed(op));
        assert_eq!(state.uploads.len(), 1);
    }

    #[test]
    fn cancelling_asks_the_worker_and_the_answer_removes_the_row() {
        let mut state = signed_in();
        let fx = update(&mut state, 1, Event::FilesDropped(vec![file("a", 10)]));
        let op = uploads(&fx).remove(0).0;
        let fx = update(&mut state, 2, Event::UploadCancelRequested(op));
        assert!(fx.contains(&Effect::CancelUpload { op }), "{fx:?}");
        assert_eq!(
            state.uploads.len(),
            1,
            "the row stays until the worker answers"
        );
        // Cancelling what is not there does nothing.
        let fx = update(&mut state, 3, Event::UploadCancelRequested(OpId(999)));
        assert!(!fx.iter().any(|e| matches!(e, Effect::CancelUpload { .. })));
        update(
            &mut state,
            4,
            Event::UploadFinished {
                op,
                result: Err(UploadFailure::Cancelled),
            },
        );
        assert!(state.uploads.is_empty());
        assert_eq!(state.status_text(4), "Upload of a cancelled");
    }

    #[test]
    fn signing_out_stops_what_is_going() {
        let mut state = signed_in();
        let fx = update(
            &mut state,
            1,
            Event::FilesDropped(vec![file("a", 10), file("b", 10)]),
        );
        let ops: Vec<OpId> = uploads(&fx).into_iter().map(|(op, _, _)| op).collect();
        let fx = update(&mut state, 2, Event::SignOutRequested);
        for op in ops {
            assert!(fx.contains(&Effect::CancelUpload { op }), "{fx:?}");
        }
        assert!(state.uploads.is_empty());
    }
}
