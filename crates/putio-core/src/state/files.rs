//! The Files part of the state machine: the folder being shown, the search, and the renaming, moving,
//! deleting and folder making that goes through them.
//!
//! The listing is fetched while the Files tab is on screen (and whenever the folder changes), a change
//! made here is followed by a fresh listing, and an answer for a folder the user has already left is
//! thrown away.

use putio_api::client::{FileListing, FileSearch};
use putio_api::{ApiError, File};

use crate::files::{NameProblem, check_name, sort_files};
use crate::i18n::Msg;
use crate::settings::Tab;

use super::refresh::Refresh;
use super::{
    ERROR_NOTICE_TIME, Effect, Millis, NOTICE_TIME, Op, OpId, State, open_file_url,
    session_expired, start_download,
};

/// How old the listing may get before it is fetched again while its tab is open.
const REFRESH_INTERVAL: Millis = 60_000;
/// Opening the tab fetches the listing again when it is older than this.
const STALE_AFTER: Millis = 15_000;

/// One folder on the way from the top level to the folder being shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crumb {
    pub id: i64,
    /// The top level has no name here: the shell calls it what it likes ("Your Files").
    pub name: String,
}

/// The results of a search, shown in place of the folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Search {
    pub query: String,
    pub items: Vec<File>,
    /// How many files match in all (more than `items` when there are many).
    pub total: usize,
    pub loading: bool,
    pub failed: bool,
}

/// What the Files tab shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesView {
    /// From the top level (id 0) down to the folder being shown.
    pub trail: Vec<Crumb>,
    /// What is in that folder: folders first, then by name.
    pub items: Vec<File>,
    /// The folder's listing has arrived.
    pub loaded: bool,
    /// Its listing could not be fetched and there is nothing to show instead.
    pub failed: bool,
    pub search: Option<Search>,
}

impl Default for FilesView {
    fn default() -> Self {
        Self {
            trail: vec![Crumb {
                id: 0,
                name: String::new(),
            }],
            items: Vec::new(),
            loaded: false,
            failed: false,
            search: None,
        }
    }
}

impl FilesView {
    /// The folder being shown (`0` is the top level).
    #[must_use]
    pub fn folder_id(&self) -> i64 {
        self.trail.last().map_or(0, |crumb| crumb.id)
    }
}

/// Where a rename, a new folder or a move stands. The dialog that asked reads this to know whether to
/// show a spinner, an error, or to close (and then acknowledges with `Event::FileActionDismissed`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum FileAction {
    #[default]
    Idle,
    Working,
    /// Refused (by the check here or by put.io); the text is for the user.
    Failed(String),
    Done,
}

/// Whether the listing is on screen: signed in, list open, Files tab chosen.
fn wanted(state: &State) -> bool {
    state.is_signed_in() && state.transfers_expanded && state.tab == Tab::Files
}

/// The Files tab was just opened: look again if what is shown is old.
pub(super) fn tab_opened(state: &mut State, now: Millis) {
    if state.file_refresh.is_older_than(now, STALE_AFTER) {
        state.file_refresh.soon(now);
    }
}

/// Ask for the folder when it is on screen and due. Runs after every event.
pub(super) fn maybe_load(state: &mut State, now: Millis, fx: &mut Vec<Effect>) {
    if wanted(state) && state.file_refresh.due(now) {
        state.file_refresh.started();
        let parent = state.files.folder_id();
        state.files_requested = Some(parent);
        fx.push(Effect::ListFiles { parent });
    }
}

/// Show another folder: what was in the old one goes at once (it would be clickable until the answer came,
/// and a click would act on the wrong list), and the new one is asked for.
fn enter(state: &mut State, now: Millis, trail: Vec<Crumb>) {
    state.files.trail = trail;
    state.files.items.clear();
    state.files.loaded = false;
    state.files.failed = false;
    state.files.search = None;
    state.files_requested = None;
    state.file_refresh.restart(now);
}

fn clean(name: &str) -> String {
    name.chars().filter(|c| !c.is_control()).take(255).collect()
}

pub(super) fn open_folder(state: &mut State, now: Millis, id: i64, name: &str) {
    if id < 0 {
        return;
    }
    if state.files.search.is_some() {
        // A search result is somewhere else: its way from the top level is not known, so the trail starts
        // over at the folder itself.
        let mut trail = vec![Crumb {
            id: 0,
            name: String::new(),
        }];
        if id != 0 {
            trail.push(Crumb {
                id,
                name: clean(name),
            });
        }
        enter(state, now, trail);
        return;
    }
    if state.files.folder_id() == id {
        return;
    }
    let mut trail = state.files.trail.clone();
    trail.push(Crumb {
        id,
        name: clean(name),
    });
    enter(state, now, trail);
}

/// Go back up to the `index`-th folder of the trail (the breadcrumb).
pub(super) fn go_to(state: &mut State, now: Millis, index: usize) {
    let last = state.files.trail.len().saturating_sub(1);
    let at = index.min(last);
    if at == last {
        // The folder that is open: only a search is left.
        state.files.search = None;
        return;
    }
    let mut trail = state.files.trail.clone();
    trail.truncate(at + 1);
    enter(state, now, trail);
}

/// The "try again" button after the listing could not be fetched.
pub(super) fn retry(state: &mut State, now: Millis) {
    state.files.failed = false;
    state.file_refresh.retry(now);
}

pub(super) fn listed(
    state: &mut State,
    now: Millis,
    parent: i64,
    result: Result<FileListing, ApiError>,
    fx: &mut Vec<Effect>,
) {
    if !state.is_signed_in() {
        return;
    }
    // An answer for a folder that has been left (or asked for before the folder changed) is of no use.
    if state.files_requested != Some(parent) || state.files.folder_id() != parent {
        return;
    }
    state.files_requested = None;
    match result {
        Ok(listing) => {
            let mut items = listing.files;
            sort_files(&mut items);
            state.files.items = items;
            state.files.loaded = true;
            state.files.failed = false;
            state.file_refresh.succeeded(now, REFRESH_INTERVAL);
        }
        Err(ApiError::Unauthorized) => session_expired(state, fx),
        Err(error) => {
            // A listing that was fetched before stays on screen.
            state.files.failed = !state.files.loaded;
            state.file_refresh.failed(now, &error);
        }
    }
}

// ---- search ---------------------------------------------------------------------------------

pub(super) fn search_requested(state: &mut State, query: &str, fx: &mut Vec<Effect>) {
    if !state.is_signed_in() {
        return;
    }
    let query = query.trim();
    if query.is_empty() {
        state.files.search = None;
        return;
    }
    let query = clean(query);
    state.files.search = Some(Search {
        query: query.clone(),
        items: Vec::new(),
        total: 0,
        loading: true,
        failed: false,
    });
    fx.push(Effect::SearchFiles { query });
}

pub(super) fn searched(
    state: &mut State,
    query: &str,
    result: Result<FileSearch, ApiError>,
    fx: &mut Vec<Effect>,
) {
    // The answer to a question that is not the current one is dropped.
    if state
        .files
        .search
        .as_ref()
        .is_none_or(|search| search.query != query)
    {
        return;
    }
    match result {
        Ok(found) => {
            let mut items = found.files;
            sort_files(&mut items);
            if let Some(search) = state.files.search.as_mut() {
                search.total = found.total;
                search.items = items;
                search.loading = false;
                search.failed = false;
            }
        }
        Err(ApiError::Unauthorized) => session_expired(state, fx),
        Err(_) => {
            if let Some(search) = state.files.search.as_mut() {
                search.items.clear();
                search.loading = false;
                search.failed = true;
            }
        }
    }
}

// ---- changing files -------------------------------------------------------------------------

fn problem_text(state: &State, problem: NameProblem) -> String {
    let msg = match problem {
        NameProblem::Empty => Msg::FileErrNameEmpty,
        NameProblem::HasSlash => Msg::FileErrNameSlash,
        NameProblem::TooLong => Msg::FileErrNameTooLong,
    };
    state.text(&msg)
}

/// A change that goes through a dialog can be asked for once at a time.
fn can_start(state: &State) -> bool {
    state.is_signed_in() && state.file_action != FileAction::Working
}

pub(super) fn rename_requested(state: &mut State, id: i64, name: &str, fx: &mut Vec<Effect>) {
    if !can_start(state) {
        return;
    }
    match check_name(name) {
        Err(problem) => state.file_action = FileAction::Failed(problem_text(state, problem)),
        Ok(name) => {
            let op = state.op(Op::FileRename);
            state.file_action = FileAction::Working;
            fx.push(Effect::RenameFile { op, id, name });
        }
    }
}

pub(super) fn create_requested(state: &mut State, name: &str, fx: &mut Vec<Effect>) {
    if !can_start(state) {
        return;
    }
    match check_name(name) {
        Err(problem) => state.file_action = FileAction::Failed(problem_text(state, problem)),
        Ok(name) => {
            let op = state.op(Op::FileCreate);
            let parent = state.files.folder_id();
            state.file_action = FileAction::Working;
            fx.push(Effect::CreateFolder { op, parent, name });
        }
    }
}

pub(super) fn move_requested(state: &mut State, id: i64, to: i64, fx: &mut Vec<Effect>) {
    if !can_start(state) || to < 0 || id < 0 {
        return;
    }
    let op = state.op(Op::FileMove);
    state.file_action = FileAction::Working;
    fx.push(Effect::MoveFile { op, id, to });
}

/// The answer to a rename, a new folder or a move.
pub(super) fn changed(
    state: &mut State,
    now: Millis,
    op: OpId,
    result: Result<(), ApiError>,
    fx: &mut Vec<Effect>,
) {
    let said = match state.ops.get(&op) {
        Some(Op::FileRename) => Msg::FileRenamedNotice,
        Some(Op::FileCreate) => Msg::FolderCreatedNotice,
        Some(Op::FileMove) => Msg::FileMovedNotice,
        _ => return,
    };
    if !state.finish(op, &[Op::FileRename, Op::FileCreate, Op::FileMove]) {
        return;
    }
    match result {
        Ok(()) => {
            state.file_action = FileAction::Done;
            state.say(said, now, NOTICE_TIME);
            refresh_everything(state, now, fx);
        }
        Err(ApiError::Unauthorized) => session_expired(state, fx),
        Err(error) => state.file_action = FileAction::Failed(state.reason(&error)),
    }
}

pub(super) fn action_dismissed(state: &mut State) {
    if matches!(state.file_action, FileAction::Done | FileAction::Failed(_)) {
        state.file_action = FileAction::Idle;
    }
}

pub(super) fn delete_requested(state: &mut State, id: i64, fx: &mut Vec<Effect>) {
    if state.is_signed_in() && id >= 0 {
        let op = state.op(Op::FileDelete);
        fx.push(Effect::DeleteFile { op, id });
    }
}

pub(super) fn delete_finished(
    state: &mut State,
    now: Millis,
    (op, id): (OpId, i64),
    result: Result<(), ApiError>,
    fx: &mut Vec<Effect>,
) {
    if !state.finish(op, &[Op::FileDelete]) {
        return;
    }
    match result {
        Ok(()) => {
            state.files.items.retain(|file| file.id != id);
            if let Some(search) = state.files.search.as_mut() {
                search.items.retain(|file| file.id != id);
            }
            state.say(Msg::FileDeletedNotice, now, NOTICE_TIME);
            refresh_everything(state, now, fx);
        }
        Err(ApiError::Unauthorized) => session_expired(state, fx),
        Err(error) => {
            let reason = state.reason(&error);
            state.say(Msg::FileActionFailed { reason }, now, ERROR_NOTICE_TIME);
        }
    }
}

/// After a change: the listing is fetched again, and so is a search that is showing.
fn refresh_everything(state: &mut State, now: Millis, fx: &mut Vec<Effect>) {
    state.file_refresh.soon(now);
    if let Some(search) = state.files.search.as_mut() {
        search.loading = true;
        fx.push(Effect::SearchFiles {
            query: search.query.clone(),
        });
    }
}

// ---- what a row does besides changing ---------------------------------------------------------

pub(super) fn open_on_putio(state: &State, id: i64, fx: &mut Vec<Effect>) {
    if state.is_signed_in() && id >= 0 {
        fx.push(Effect::OpenUrl(open_file_url(Some(id))));
    }
}

pub(super) fn copy_link(state: &mut State, now: Millis, id: i64, fx: &mut Vec<Effect>) {
    if state.is_signed_in() && id >= 0 {
        fx.push(Effect::CopyText(open_file_url(Some(id))));
        state.say(Msg::Copied, now, NOTICE_TIME);
    }
}

pub(super) fn download(state: &mut State, now: Millis, id: i64, fx: &mut Vec<Effect>) {
    if id >= 0 {
        start_download(state, now, Some(id), fx);
    }
}

pub(super) fn stream(state: &mut State, id: i64, fx: &mut Vec<Effect>) {
    if state.is_signed_in() && id >= 0 {
        state.close_at = None;
        let op = state.op(Op::Stream);
        state.say_until_replaced(Msg::Streaming);
        fx.push(Effect::Stream { op, file_id: id });
    }
}

/// Forget everything about the account's files (signing out).
pub(super) fn reset(state: &mut State) {
    state.files = FilesView::default();
    state.file_action = FileAction::Idle;
    state.file_refresh = Refresh::default();
    state.files_requested = None;
}

#[cfg(test)]
mod tests {
    use putio_api::{ApiFailure, FileType};

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

    fn file(id: i64, name: &str, kind: FileType) -> File {
        File {
            id,
            name: name.into(),
            parent_id: Some(0),
            file_type: kind,
            content_type: String::new(),
            size: 10,
            screenshot: None,
            created_at: None,
        }
    }

    #[allow(clippy::unnecessary_wraps)] // it is what the events carry
    fn listing(files: Vec<File>) -> Result<FileListing, ApiError> {
        Ok(FileListing {
            parent: None,
            files,
        })
    }

    fn api_error(status: u16, kind: &str, message: &str) -> ApiError {
        ApiError::Api(ApiFailure {
            status,
            error_type: Some(kind.into()),
            message: message.into(),
        })
    }

    fn op_of(fx: &[Effect]) -> OpId {
        fx.iter()
            .find_map(|effect| match effect {
                Effect::RenameFile { op, .. }
                | Effect::CreateFolder { op, .. }
                | Effect::MoveFile { op, .. }
                | Effect::DeleteFile { op, .. }
                | Effect::Stream { op, .. }
                | Effect::PrepareDownload { op, .. } => Some(*op),
                _ => None,
            })
            .expect("a file effect with an op id")
    }

    fn lists(fx: &[Effect]) -> usize {
        fx.iter()
            .filter(|e| matches!(e, Effect::ListFiles { .. }))
            .count()
    }

    /// Open the Files tab and answer the first listing.
    fn with_files(state: &mut State, now: Millis, files: Vec<File>) {
        let fx = update(state, now, Event::SelectTab(Tab::Files));
        assert!(fx.contains(&Effect::ListFiles { parent: 0 }), "{fx:?}");
        update(
            state,
            now,
            Event::FilesListed {
                parent: 0,
                result: listing(files),
            },
        );
    }

    fn names(files: &[File]) -> Vec<&str> {
        files.iter().map(|f| f.name.as_str()).collect()
    }

    fn open(id: i64, name: &str) -> Event {
        Event::FilesOpenFolder {
            id,
            name: name.into(),
        }
    }

    #[test]
    fn nothing_is_listed_until_the_files_tab_is_open_and_then_once() {
        let mut state = signed_in();
        for now in [1_000, 100_000] {
            assert_eq!(lists(&update(&mut state, now, Event::Tick)), 0);
        }
        let fx = update(&mut state, 200_000, Event::SelectTab(Tab::Files));
        assert_eq!(lists(&fx), 1, "{fx:?}");
        assert_eq!(lists(&update(&mut state, 200_100, Event::Tick)), 0);
    }

    #[test]
    fn a_listing_is_shown_with_folders_first_and_kept_fresh_while_it_is_open() {
        let mut state = signed_in();
        with_files(
            &mut state,
            0,
            vec![
                file(1, "b.mkv", FileType::Video),
                file(2, "Zed", FileType::Folder),
                file(3, "a.mkv", FileType::Video),
            ],
        );
        assert_eq!(names(&state.files.items), ["Zed", "a.mkv", "b.mkv"]);
        assert!(state.files.loaded && !state.files.failed);
        assert_eq!(
            lists(&update(&mut state, REFRESH_INTERVAL - 1, Event::Tick)),
            0
        );
        assert_eq!(lists(&update(&mut state, REFRESH_INTERVAL, Event::Tick)), 1);
    }

    #[test]
    fn going_into_a_folder_empties_the_list_and_asks_for_that_folder() {
        let mut state = signed_in();
        with_files(&mut state, 0, vec![file(9, "Movies", FileType::Folder)]);
        let fx = update(&mut state, 10, open(9, "Movies"));
        assert!(
            state.files.items.is_empty(),
            "nothing to click on a wrong page"
        );
        assert!(!state.files.loaded);
        assert_eq!(state.files.folder_id(), 9);
        assert!(fx.contains(&Effect::ListFiles { parent: 9 }), "{fx:?}");
        // The answer for the folder that was left, arriving late, changes nothing.
        update(
            &mut state,
            11,
            Event::FilesListed {
                parent: 0,
                result: listing(vec![file(1, "late", FileType::File)]),
            },
        );
        assert!(state.files.items.is_empty());
        update(
            &mut state,
            12,
            Event::FilesListed {
                parent: 9,
                result: listing(vec![file(2, "film.mkv", FileType::Video)]),
            },
        );
        assert_eq!(names(&state.files.items), ["film.mkv"]);
        // Opening the folder that is already open does nothing.
        assert!(update(&mut state, 13, open(9, "Movies")).is_empty());
    }

    #[test]
    fn the_breadcrumb_goes_back_up() {
        let mut state = signed_in();
        with_files(&mut state, 0, vec![]);
        for (id, name) in [(1, "A"), (2, "B"), (3, "C")] {
            update(&mut state, 1, open(id, name));
        }
        assert_eq!(state.files.trail.len(), 4);
        let fx = update(&mut state, 2, Event::FilesGoTo(1));
        assert_eq!(state.files.folder_id(), 1);
        assert_eq!(state.files.trail.len(), 2);
        assert!(fx.contains(&Effect::ListFiles { parent: 1 }), "{fx:?}");
        // The top level, and an index past the end.
        update(&mut state, 3, Event::FilesGoTo(0));
        assert_eq!(state.files.folder_id(), 0);
        update(&mut state, 4, Event::FilesGoTo(99));
        assert_eq!(state.files.folder_id(), 0);
    }

    #[test]
    fn a_failed_first_listing_says_so_and_a_failed_later_one_keeps_the_files() {
        let mut state = signed_in();
        update(&mut state, 0, Event::SelectTab(Tab::Files));
        update(
            &mut state,
            10,
            Event::FilesListed {
                parent: 0,
                result: Err(api_error(503, "x", "down")),
            },
        );
        assert!(state.files.failed && !state.files.loaded);
        let fx = update(&mut state, 20, Event::FilesRefreshRequested);
        assert!(!state.files.failed);
        assert!(fx.contains(&Effect::ListFiles { parent: 0 }), "{fx:?}");
        update(
            &mut state,
            30,
            Event::FilesListed {
                parent: 0,
                result: listing(vec![file(1, "a", FileType::File)]),
            },
        );
        update(
            &mut state,
            90_000,
            Event::FilesListed {
                parent: 0,
                result: Err(ApiError::Network("x".into())),
            },
        );
        assert!(!state.files.failed);
        assert_eq!(state.files.items.len(), 1);
    }

    #[allow(clippy::unnecessary_wraps)] // it is what the events carry
    fn found(files: Vec<File>, total: usize) -> Result<FileSearch, ApiError> {
        Ok(FileSearch { files, total })
    }

    #[test]
    fn a_search_replaces_the_folder_until_it_is_cleared_and_late_answers_are_dropped() {
        let mut state = signed_in();
        with_files(&mut state, 0, vec![file(1, "a", FileType::File)]);
        let fx = update(
            &mut state,
            1,
            Event::FilesSearchRequested("  spring ".into()),
        );
        assert!(
            fx.contains(&Effect::SearchFiles {
                query: "spring".into()
            }),
            "{fx:?}"
        );
        assert!(state.files.search.as_ref().unwrap().loading);
        // An answer to an earlier question does not show.
        update(
            &mut state,
            2,
            Event::FilesSearched {
                query: "old".into(),
                result: found(vec![file(7, "x", FileType::File)], 1),
            },
        );
        assert!(state.files.search.as_ref().unwrap().items.is_empty());
        update(
            &mut state,
            3,
            Event::FilesSearched {
                query: "spring".into(),
                result: found(
                    vec![
                        file(5, "s.mkv", FileType::Video),
                        file(6, "Spring", FileType::Folder),
                    ],
                    250,
                ),
            },
        );
        let search = state.files.search.as_ref().unwrap();
        assert_eq!(names(&search.items), ["Spring", "s.mkv"]);
        assert_eq!(
            (search.total, search.loading, search.failed),
            (250, false, false)
        );
        // The folder underneath is untouched.
        assert_eq!(state.files.items.len(), 1);
        update(&mut state, 4, Event::FilesSearchCleared);
        assert!(state.files.search.is_none());
        // An empty question is no question.
        assert!(update(&mut state, 5, Event::FilesSearchRequested("   ".into())).is_empty());
        assert!(state.files.search.is_none());
    }

    #[test]
    fn a_failed_search_says_so_and_opening_a_result_folder_starts_a_new_trail() {
        let mut state = signed_in();
        with_files(&mut state, 0, vec![]);
        update(&mut state, 1, open(4, "Deep"));
        update(&mut state, 2, Event::FilesSearchRequested("x".into()));
        update(
            &mut state,
            3,
            Event::FilesSearched {
                query: "x".into(),
                result: Err(ApiError::Network("x".into())),
            },
        );
        let search = state.files.search.as_ref().unwrap();
        assert!(search.failed && !search.loading);
        update(&mut state, 4, Event::FilesSearchRequested("y".into()));
        let fx = update(&mut state, 5, open(77, "Found"));
        assert!(state.files.search.is_none());
        assert_eq!(
            state.files.trail.iter().map(|c| c.id).collect::<Vec<_>>(),
            [0, 77],
            "the way to a search result is not known"
        );
        assert!(fx.contains(&Effect::ListFiles { parent: 77 }), "{fx:?}");
    }

    #[test]
    fn a_name_is_checked_before_anything_is_sent() {
        let mut state = signed_in();
        let long = "x".repeat(300);
        for (name, expected) in [
            ("", Msg::FileErrNameEmpty),
            ("a/b", Msg::FileErrNameSlash),
            (long.as_str(), Msg::FileErrNameTooLong),
        ] {
            let fx = update(
                &mut state,
                1,
                Event::FileRenameRequested {
                    id: 3,
                    name: name.into(),
                },
            );
            assert!(fx.is_empty(), "{name:?}: {fx:?}");
            assert_eq!(
                *state.file_action(),
                FileAction::Failed(expected.text(Locale::En))
            );
            update(&mut state, 2, Event::FileActionDismissed);
            assert_eq!(*state.file_action(), FileAction::Idle);
            let fx = update(&mut state, 3, Event::FolderCreateRequested(name.into()));
            assert!(fx.is_empty());
            update(&mut state, 4, Event::FileActionDismissed);
        }
    }

    #[test]
    fn renaming_goes_through_working_to_done_and_the_listing_is_fetched_again() {
        let mut state = signed_in();
        with_files(&mut state, 0, vec![file(3, "old", FileType::File)]);
        let rename = |name: &str| Event::FileRenameRequested {
            id: 3,
            name: name.into(),
        };
        let fx = update(&mut state, 100, rename("  new  "));
        assert!(
            matches!(&fx[..], [Effect::RenameFile { id: 3, name, .. }] if name == "new"),
            "{fx:?}"
        );
        assert_eq!(*state.file_action(), FileAction::Working);
        // A second request while the first is on its way is ignored.
        assert!(update(&mut state, 101, rename("again")).is_empty());
        let fx = update(
            &mut state,
            200,
            Event::FileChangeFinished {
                op: op_of(&fx),
                result: Ok(()),
            },
        );
        assert_eq!(*state.file_action(), FileAction::Done);
        assert_eq!(*state.status(200), Msg::FileRenamedNotice);
        assert!(fx.contains(&Effect::ListFiles { parent: 0 }), "{fx:?}");
        update(&mut state, 300, Event::FileActionDismissed);
        assert_eq!(*state.file_action(), FileAction::Idle);
    }

    #[test]
    fn put_io_refusing_a_change_is_explained_and_the_dialog_stays() {
        let mut state = signed_in();
        let fx = update(&mut state, 1, Event::FileMoveRequested { id: 3, to: 9 });
        assert!(matches!(&fx[..], [Effect::MoveFile { id: 3, to: 9, .. }]));
        update(
            &mut state,
            2,
            Event::FileChangeFinished {
                op: op_of(&fx),
                result: Err(api_error(400, "X", "cannot move\u{7} there")),
            },
        );
        assert_eq!(
            *state.file_action(),
            FileAction::Failed("cannot move there".into())
        );
        // After a refusal the user can try again.
        assert!(!update(&mut state, 3, Event::FileMoveRequested { id: 3, to: 5 }).is_empty());
    }

    #[test]
    fn a_new_folder_is_made_in_the_folder_being_shown() {
        let mut state = signed_in();
        with_files(&mut state, 0, vec![]);
        update(&mut state, 1, open(12, "TV"));
        let fx = update(
            &mut state,
            2,
            Event::FolderCreateRequested(" Season 2 ".into()),
        );
        assert!(
            matches!(&fx[..], [Effect::CreateFolder { parent: 12, name, .. }] if name == "Season 2"),
            "{fx:?}"
        );
        update(
            &mut state,
            3,
            Event::FileChangeFinished {
                op: op_of(&fx),
                result: Ok(()),
            },
        );
        assert_eq!(*state.status(3), Msg::FolderCreatedNotice);
    }

    #[test]
    fn a_change_while_a_search_shows_runs_the_search_again() {
        let mut state = signed_in();
        with_files(&mut state, 0, vec![]);
        update(&mut state, 1, Event::FilesSearchRequested("x".into()));
        update(
            &mut state,
            2,
            Event::FilesSearched {
                query: "x".into(),
                result: found(vec![file(5, "x.mkv", FileType::Video)], 1),
            },
        );
        let fx = update(&mut state, 3, Event::FileMoveRequested { id: 5, to: 1 });
        let fx = update(
            &mut state,
            4,
            Event::FileChangeFinished {
                op: op_of(&fx),
                result: Ok(()),
            },
        );
        assert!(
            fx.contains(&Effect::SearchFiles { query: "x".into() }),
            "{fx:?}"
        );
        assert!(state.files.search.as_ref().unwrap().loading);
    }

    #[test]
    fn deleting_removes_the_row_only_when_put_io_agrees() {
        let mut state = signed_in();
        with_files(
            &mut state,
            0,
            vec![file(5, "a", FileType::File), file(6, "b", FileType::File)],
        );
        let fx = update(&mut state, 1, Event::FileDeleteRequested(5));
        assert!(matches!(&fx[..], [Effect::DeleteFile { id: 5, .. }]));
        update(
            &mut state,
            2,
            Event::FileDeleteFinished {
                op: op_of(&fx),
                id: 5,
                result: Err(api_error(403, "X", "not yours")),
            },
        );
        assert_eq!(state.files.items.len(), 2);
        assert_eq!(
            *state.status(2),
            Msg::FileActionFailed {
                reason: "not yours".into()
            }
        );
        let fx = update(&mut state, 3, Event::FileDeleteRequested(5));
        let after = update(
            &mut state,
            4,
            Event::FileDeleteFinished {
                op: op_of(&fx),
                id: 5,
                result: Ok(()),
            },
        );
        assert_eq!(names(&state.files.items), ["b"]);
        assert_eq!(*state.status(4), Msg::FileDeletedNotice);
        assert!(
            after.contains(&Effect::ListFiles { parent: 0 }),
            "{after:?}"
        );
    }

    #[test]
    fn opening_copying_downloading_and_streaming_a_file() {
        let mut state = signed_in();
        assert_eq!(
            update(&mut state, 1, Event::FileOpenRequested(55)),
            [Effect::OpenUrl("https://put.io/files/55".into())]
        );
        assert_eq!(
            update(&mut state, 2, Event::FileCopyLinkRequested(55)),
            [Effect::CopyText("https://put.io/files/55".into())]
        );
        assert_eq!(*state.status(2), Msg::Copied);
        let fx = update(&mut state, 3, Event::FileDownloadRequested(55));
        assert!(
            matches!(&fx[..], [Effect::PrepareDownload { file_id: 55, .. }]),
            "{fx:?}"
        );
        assert_eq!(*state.status(3), Msg::DownloadPreparing);
        let fx = update(&mut state, 4, Event::FileStreamRequested(56));
        assert!(
            matches!(&fx[..], [Effect::Stream { file_id: 56, .. }]),
            "{fx:?}"
        );
        // Nonsense ids do nothing.
        for event in [
            Event::FileOpenRequested(-1),
            Event::FileCopyLinkRequested(-1),
            Event::FileDownloadRequested(-1),
            Event::FileStreamRequested(-1),
            Event::FileDeleteRequested(-1),
        ] {
            assert!(update(&mut state, 5, event).is_empty());
        }
        // Neither does a missing account.
        let mut out = State::new(Settings::default(), Locale::En, "0.1.0");
        for event in [
            Event::FileOpenRequested(1),
            Event::FileCopyLinkRequested(1),
            Event::FileDownloadRequested(1),
            Event::FileStreamRequested(1),
            Event::FileDeleteRequested(1),
            Event::FileRenameRequested {
                id: 1,
                name: "a".into(),
            },
            Event::FolderCreateRequested("a".into()),
            Event::FileMoveRequested { id: 1, to: 0 },
        ] {
            assert!(update(&mut out, 1, event).is_empty());
        }
    }

    #[test]
    fn unknown_and_repeated_answers_are_ignored() {
        let mut state = signed_in();
        with_files(&mut state, 0, vec![file(5, "a", FileType::File)]);
        let fx = update(&mut state, 1, Event::FileDeleteRequested(5));
        let op = op_of(&fx);
        let done = || Event::FileDeleteFinished {
            op,
            id: 5,
            result: Ok(()),
        };
        update(&mut state, 2, done());
        update(
            &mut state,
            3,
            Event::FilesListed {
                parent: 0,
                result: listing(vec![file(5, "a", FileType::File)]),
            },
        );
        update(&mut state, 4, done());
        assert_eq!(
            state.files.items.len(),
            1,
            "a repeated answer must not remove it again"
        );
        update(
            &mut state,
            5,
            Event::FileChangeFinished {
                op: OpId(9_999),
                result: Ok(()),
            },
        );
        assert_eq!(*state.file_action(), FileAction::Idle);
    }

    #[test]
    fn an_expired_session_signs_out_and_forgets_the_files() {
        let mut state = signed_in();
        with_files(&mut state, 0, vec![file(5, "a", FileType::File)]);
        update(&mut state, 1, open(9, "x"));
        let fx = update(
            &mut state,
            2,
            Event::FilesListed {
                parent: 9,
                result: Err(ApiError::Unauthorized),
            },
        );
        assert!(fx.contains(&Effect::DeleteToken));
        assert_eq!(state.files, FilesView::default());
        assert_eq!(*state.file_action(), FileAction::Idle);
        assert_eq!(lists(&update(&mut state, 10_000_000, Event::Tick)), 0);
        // A late answer for the account that is gone brings nothing back.
        update(
            &mut state,
            10_000_001,
            Event::FilesListed {
                parent: 0,
                result: listing(vec![file(1, "x", FileType::File)]),
            },
        );
        assert!(state.files.items.is_empty());
    }
}
