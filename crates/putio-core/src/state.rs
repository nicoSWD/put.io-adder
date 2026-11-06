//! The application as a pure state machine.
//!
//! `update(state, now, event)` mutates the [`State`] and returns the [`Effect`]s the shell must
//! carry out (network calls, notifications, opening URLs, …). The shell reports results back as
//! further [`Event`]s. There is no I/O, no threads and no clock in here: `now` is passed in, so
//! every behaviour (polling cadence, back-off, auto-close, status-line expiry) is testable by
//! feeding events and advancing a number.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use putio_api::client::{FileListing, FileSearch};
use putio_api::{
    Account, ApiError, DeviceCode, Extraction, Friend, FriendChange, Release, RssFeed,
    RssFeedSettings, Share, ShareWith, SharedFile, Token, Transfer, ZipState, ZipStatus,
};

use crate::update::UpdateAsset;

use crate::download;
use crate::handler;
use crate::i18n::{Locale, Msg};
use crate::intake::{Intake, IntakeError};
use crate::secret::Secret;
use crate::settings::{Folder, Settings, Tab, ThemeChoice, WindowSize};
use crate::transfers::{self, Summary, Tracker};
use crate::update::{self, UpdateStatus};

mod detail;
mod extract;
mod feeds;
mod files;
mod friends;
mod links;
mod refresh;
mod shares;
mod uploads;

pub use detail::TransferDetail;
pub use extract::{PasswordAsk, Unpacking};
pub use feeds::{FeedList, FeedSave};
pub use files::{Crumb, FileAction, FilesView, Search};
pub use friends::{FriendAction, FriendsView};
pub use links::LinkAdd;
pub use shares::{Fetch, Problem, ShareAction, SharedFiles, SharingView};
pub use uploads::{Dropped, DroppedKind, MAX_PER_DROP, Upload, UploadFailure};

/// Monotonic milliseconds supplied by the shell.
pub type Millis = u64;

pub const POLL_INTERVAL: Millis = 20_000;
pub const MAX_BACKOFF: Millis = 300_000;
const LINK_POLL_INTERVAL: Millis = 3_000;
const LINK_TTL: Millis = 10 * 60_000;
/// Between update checks while the app stays open, and after one that could not reach GitHub.
const UPDATE_CHECK_INTERVAL: Millis = 6 * 60 * 60_000;
const UPDATE_RETRY_INTERVAL: Millis = 30 * 60_000;
const ZIP_POLL_INTERVAL: Millis = 2_000;
const ZIP_TTL: Millis = 10 * 60_000;
const CLOSE_DELAY: Millis = 5_000;
const NOTICE_TIME: Millis = 5_000;
const ERROR_NOTICE_TIME: Millis = 9_000;
const FOREVER: Millis = Millis::MAX;
/// Items received before sign-in are kept (and added after) up to this many.
const MAX_QUEUED_INTAKE: usize = 20;
const MAX_REASON_CHARS: usize = 200;

pub const LINK_URL: &str = "https://put.io/link";
pub const PROJECT_URL: &str = "https://github.com/nicoSWD/put.io-adder";
/// Where to say thanks for the app.
pub const SUPPORT_URL: &str = "https://buymeacoffee.com/nicoswd";

/// What went wrong with the place the sign-in is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretProblem {
    /// There is none: the sign-in lives only until the app quits.
    Unavailable,
    Load,
    Save,
    Delete,
}

/// What went wrong with the preferences file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsProblem {
    /// There is a file, but it could not be read or is not valid: the defaults are in use.
    Unreadable,
    /// Changes cannot be written, so they are lost when the app quits.
    NotSaved,
}

/// Identifies one outstanding operation, so a late result can never be attributed to the wrong request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OpId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignedOutReason {
    /// The server rejected the stored token.
    SessionExpired,
    /// The device code was not linked in time (or the server no longer knows it).
    CodeExpired,
    /// Could not even get a code.
    CodeRequestFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Linking {
    pub code: String,
    next_poll_at: Millis,
    deadline: Millis,
    in_flight: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Auth {
    /// Before [`Event::Started`].
    Starting,
    SignedOut(Option<SignedOutReason>),
    RequestingCode,
    Linking(Linking),
    SignedIn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    AddMagnet,
    AddLink,
    UploadTorrent,
    Cancel,
    Retry,
    Clear,
    Download,
    Update,
    Stream,
    FeedCreate,
    FeedUpdate,
    FeedPause,
    FeedDelete,
    FileRename,
    FileCreate,
    FileMove,
    FileDelete,
    Share,
    Unshare,
    Extract,
    Friend,
    UploadFile,
}

/// Where an in-place update has got to.
#[derive(Debug, Clone, PartialEq)]
pub enum Install {
    Idle,
    Working {
        stage: UpdateStage,
        /// How far through the whole job (0 to 1).
        fraction: f32,
    },
    /// Downloaded and unpacked: waiting for the user to restart.
    Ready {
        version: String,
    },
    Failed {
        version: String,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateStage {
    Downloading,
    Unpacking,
}

/// Whether this copy of the app can replace itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallTarget {
    /// Not looked at yet.
    Unknown,
    Ready,
    /// Why not (a development build, a folder the user cannot write to, …).
    Unavailable(String),
}

/// What put.io gave us for a download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prepared {
    /// A file: its signed download address.
    Url(String),
    /// A folder: the id of the ZIP put.io is building from it.
    Zip(i64),
}

/// A ZIP that put.io is building, polled until it is done.
#[derive(Debug)]
struct ZipJob {
    op: OpId,
    zip_id: i64,
    deadline: Millis,
    next_poll_at: Millis,
    in_flight: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamFailure {
    NoPlayer,
    NothingToPlay,
    LaunchFailed,
    Api(ApiError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsChange {
    CloseAfterMagnet(bool),
    CloseAfterTorrent(bool),
    TrashTorrentAfterUpload(bool),
    CheckForUpdates(bool),
    DefaultFolder(Option<Folder>),
    Theme(ThemeChoice),
    /// The user resized the window.
    WindowSize(WindowSize),
}

#[derive(Debug)]
pub enum Event {
    /// App launched. `has_token` says whether the secret store held a token.
    Started {
        has_token: bool,
    },
    /// Time passed. Send regularly (a few times a second is plenty).
    Tick,

    SignInRequested,
    OpenLinkPage,
    DeviceCodeReceived(Result<DeviceCode, ApiError>),
    LinkPolled(Result<Option<Token>, ApiError>),
    SignOutRequested,

    /// A magnet link or `.torrent` the OS handed over (already validated).
    Intake(Intake),
    /// Something the OS handed over that failed validation.
    IntakeRejected(IntakeError),
    /// The shell could not use a `.torrent` file (unreadable, wrong content, too big).
    TorrentUnusable {
        op: OpId,
        reason: String,
    },
    AddFinished {
        op: OpId,
        result: Result<Option<i64>, ApiError>,
    },

    Polled {
        account: Result<Account, ApiError>,
        transfers: Result<Vec<Transfer>, ApiError>,
    },

    CancelRequested(i64),
    CancelFinished {
        op: OpId,
        result: Result<(), ApiError>,
    },
    RetryRequested(i64),
    RetryFinished {
        op: OpId,
        result: Result<(), ApiError>,
    },
    /// Remove every finished transfer from the list (the files stay in the account).
    ClearRequested,
    ClearFinished {
        op: OpId,
        result: Result<(), ApiError>,
    },
    /// Copy the put.io page of the transfer's file to the clipboard.
    CopyLinkRequested(i64),
    /// Copy the magnet link or URL the transfer was added from.
    CopySourceRequested(i64),
    /// Download the transfer's file or folder to this computer (through the browser).
    DownloadRequested(i64),
    DownloadPrepared {
        op: OpId,
        result: Result<Prepared, ApiError>,
    },
    ZipPolled {
        op: OpId,
        result: Result<ZipStatus, ApiError>,
    },
    /// Look at the system: is this app the default for magnet links and `.torrent` files?
    DefaultHandlerCheckRequested,
    DefaultHandlerChecked(handler::Check),
    /// Make this app the default (from the question, or from Preferences).
    DefaultHandlerRequested,
    /// The system was asked to change; it says what the defaults are now, or why not.
    DefaultHandlerRegistered(Result<handler::Status, String>),
    /// The question was answered "no": `forever` is "don't ask again".
    DefaultHandlerPromptDismissed {
        forever: bool,
    },
    /// Whether this copy of the app can replace itself, as the shell found out at start-up. The text is
    /// the reason when it cannot.
    InstallTargetKnown(Result<(), String>),
    /// Download and unpack the update on offer.
    UpdateInstallRequested,
    UpdateProgress {
        op: OpId,
        stage: UpdateStage,
        fraction: f32,
    },
    /// The download is unpacked and checked, or why it is not.
    UpdatePrepared {
        op: OpId,
        result: Result<(), String>,
    },
    /// Swap the prepared update in and restart.
    UpdateRestartRequested,
    /// The swap and the restart helper are done, or why they failed.
    UpdateApplied(Result<(), String>),
    StreamRequested(i64),
    StreamFinished {
        op: OpId,
        result: Result<(), StreamFailure>,
    },
    OpenTransferOnPutio(i64),
    /// The system was asked to open a download address in the browser, and said whether it took it.
    DownloadHandedOver {
        opened: bool,
    },
    /// The system could not open a page (the sign-in page, a transfer, the project page).
    PageOpenFailed,
    /// The place the sign-in is kept (the system keychain) did not do what was asked.
    SecretStoreFailed(SecretProblem),
    /// The preferences file could not be read at startup, or a change to it could not be written.
    SettingsFailed(SettingsProblem),
    NotificationClicked {
        file_id: Option<i64>,
    },
    OpenProjectPage,
    /// The "Buy me a coffee" link of the menu.
    OpenSupportPage,
    /// Open the release page of the update that was found (the URL comes from the validated update status).
    OpenReleasePage,

    ToggleTransfers,
    SettingsChanged(SettingsChange),

    /// Show another list under the header.
    SelectTab(Tab),
    /// The answer to `Effect::LoadFeeds`.
    FeedsLoaded(Result<Vec<RssFeed>, ApiError>),
    /// "Try again" after the feeds could not be loaded.
    FeedsRefreshRequested,
    /// Add a feed (`id: None`) or change one.
    FeedSaveRequested {
        id: Option<i64>,
        settings: RssFeedSettings,
    },
    FeedSaved {
        op: OpId,
        result: Result<(), ApiError>,
    },
    /// The add / edit form has seen how its request ended and closed.
    FeedSaveDismissed,
    FeedPauseRequested {
        id: i64,
        paused: bool,
    },
    FeedPauseFinished {
        op: OpId,
        id: i64,
        paused: bool,
        result: Result<(), ApiError>,
    },
    FeedDeleteRequested(i64),
    FeedCopyAddressRequested(i64),

    /// Go into a folder of the Files tab.
    FilesOpenFolder {
        id: i64,
        name: String,
    },
    /// Go back up to the `n`-th folder of the breadcrumb.
    FilesGoTo(usize),
    FilesRefreshRequested,
    /// The answer to `Effect::ListFiles`.
    FilesListed {
        parent: i64,
        result: Result<FileListing, ApiError>,
    },
    FilesSearchRequested(String),
    FilesSearchCleared,
    /// The answer to `Effect::SearchFiles`.
    FilesSearched {
        query: String,
        result: Result<FileSearch, ApiError>,
    },
    FileRenameRequested {
        id: i64,
        name: String,
    },
    /// Make a folder in the folder being shown.
    FolderCreateRequested(String),
    FileMoveRequested {
        id: i64,
        to: i64,
    },
    /// The answer to a rename, a new folder or a move.
    FileChangeFinished {
        op: OpId,
        result: Result<(), ApiError>,
    },
    /// The dialog has seen how its request ended and closed.
    FileActionDismissed,
    FileDeleteRequested(i64),
    FileDeleteFinished {
        op: OpId,
        id: i64,
        result: Result<(), ApiError>,
    },
    /// Files were dropped on the window: upload them into the folder the Files tab shows.
    FilesDropped(Vec<Dropped>),
    /// How many bytes of an upload have gone.
    UploadProgress {
        op: OpId,
        sent: u64,
    },
    UploadFinished {
        op: OpId,
        result: Result<(), UploadFailure>,
    },
    /// Stop an upload (or take it out of the queue).
    UploadCancelRequested(OpId),
    /// The row of an upload that failed was put away.
    UploadDismissed(OpId),
    /// Add a transfer from this pasted link (answered with `AddFinished`).
    AddLinkRequested(String),
    /// The *Add a link* card was closed or its message put away.
    AddLinkDismissed,
    /// The friends card was opened / closed.
    FriendsCardOpened,
    FriendsCardClosed,
    /// The answers to `Effect::FetchFriendList` and `Effect::FetchFriendRequests`.
    FriendListFetched(Result<Vec<Friend>, ApiError>),
    FriendRequestsFetched(Result<Vec<Friend>, ApiError>),
    /// Ask somebody to be a friend, answer a request, or end a friendship.
    FriendChangeRequested(FriendChange),
    FriendChangeFinished {
        op: OpId,
        change: FriendChange,
        result: Result<(), ApiError>,
    },
    /// The sharing card of a file was opened (named `name`) / closed.
    ShareOpened {
        file_id: i64,
        name: String,
    },
    ShareClosed,
    /// The answers to `Effect::FetchSharedWith` and `Effect::FetchFriends`.
    SharedWithFetched {
        file_id: i64,
        result: Result<Vec<Share>, ApiError>,
    },
    FriendsFetched {
        file_id: i64,
        result: Result<Vec<Friend>, ApiError>,
    },
    /// Share the open card's file with these friends / end one of its shares.
    ShareRequested(ShareWith),
    ShareFinished {
        op: OpId,
        file_id: i64,
        result: Result<(), ApiError>,
    },
    UnshareRequested(i64),
    UnshareFinished {
        op: OpId,
        file_id: i64,
        result: Result<(), ApiError>,
    },
    /// The answer to `Effect::ListShared`.
    SharedListed(Result<Vec<SharedFile>, ApiError>),
    /// The details panel of a transfer was opened / closed.
    TransferDetailsOpened(i64),
    TransferDetailsClosed,
    /// The answer to `Effect::FetchTransfer`.
    TransferDetailsFetched {
        id: i64,
        result: Result<Transfer, ApiError>,
    },
    /// Show the folder `folder` (named `name`) on the Files tab, wherever the user was in it before.
    FilesReveal {
        folder: i64,
        name: String,
    },
    /// Unpack the archive `id` into a folder next to it (`password` when it has one).
    FileExtractRequested {
        id: i64,
        password: Option<Secret>,
    },
    /// The answer to `Effect::ExtractFile`.
    FileExtractStarted {
        op: OpId,
        id: i64,
        result: Result<Vec<Extraction>, ApiError>,
    },
    /// The answer to `Effect::ListExtractions`.
    ExtractionsListed(Result<Vec<Extraction>, ApiError>),
    /// The user gave up on entering a password.
    FileExtractPasswordDismissed,
    FileOpenRequested(i64),
    FileCopyLinkRequested(i64),
    FileDownloadRequested(i64),
    FileStreamRequested(i64),
    FeedDeleteFinished {
        op: OpId,
        id: i64,
        result: Result<(), ApiError>,
    },

    CheckForUpdates {
        user_initiated: bool,
    },
    ReleaseFetched(Result<Release, ApiError>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub title: String,
    pub body: String,
    /// File to open on put.io when the notification is clicked.
    pub file_id: Option<i64>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Effect {
    RequestDeviceCode,
    PollDeviceCode {
        code: String,
    },
    StoreToken(Token),
    DeleteToken,
    SaveSettings(Settings),
    /// Fetch account info and the transfer list, then send [`Event::Polled`].
    Poll,
    AddMagnet {
        op: OpId,
        uri: String,
        parent: Option<i64>,
    },
    UploadTorrent {
        op: OpId,
        path: PathBuf,
        parent: Option<i64>,
        /// Move the file to the Trash once put.io has taken it (the setting, as it was when the upload started).
        trash_after: bool,
    },
    CancelTransfer {
        op: OpId,
        id: i64,
    },
    RetryTransfer {
        op: OpId,
        id: i64,
    },
    CleanTransfers {
        op: OpId,
        ids: Vec<i64>,
    },
    /// Fetch the account's RSS feeds (answered with `Event::FeedsLoaded`).
    LoadFeeds,
    /// Add a feed (`id: None`) or change one.
    SaveFeed {
        op: OpId,
        id: Option<i64>,
        settings: RssFeedSettings,
    },
    SetFeedPaused {
        op: OpId,
        id: i64,
        paused: bool,
    },
    DeleteFeed {
        op: OpId,
        id: i64,
    },
    /// List the files in a folder (answered with `Event::FilesListed`).
    ListFiles {
        parent: i64,
    },
    SearchFiles {
        query: String,
    },
    RenameFile {
        op: OpId,
        id: i64,
        name: String,
    },
    CreateFolder {
        op: OpId,
        parent: i64,
        name: String,
    },
    MoveFile {
        op: OpId,
        id: i64,
        to: i64,
    },
    DeleteFile {
        op: OpId,
        id: i64,
    },
    /// Upload the file at `path` (`size` bytes long) as `name` into folder `parent`. Answered with
    /// `UploadProgress` now and then and then `UploadFinished`.
    UploadFile {
        op: OpId,
        path: std::path::PathBuf,
        name: String,
        size: u64,
        parent: i64,
    },
    /// Stop that upload.
    CancelUpload {
        op: OpId,
    },
    FetchFriendList,
    FetchFriendRequests,
    ChangeFriend {
        op: OpId,
        change: FriendChange,
    },
    FetchSharedWith {
        file_id: i64,
    },
    FetchFriends {
        file_id: i64,
    },
    ShareFile {
        op: OpId,
        file_id: i64,
        with: ShareWith,
    },
    UnshareFile {
        op: OpId,
        file_id: i64,
        share_id: i64,
    },
    /// Ask for the files the account shares (answered with `Event::SharedListed`).
    ListShared,
    /// Fetch one transfer (answered with `Event::TransferDetailsFetched`).
    FetchTransfer {
        id: i64,
    },
    /// Unpack an archive (answered with `Event::FileExtractStarted`).
    ExtractFile {
        op: OpId,
        id: i64,
        password: Option<Secret>,
    },
    /// List the running extractions (answered with `Event::ExtractionsListed`).
    ListExtractions,
    /// Put text on the system clipboard. Only ever built here, from validated data.
    CopyText(String),
    /// Ask the system whether this app is the default for magnet links and `.torrent` files.
    CheckDefaultHandler,
    /// Make this app the default for both, then report with [`Event::DefaultHandlerRegistered`].
    RegisterDefaultHandler,
    /// Work out what to download: a file's signed address, or a ZIP for a folder.
    PrepareDownload {
        op: OpId,
        file_id: i64,
    },
    PollZip {
        op: OpId,
        zip_id: i64,
    },
    /// Hand a checked put.io download address to the browser. Never logged: it is a credential.
    OpenDownload(String),
    /// Download, unpack and check the update, reporting [`Event::UpdateProgress`] as it goes and
    /// [`Event::UpdatePrepared`] at the end. Nothing on disk is replaced yet.
    PrepareUpdate {
        op: OpId,
        asset: UpdateAsset,
    },
    /// Replace the app with the prepared update and arrange for it to start once this process is
    /// gone. The shell answers with [`Event::UpdateApplied`].
    ApplyUpdate,
    /// Resolve a playable URL for the file (or the videos inside a folder) and hand it to the player.
    Stream {
        op: OpId,
        file_id: i64,
    },
    FetchLatestRelease,
    Notify(Notification),
    /// Dock/taskbar progress, `None` to clear.
    SetBadge(Option<u8>),
    /// Only ever `https://put.io/…` or the project page, built from numeric ids and constants here.
    OpenUrl(String),
    Quit,
}

#[derive(Debug, Clone)]
struct Notice {
    msg: Msg,
    until: Millis,
}

#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)] // independent flags of one state machine
pub struct State {
    pub settings: Settings,
    pub locale: Locale,
    pub auth: Auth,
    pub account: Option<Account>,
    pub transfers: Vec<Transfer>,
    pub summary: Summary,
    pub transfers_expanded: bool,
    /// The list showing under the header.
    pub tab: Tab,
    /// The account's RSS feeds as last fetched.
    pub feeds: FeedList,
    /// The Files tab: the folder being shown, or the search.
    pub files: FilesView,
    /// The transfer whose details panel is open.
    pub detail: Option<TransferDetail>,
    /// Where adding a link from the *Add a link* card has got to.
    pub link_add: LinkAdd,
    /// Files being uploaded or waiting for their turn, in the order they were dropped.
    pub uploads: Vec<Upload>,
    /// The open friends card.
    pub friends_card: Option<FriendsView>,
    /// The open sharing card.
    pub sharing: Option<SharingView>,
    /// The files the account shares.
    pub shared: SharedFiles,
    /// Archives being unpacked on put.io.
    pub unpacking: Vec<Unpacking>,
    pub update: UpdateStatus,
    pub install: Install,
    pub install_target: InstallTarget,
    /// What the system said about the default app, once asked.
    pub handler_check: Option<handler::Check>,
    /// The question "make this the default app?" is showing.
    pub handler_prompt: bool,
    /// The system is being asked to change the default.
    pub handler_busy: bool,
    /// Which release file this computer installs for a tag (set by the shell: it depends on the OS and CPU).
    pub asset_name: fn(&str) -> Option<String>,
    pub current_version: String,

    base: Msg,
    notice: Option<Notice>,
    busy: u32,
    ops: HashMap<OpId, Op>,
    next_op: u64,
    pending_intake: VecDeque<Intake>,
    tracker: Tracker,
    poll_in_flight: bool,
    next_poll_at: Option<Millis>,
    zip: Option<ZipJob>,
    feed_save: FeedSave,
    feed_refresh: refresh::Refresh,
    file_action: FileAction,
    file_refresh: refresh::Refresh,
    detail_refresh: refresh::Refresh,
    shared_refresh: refresh::Refresh,
    /// The transfer whose fetch is on its way.
    detail_requested: Option<i64>,
    extract_ask: Option<PasswordAsk>,
    extract_refresh: refresh::Refresh,
    /// The folder whose listing is on its way.
    files_requested: Option<i64>,
    failures: u32,
    close_at: Option<Millis>,
    badge: Option<u8>,
    update_user_initiated: bool,
    /// A request for the latest release is on its way.
    update_check_in_flight: bool,
    next_update_check_at: Option<Millis>,
    /// What to say once the browser has taken the download address.
    opening: Option<Msg>,
    /// The question has been shown in this run: one time is enough.
    handler_asked: bool,
}

impl State {
    #[must_use]
    pub fn new(settings: Settings, locale: Locale, current_version: &str) -> Self {
        Self {
            transfers_expanded: settings.transfers_expanded,
            tab: settings.tab,
            feeds: FeedList::default(),
            files: FilesView::default(),
            detail: None,
            link_add: LinkAdd::default(),
            uploads: Vec::new(),
            friends_card: None,
            sharing: None,
            shared: SharedFiles::default(),
            unpacking: Vec::new(),
            settings,
            locale,
            auth: Auth::Starting,
            account: None,
            transfers: Vec::new(),
            summary: Summary::default(),
            update: UpdateStatus::Unknown,
            install: Install::Idle,
            install_target: InstallTarget::Unknown,
            handler_check: None,
            handler_prompt: false,
            handler_busy: false,
            asset_name: |_| None,
            current_version: current_version.to_owned(),
            base: Msg::FetchingInfo,
            notice: None,
            busy: 0,
            ops: HashMap::new(),
            next_op: 1,
            pending_intake: VecDeque::new(),
            tracker: Tracker::new(),
            poll_in_flight: false,
            next_poll_at: None,
            zip: None,
            feed_save: FeedSave::Idle,
            feed_refresh: refresh::Refresh::default(),
            file_action: FileAction::Idle,
            file_refresh: refresh::Refresh::default(),
            detail_refresh: refresh::Refresh::default(),
            shared_refresh: refresh::Refresh::default(),
            detail_requested: None,
            extract_ask: None,
            extract_refresh: refresh::Refresh::default(),
            files_requested: None,
            failures: 0,
            close_at: None,
            badge: None,
            update_user_initiated: false,
            update_check_in_flight: false,
            next_update_check_at: None,
            opening: None,
            handler_asked: false,
        }
    }

    /// The archive that needs a password the app does not have, if any.
    #[must_use]
    pub fn extract_ask(&self) -> Option<&PasswordAsk> {
        self.extract_ask.as_ref()
    }

    /// How the rename, new folder or move dialog's request stands.
    #[must_use]
    pub fn file_action(&self) -> &FileAction {
        &self.file_action
    }

    /// How the add / edit feed form's request stands.
    #[must_use]
    pub fn feed_save(&self) -> &FeedSave {
        &self.feed_save
    }

    /// The message for the status line right now: a recent notice, else the standing message.
    #[must_use]
    pub fn status(&self, now: Millis) -> &Msg {
        match &self.notice {
            Some(notice) if now < notice.until => &notice.msg,
            _ => &self.base,
        }
    }

    #[must_use]
    pub fn status_text(&self, now: Millis) -> String {
        self.status(now).text(self.locale)
    }

    /// An operation (add, upload, cancel, stream) is running: show the spinner.
    #[must_use]
    pub fn is_busy(&self) -> bool {
        self.busy > 0
    }

    #[must_use]
    pub fn is_signed_in(&self) -> bool {
        self.auth == Auth::SignedIn
    }

    /// Whether the app is going to quit by itself soon (the UI may show a hint).
    #[must_use]
    pub fn closing_soon(&self) -> bool {
        self.close_at.is_some()
    }

    fn op(&mut self, kind: Op) -> OpId {
        let id = OpId(self.next_op);
        self.next_op += 1;
        self.ops.insert(id, kind);
        self.busy += 1;
        id
    }

    /// Finish an operation. Returns `false` for an unknown (stale or duplicate) id, which is ignored.
    fn finish(&mut self, id: OpId, expected: &[Op]) -> bool {
        match self.ops.get(&id) {
            Some(kind) if expected.contains(kind) => {
                self.ops.remove(&id);
                self.busy = self.busy.saturating_sub(1);
                true
            }
            _ => false,
        }
    }

    fn say(&mut self, msg: Msg, now: Millis, hold: Millis) {
        self.notice = Some(Notice {
            msg,
            until: now.saturating_add(hold),
        });
    }

    fn say_until_replaced(&mut self, msg: Msg) {
        self.notice = Some(Notice {
            msg,
            until: FOREVER,
        });
    }

    fn text(&self, msg: &Msg) -> String {
        msg.text(self.locale)
    }

    /// Short, user-facing reason for a failed API call.
    fn reason(&self, error: &ApiError) -> String {
        match error {
            ApiError::Network(_) => self.text(&Msg::Offline),
            ApiError::RateLimited { retry_after } => self.text(&Msg::RateLimited {
                seconds: retry_after.as_secs(),
            }),
            ApiError::Api(failure) if failure.status >= 500 => self.text(&Msg::ServerError),
            // Payment required: put.io's way of saying the account has no room (or no plan) left, whatever
            // the body says (an upload's answer often has none).
            ApiError::Api(failure) if failure.status == 402 => self.text(&Msg::PaymentRequired),
            ApiError::Api(failure) => failure
                .message
                .chars()
                .filter(|c| !c.is_control())
                .take(MAX_REASON_CHARS)
                .collect(),
            ApiError::InvalidInput(why) => (*why).to_owned(),
            ApiError::File(why) => why
                .chars()
                .filter(|c| !c.is_control())
                .take(MAX_REASON_CHARS)
                .collect(),
            ApiError::Unauthorized | ApiError::Decode(_) | ApiError::TooLarge => {
                self.text(&Msg::ServerError)
            }
        }
    }

    fn schedule_poll(&mut self, at: Millis) {
        self.next_poll_at = Some(at);
    }

    fn backoff(&self, error: &ApiError) -> Millis {
        if let ApiError::RateLimited { retry_after } = error {
            let ms = u64::try_from(retry_after.as_millis()).unwrap_or(MAX_BACKOFF);
            return ms.min(MAX_BACKOFF);
        }
        POLL_INTERVAL
            .saturating_mul(1u64 << self.failures.min(4))
            .min(MAX_BACKOFF)
    }
}

/// Show another list under the header, remember it, and ask for what is old in it.
fn select_tab(state: &mut State, now: Millis, tab: Tab, fx: &mut Vec<Effect>) {
    if state.tab == tab {
        return;
    }
    state.tab = tab;
    state.settings.tab = tab;
    fx.push(Effect::SaveSettings(state.settings.clone()));
    match tab {
        Tab::Feeds => feeds::tab_opened(state, now),
        Tab::Files => files::tab_opened(state, now),
        Tab::Transfers => {}
    }
}

/// Start handing a file (or a folder, as a ZIP) to the browser. `None`: there is no file to hand over yet.
fn start_download(state: &mut State, now: Millis, file_id: Option<i64>, fx: &mut Vec<Effect>) {
    match file_id {
        _ if !state.is_signed_in() => {}
        None => state.say(Msg::DownloadNotReady, now, NOTICE_TIME),
        Some(_) if state.zip.is_some() => state.say(Msg::DownloadBusy, now, NOTICE_TIME),
        Some(file_id) => {
            state.close_at = None;
            let op = state.op(Op::Download);
            state.say_until_replaced(Msg::DownloadPreparing);
            fx.push(Effect::PrepareDownload { op, file_id });
        }
    }
}

fn open_file_url(file_id: Option<i64>) -> String {
    match file_id {
        Some(id) if id >= 0 => format!("https://put.io/files/{id}"),
        _ => "https://put.io/transfers".to_owned(),
    }
}

/// Apply one event. See the module documentation.
#[allow(clippy::too_many_lines)] // one flat dispatch is easier to audit than many small indirections
pub fn update(state: &mut State, now: Millis, event: Event) -> Vec<Effect> {
    let mut fx = Vec::new();
    match event {
        Event::Started { has_token } => {
            if state.settings.check_for_updates {
                start_update_check(state, false, &mut fx);
            }
            if has_token {
                become_signed_in(state, now, Msg::FetchingInfo, &mut fx);
            } else {
                begin_sign_in(state, &mut fx);
            }
        }
        Event::Tick => tick(state, now, &mut fx),

        Event::SignInRequested => {
            if matches!(state.auth, Auth::SignedOut(_) | Auth::Linking(_)) {
                begin_sign_in(state, &mut fx);
            }
        }
        Event::OpenLinkPage => fx.push(Effect::OpenUrl(LINK_URL.to_owned())),
        Event::DeviceCodeReceived(result) => {
            if state.auth == Auth::RequestingCode {
                match result {
                    Ok(DeviceCode { code }) => {
                        state.auth = Auth::Linking(Linking {
                            code,
                            next_poll_at: now + LINK_POLL_INTERVAL,
                            deadline: now + LINK_TTL,
                            in_flight: false,
                        });
                        state.base = Msg::SigningIn;
                    }
                    Err(error) => {
                        state.auth = Auth::SignedOut(Some(SignedOutReason::CodeRequestFailed));
                        state.base = Msg::AuthRequired;
                        let reason = state.reason(&error);
                        state.say(Msg::SignInFailed { reason }, now, ERROR_NOTICE_TIME);
                    }
                }
            }
        }
        Event::LinkPolled(result) => link_polled(state, now, result, &mut fx),
        Event::SignOutRequested => {
            sign_out(state, &mut fx);
            state.auth = Auth::SignedOut(None);
        }

        Event::Intake(item) => intake(state, item, &mut fx),
        Event::IntakeRejected(error) => {
            let msg = match error {
                IntakeError::Magnet(_) => Msg::NotAMagnet,
                other => Msg::NotATorrent {
                    reason: other.to_string(),
                },
            };
            state.say(msg, now, ERROR_NOTICE_TIME);
        }
        Event::TorrentUnusable { op, reason } => {
            if state.finish(op, &[Op::UploadTorrent]) {
                state.say(Msg::NotATorrent { reason }, now, ERROR_NOTICE_TIME);
            }
        }
        Event::AddFinished { op, result } => add_finished(state, now, op, result, &mut fx),

        Event::Polled { account, transfers } => polled(state, now, account, transfers, &mut fx),

        Event::CancelRequested(id) => {
            if state.is_signed_in() {
                state.close_at = None;
                let op = state.op(Op::Cancel);
                state.say_until_replaced(Msg::Canceling);
                fx.push(Effect::CancelTransfer { op, id });
            }
        }
        Event::CancelFinished { op, result } => {
            if state.finish(op, &[Op::Cancel]) {
                match result {
                    Ok(()) => {
                        state.say(Msg::Canceled, now, NOTICE_TIME);
                        state.schedule_poll(now);
                    }
                    Err(ApiError::Unauthorized) => session_expired(state, &mut fx),
                    Err(_) => state.say(Msg::CancelFailed, now, ERROR_NOTICE_TIME),
                }
            }
        }
        Event::RetryRequested(id) => {
            if state.is_signed_in() && state.transfers.iter().any(|t| t.id == id) {
                state.close_at = None;
                let op = state.op(Op::Retry);
                state.say_until_replaced(Msg::Retrying);
                fx.push(Effect::RetryTransfer { op, id });
            }
        }
        Event::RetryFinished { op, result } => {
            if state.finish(op, &[Op::Retry]) {
                match result {
                    Ok(()) => {
                        state.say(Msg::Retried, now, NOTICE_TIME);
                        state.schedule_poll(now);
                    }
                    Err(ApiError::Unauthorized) => session_expired(state, &mut fx),
                    Err(_) => state.say(Msg::RetryFailed, now, ERROR_NOTICE_TIME),
                }
            }
        }
        Event::ClearRequested => {
            let ids = transfers::clearable_ids(&state.transfers);
            if state.is_signed_in() && !ids.is_empty() {
                state.close_at = None;
                let op = state.op(Op::Clear);
                state.say_until_replaced(Msg::Clearing);
                fx.push(Effect::CleanTransfers { op, ids });
            }
        }
        Event::ClearFinished { op, result } => {
            if state.finish(op, &[Op::Clear]) {
                match result {
                    Ok(()) => {
                        state.say(Msg::Cleared, now, NOTICE_TIME);
                        state.schedule_poll(now);
                    }
                    Err(ApiError::Unauthorized) => session_expired(state, &mut fx),
                    Err(_) => state.say(Msg::ClearFailed, now, ERROR_NOTICE_TIME),
                }
            }
        }
        Event::CopyLinkRequested(id) => {
            let file_id = state
                .transfers
                .iter()
                .find(|t| t.id == id)
                .and_then(|t| t.file_id);
            if let Some(file_id) = file_id.filter(|id| *id >= 0) {
                fx.push(Effect::CopyText(open_file_url(Some(file_id))));
                state.say(Msg::Copied, now, NOTICE_TIME);
            }
        }
        Event::CopySourceRequested(id) => {
            let text = state
                .transfers
                .iter()
                .find(|t| t.id == id)
                .and_then(transfers::copyable_source)
                .map(|source| source.text.to_owned());
            if let Some(text) = text {
                fx.push(Effect::CopyText(text));
                state.say(Msg::Copied, now, NOTICE_TIME);
            }
        }
        Event::DownloadRequested(id) => {
            let file_id = state
                .transfers
                .iter()
                .find(|t| t.id == id)
                .and_then(|t| t.file_id)
                .filter(|file_id| *file_id >= 0);
            start_download(state, now, file_id, &mut fx);
        }
        Event::DownloadPrepared { op, result } => {
            if state.ops.get(&op) == Some(&Op::Download) {
                match result {
                    Ok(Prepared::Url(url)) => open_download(state, now, op, &url, 0, &mut fx),
                    Ok(Prepared::Zip(zip_id)) if state.zip.is_none() => {
                        state.zip = Some(ZipJob {
                            op,
                            zip_id,
                            deadline: now.saturating_add(ZIP_TTL),
                            next_poll_at: now,
                            in_flight: false,
                        });
                    }
                    Ok(Prepared::Zip(_)) => {
                        state.finish(op, &[Op::Download]);
                        state.say(Msg::DownloadBusy, now, NOTICE_TIME);
                    }
                    Err(error) => download_failed(state, now, op, &error, &mut fx),
                }
            }
        }
        Event::ZipPolled { op, result } => zip_polled(state, now, op, result, &mut fx),
        Event::StreamRequested(id) => {
            if let Some(transfer) = state.transfers.iter().find(|t| t.id == id) {
                match transfer.file_id {
                    Some(file_id) => {
                        state.close_at = None;
                        let op = state.op(Op::Stream);
                        state.say_until_replaced(Msg::Streaming);
                        fx.push(Effect::Stream { op, file_id });
                    }
                    None => state.say(Msg::StreamNotReady, now, ERROR_NOTICE_TIME),
                }
            }
        }
        Event::StreamFinished { op, result } => {
            if state.finish(op, &[Op::Stream]) {
                match result {
                    Ok(()) => state.say(Msg::Streaming, now, NOTICE_TIME),
                    Err(StreamFailure::NoPlayer) => {
                        state.say(Msg::StreamNoPlayer, now, ERROR_NOTICE_TIME);
                    }
                    Err(StreamFailure::NothingToPlay) => {
                        state.say(Msg::StreamNothingToPlay, now, ERROR_NOTICE_TIME);
                    }
                    Err(StreamFailure::Api(ApiError::Unauthorized)) => {
                        session_expired(state, &mut fx);
                    }
                    Err(StreamFailure::LaunchFailed | StreamFailure::Api(_)) => {
                        state.say(Msg::StreamFailed, now, ERROR_NOTICE_TIME);
                    }
                }
            }
        }
        Event::OpenTransferOnPutio(id) => {
            let file_id = state
                .transfers
                .iter()
                .find(|t| t.id == id)
                .and_then(|t| t.file_id);
            fx.push(Effect::OpenUrl(open_file_url(file_id)));
        }
        Event::DownloadHandedOver { opened } => {
            if let Some(started) = state.opening.take() {
                if opened {
                    state.say(started, now, NOTICE_TIME);
                } else {
                    state.say(Msg::DownloadOpenFailed, now, ERROR_NOTICE_TIME);
                }
            }
        }
        Event::PageOpenFailed => state.say(Msg::OpenFailed, now, ERROR_NOTICE_TIME),
        Event::SecretStoreFailed(problem) => {
            let msg = match problem {
                SecretProblem::Unavailable => Msg::SecretStoreUnavailable,
                SecretProblem::Load => Msg::SecretLoadFailed,
                SecretProblem::Save => Msg::SecretSaveFailed,
                SecretProblem::Delete => Msg::SecretDeleteFailed,
            };
            state.say(msg, now, ERROR_NOTICE_TIME);
        }
        Event::SettingsFailed(problem) => {
            let msg = match problem {
                SettingsProblem::Unreadable => Msg::SettingsUnreadable,
                SettingsProblem::NotSaved => Msg::SettingsNotSaved,
            };
            // Stays until something else is said: the user must not miss that changes are being lost.
            state.say_until_replaced(msg);
        }
        Event::NotificationClicked { file_id } => fx.push(Effect::OpenUrl(open_file_url(file_id))),
        Event::OpenProjectPage => fx.push(Effect::OpenUrl(PROJECT_URL.to_owned())),
        Event::OpenSupportPage => fx.push(Effect::OpenUrl(SUPPORT_URL.to_owned())),
        Event::OpenReleasePage => {
            if let UpdateStatus::Available { url, .. } = &state.update {
                fx.push(Effect::OpenUrl(url.clone()));
            }
        }

        Event::ToggleTransfers => {
            state.transfers_expanded = !state.transfers_expanded;
            state.settings.transfers_expanded = state.transfers_expanded;
            fx.push(Effect::SaveSettings(state.settings.clone()));
        }
        Event::SelectTab(tab) => select_tab(state, now, tab, &mut fx),
        Event::FeedsLoaded(result) => feeds::loaded(state, now, result, &mut fx),
        Event::FeedsRefreshRequested => feeds::retry(state, now),
        Event::FeedSaveRequested { id, settings } => {
            feeds::save_requested(state, id, settings, &mut fx);
        }
        Event::FeedSaved { op, result } => feeds::save_finished(state, now, op, result, &mut fx),
        Event::FeedSaveDismissed => feeds::save_dismissed(state),
        Event::FeedPauseRequested { id, paused } => {
            feeds::pause_requested(state, id, paused, &mut fx);
        }
        Event::FeedPauseFinished {
            op,
            id,
            paused,
            result,
        } => feeds::pause_finished(state, now, (op, id, paused), result, &mut fx),
        Event::FeedDeleteRequested(id) => feeds::delete_requested(state, id, &mut fx),
        Event::FeedDeleteFinished { op, id, result } => {
            feeds::delete_finished(state, now, (op, id), result, &mut fx);
        }
        Event::FeedCopyAddressRequested(id) => feeds::copy_address(state, now, id, &mut fx),
        Event::FilesOpenFolder { id, name } => files::open_folder(state, now, id, &name),
        Event::FilesGoTo(index) => files::go_to(state, now, index),
        Event::FilesRefreshRequested => files::retry(state, now),
        Event::FilesListed { parent, result } => {
            files::listed(state, now, parent, result, &mut fx);
        }
        Event::FilesSearchRequested(query) => files::search_requested(state, &query, &mut fx),
        Event::FilesSearchCleared => state.files.search = None,
        Event::FilesSearched { query, result } => {
            files::searched(state, &query, result, &mut fx);
        }
        Event::FileRenameRequested { id, name } => {
            files::rename_requested(state, id, &name, &mut fx);
        }
        Event::FolderCreateRequested(name) => files::create_requested(state, &name, &mut fx),
        Event::FileMoveRequested { id, to } => files::move_requested(state, id, to, &mut fx),
        Event::FileChangeFinished { op, result } => {
            files::changed(state, now, op, result, &mut fx);
        }
        Event::FileActionDismissed => files::action_dismissed(state),
        Event::FileDeleteRequested(id) => files::delete_requested(state, id, &mut fx),
        Event::FileDeleteFinished { op, id, result } => {
            files::delete_finished(state, now, (op, id), result, &mut fx);
        }
        Event::AddLinkRequested(raw) => links::requested(state, &raw, &mut fx),
        Event::AddLinkDismissed => links::dismissed(state),
        Event::FilesDropped(items) => uploads::dropped(state, now, items, &mut fx),
        Event::UploadProgress { op, sent } => uploads::progress(state, op, sent),
        Event::UploadFinished { op, result } => uploads::finished(state, now, op, result),
        Event::UploadCancelRequested(op) => uploads::cancel_requested(state, op, &mut fx),
        Event::UploadDismissed(op) => uploads::dismissed(state, op),
        Event::FriendsCardOpened => friends::opened(state, &mut fx),
        Event::FriendsCardClosed => friends::closed(state),
        Event::FriendListFetched(result) => friends::list_fetched(state, result),
        Event::FriendRequestsFetched(result) => friends::requests_fetched(state, result),
        Event::FriendChangeRequested(change) => {
            friends::change_requested(state, &change, &mut fx);
        }
        Event::FriendChangeFinished { op, change, result } => {
            friends::change_finished(state, (op, change), result, &mut fx);
        }
        Event::ShareOpened { file_id, name } => shares::opened(state, file_id, &name, &mut fx),
        Event::ShareClosed => shares::closed(state),
        Event::SharedWithFetched { file_id, result } => {
            shares::shares_fetched(state, file_id, result);
        }
        Event::FriendsFetched { file_id, result } => {
            shares::friends_fetched(state, file_id, result);
        }
        Event::ShareRequested(with) => shares::share_requested(state, with, &mut fx),
        Event::ShareFinished {
            op,
            file_id,
            result,
        } => shares::share_finished(state, (op, file_id), result, &mut fx),
        Event::UnshareRequested(share_id) => shares::unshare_requested(state, share_id, &mut fx),
        Event::UnshareFinished {
            op,
            file_id,
            result,
        } => shares::unshare_finished(state, (op, file_id), result, &mut fx),
        Event::SharedListed(result) => shares::listed(state, now, result),
        Event::TransferDetailsOpened(id) => detail::opened(state, now, id),
        Event::TransferDetailsClosed => detail::closed(state),
        Event::TransferDetailsFetched { id, result } => {
            detail::fetched(state, now, id, result, &mut fx);
        }
        Event::FilesReveal { folder, name } => files::reveal(state, now, folder, &name),
        Event::FileExtractRequested { id, password } => {
            extract::requested(state, id, password, &mut fx);
        }
        Event::FileExtractStarted { op, id, result } => {
            extract::started(state, now, (op, id), result, &mut fx);
        }
        Event::ExtractionsListed(result) => extract::listed(state, now, result, &mut fx),
        Event::FileExtractPasswordDismissed => extract::password_dismissed(state),
        Event::FileOpenRequested(id) => files::open_on_putio(state, id, &mut fx),
        Event::FileCopyLinkRequested(id) => files::copy_link(state, now, id, &mut fx),
        Event::FileDownloadRequested(id) => files::download(state, now, id, &mut fx),
        Event::FileStreamRequested(id) => files::stream(state, id, &mut fx),
        Event::SettingsChanged(change) => {
            match change {
                SettingsChange::CloseAfterMagnet(on) => state.settings.close_after_magnet = on,
                SettingsChange::CloseAfterTorrent(on) => state.settings.close_after_torrent = on,
                SettingsChange::TrashTorrentAfterUpload(on) => {
                    state.settings.trash_torrent_after_upload = on;
                }
                SettingsChange::CheckForUpdates(on) => state.settings.check_for_updates = on,
                SettingsChange::DefaultFolder(folder) => state.settings.default_folder = folder,
                SettingsChange::Theme(theme) => state.settings.theme = theme,
                SettingsChange::WindowSize(size) => {
                    state.settings.window_size = Some(size.clamped());
                }
            }
            fx.push(Effect::SaveSettings(state.settings.clone()));
        }

        Event::CheckForUpdates { user_initiated } => {
            start_update_check(state, user_initiated, &mut fx);
        }
        Event::ReleaseFetched(result) => release_fetched(state, now, result),
        Event::DefaultHandlerCheckRequested => fx.push(Effect::CheckDefaultHandler),
        Event::DefaultHandlerChecked(check) => {
            if handler::should_ask(
                &check,
                state.settings.ask_default_handler,
                state.handler_asked,
            ) {
                state.handler_prompt = true;
                state.handler_asked = true;
            }
            state.handler_check = Some(check);
        }
        Event::DefaultHandlerRequested => {
            let possible = matches!(state.handler_check, Some(handler::Check::Known(_)));
            if possible && !state.handler_busy {
                state.handler_busy = true;
                state.handler_prompt = false;
                state.say_until_replaced(Msg::HandlerRegistering);
                fx.push(Effect::RegisterDefaultHandler);
            }
        }
        Event::DefaultHandlerRegistered(result) => {
            state.handler_busy = false;
            state.handler_prompt = false;
            match result {
                Ok(status) => {
                    state.handler_check = Some(handler::Check::Known(status));
                    if status.is_default() {
                        state.say(Msg::HandlerSet, now, NOTICE_TIME);
                    } else {
                        let reason = state.text(&Msg::HandlerKeptOther);
                        state.say(Msg::HandlerFailed { reason }, now, ERROR_NOTICE_TIME);
                    }
                }
                Err(reason) => {
                    let reason: String = reason
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(240)
                        .collect();
                    state.say(Msg::HandlerFailed { reason }, now, ERROR_NOTICE_TIME);
                }
            }
        }
        Event::DefaultHandlerPromptDismissed { forever } => {
            state.handler_prompt = false;
            if forever {
                state.settings.ask_default_handler = false;
                fx.push(Effect::SaveSettings(state.settings.clone()));
            }
        }
        Event::InstallTargetKnown(result) => {
            state.install_target = match result {
                Ok(()) => InstallTarget::Ready,
                Err(reason) => InstallTarget::Unavailable(reason),
            };
        }
        Event::UpdateInstallRequested => update_install_requested(state, &mut fx),
        Event::UpdateProgress {
            op,
            stage,
            fraction,
        } => {
            if state.ops.get(&op) == Some(&Op::Update)
                && let Install::Working {
                    fraction: shown, ..
                } = &state.install
            {
                // The bar only moves forward.
                let fraction = fraction.clamp(0.0, 1.0).max(*shown);
                state.install = Install::Working { stage, fraction };
                let msg = match stage {
                    UpdateStage::Downloading => Msg::UpdateDownloading {
                        percent: percent_of(fraction),
                    },
                    UpdateStage::Unpacking => Msg::UpdateUnpacking,
                };
                state.say_until_replaced(msg);
            }
        }
        Event::UpdatePrepared { op, result } => {
            if state.finish(op, &[Op::Update]) {
                let version = available_version(&state.update);
                match result {
                    Ok(()) => {
                        let msg = Msg::UpdateReady {
                            version: version.clone(),
                        };
                        state.install = Install::Ready { version };
                        state.say(msg, now, ERROR_NOTICE_TIME);
                    }
                    Err(reason) => update_failed(state, now, version, &reason),
                }
            }
        }
        Event::UpdateRestartRequested => {
            if matches!(state.install, Install::Ready { .. }) {
                state.close_at = None;
                state.say_until_replaced(Msg::UpdateRestarting);
                fx.push(Effect::ApplyUpdate);
            }
        }
        Event::UpdateApplied(result) => {
            if let Install::Ready { version } = &state.install {
                match result {
                    Ok(()) => fx.push(Effect::Quit),
                    Err(reason) => {
                        let version = version.clone();
                        update_failed(state, now, version, &reason);
                    }
                }
            }
        }
    }

    maybe_poll(state, now, &mut fx);
    fx
}

fn begin_sign_in(state: &mut State, fx: &mut Vec<Effect>) {
    state.auth = Auth::RequestingCode;
    state.base = Msg::AuthRequired;
    fx.push(Effect::RequestDeviceCode);
}

/// The one way into [`Auth::SignedIn`]: schedule the first poll and add anything the OS handed over
/// before we were ready (a cold-launch magnet link arrives before the first event loop turn).
fn become_signed_in(state: &mut State, now: Millis, base: Msg, fx: &mut Vec<Effect>) {
    state.auth = Auth::SignedIn;
    state.base = base;
    state.schedule_poll(now);
    let queued: Vec<Intake> = state.pending_intake.drain(..).collect();
    for item in queued {
        intake(state, item, fx);
    }
}

fn sign_out(state: &mut State, fx: &mut Vec<Effect>) {
    state.account = None;
    state.transfers.clear();
    state.summary = Summary::default();
    state.tracker.reset();
    state.poll_in_flight = false;
    state.next_poll_at = None;
    state.failures = 0;
    state.pending_intake.clear();
    state.close_at = None;
    state.zip = None;
    feeds::reset(state);
    files::reset(state);
    detail::reset(state);
    shares::reset(state);
    extract::reset(state);
    friends::reset(state);
    links::reset(state);
    uploads::reset(state, fx);
    // What was running for this account has no one to report to. (An update is not tied to the account.)
    state.ops.retain(|_, kind| *kind == Op::Update);
    state.busy = u32::try_from(state.ops.len()).unwrap_or(u32::MAX);
    state.base = Msg::AuthRequired;
    fx.push(Effect::DeleteToken);
    if state.badge.take().is_some() {
        fx.push(Effect::SetBadge(None));
    }
}

fn session_expired(state: &mut State, fx: &mut Vec<Effect>) {
    sign_out(state, fx);
    state.auth = Auth::SignedOut(Some(SignedOutReason::SessionExpired));
    state.base = Msg::AuthExpired;
    state.notice = None;
}

fn tick(state: &mut State, now: Millis, fx: &mut Vec<Effect>) {
    // The periodic update check: not while an update is being installed or waits for a restart.
    if state.next_update_check_at.is_some_and(|at| now >= at)
        && matches!(state.install, Install::Idle | Install::Failed { .. })
    {
        if state.settings.check_for_updates {
            start_update_check(state, false, fx);
        } else {
            state.next_update_check_at = None;
        }
    }
    // Never quit with work in flight: a slow upload behind a finished one must not be killed.
    if state.close_at.is_some_and(|at| now >= at) && !state.is_busy() {
        state.close_at = None;
        fx.push(Effect::Quit);
    }
    zip_tick(state, now, fx);
    if let Auth::Linking(link) = &mut state.auth {
        if now >= link.deadline {
            state.auth = Auth::SignedOut(Some(SignedOutReason::CodeExpired));
            state.base = Msg::LinkExpired;
        } else if !link.in_flight && now >= link.next_poll_at {
            link.in_flight = true;
            fx.push(Effect::PollDeviceCode {
                code: link.code.clone(),
            });
        }
    }
}

fn percent_of(fraction: f32) -> u8 {
    // Clamped to 0..=100 first, so the cast cannot lose anything that matters.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let percent = (fraction.clamp(0.0, 1.0) * 100.0).round() as u8;
    percent
}

fn available_version(status: &UpdateStatus) -> String {
    match status {
        UpdateStatus::Available { version, .. } => version.clone(),
        _ => String::new(),
    }
}

/// A failure text from the shell: no control characters (it can contain file names), bounded.
fn update_failed(state: &mut State, now: Millis, version: String, reason: &str) {
    let reason: String = reason
        .chars()
        .filter(|c| !c.is_control())
        .take(240)
        .collect();
    state.install = Install::Failed {
        version,
        message: reason.clone(),
    };
    state.say(Msg::UpdateFailed { reason }, now, ERROR_NOTICE_TIME);
}

/// Ask GitHub for the latest release. A background check keeps showing an update that is already on
/// offer instead of flashing "checking" over it.
fn start_update_check(state: &mut State, user_initiated: bool, fx: &mut Vec<Effect>) {
    if user_initiated || !matches!(state.update, UpdateStatus::Available { .. }) {
        state.update = UpdateStatus::Checking;
    }
    state.next_update_check_at = None;
    if state.update_check_in_flight {
        // One request at a time. If the user is the one asking, the answer on its way is for them.
        state.update_user_initiated |= user_initiated;
        return;
    }
    state.update_user_initiated = user_initiated;
    state.update_check_in_flight = true;
    fx.push(Effect::FetchLatestRelease);
}

fn release_fetched(state: &mut State, now: Millis, result: Result<Release, ApiError>) {
    state.update_check_in_flight = false;
    let user_initiated = std::mem::take(&mut state.update_user_initiated);
    let was_on_offer = available_version(&state.update);
    let wait = if result.is_ok() {
        UPDATE_CHECK_INTERVAL
    } else {
        UPDATE_RETRY_INTERVAL
    };
    state.next_update_check_at = state
        .settings
        .check_for_updates
        .then(|| now.saturating_add(wait));
    if let Ok(release) = result {
        state.update = update::evaluate(&state.current_version, &release, &state.asset_name);
        match &state.update {
            UpdateStatus::Available { version, .. } => {
                // A failure about an older release is moot now.
                if matches!(&state.install, Install::Failed { version: failed, .. } if failed != version)
                {
                    state.install = Install::Idle;
                }
                // Say it once, not at every re-check.
                if *version != was_on_offer || user_initiated {
                    let msg = Msg::UpdateAvailable {
                        version: version.clone(),
                    };
                    state.say(msg, now, ERROR_NOTICE_TIME);
                }
            }
            _ if user_initiated => state.say(Msg::UpToDate, now, NOTICE_TIME),
            _ => {}
        }
    } else {
        // A failed refresh keeps an update that is already on offer.
        if !matches!(state.update, UpdateStatus::Available { .. }) {
            state.update = UpdateStatus::Unknown;
        }
        if user_initiated {
            state.say(Msg::UpdateCheckFailed, now, ERROR_NOTICE_TIME);
        }
    }
}

fn update_install_requested(state: &mut State, fx: &mut Vec<Effect>) {
    let UpdateStatus::Available {
        asset: Some(asset), ..
    } = &state.update
    else {
        return;
    };
    let startable = matches!(state.install, Install::Idle | Install::Failed { .. });
    if !startable || state.install_target != InstallTarget::Ready {
        return;
    }
    let asset = asset.clone();
    state.close_at = None;
    let op = state.op(Op::Update);
    state.install = Install::Working {
        stage: UpdateStage::Downloading,
        fraction: 0.0,
    };
    state.say_until_replaced(Msg::UpdateDownloading { percent: 0 });
    fx.push(Effect::PrepareUpdate { op, asset });
}

fn zip_tick(state: &mut State, now: Millis, fx: &mut Vec<Effect>) {
    let Some(job) = &mut state.zip else {
        return;
    };
    if now >= job.deadline {
        let op = job.op;
        state.zip = None;
        state.finish(op, &[Op::Download]);
        state.say(Msg::DownloadZipTimeout, now, ERROR_NOTICE_TIME);
    } else if !job.in_flight && now >= job.next_poll_at {
        job.in_flight = true;
        fx.push(Effect::PollZip {
            op: job.op,
            zip_id: job.zip_id,
        });
    }
}

fn zip_polled(
    state: &mut State,
    now: Millis,
    op: OpId,
    result: Result<ZipStatus, ApiError>,
    fx: &mut Vec<Effect>,
) {
    let Some(job) = state.zip.as_mut().filter(|job| job.op == op) else {
        return;
    };
    job.in_flight = false;
    match result {
        Ok(status) => match (status.zip_status, status.url.clone()) {
            (ZipState::Done, Some(url)) => {
                state.zip = None;
                open_download(state, now, op, &url, status.missing_count(), fx);
            }
            (ZipState::Done | ZipState::Error, _) => {
                state.zip = None;
                state.finish(op, &[Op::Download]);
                state.say(Msg::DownloadZipFailed, now, ERROR_NOTICE_TIME);
            }
            // Still being built (or a state we do not know yet): ask again shortly.
            _ => job.next_poll_at = now.saturating_add(ZIP_POLL_INTERVAL),
        },
        Err(error) if error.is_transient() => {
            job.next_poll_at = now.saturating_add(ZIP_POLL_INTERVAL);
        }
        Err(error) => {
            state.zip = None;
            download_failed(state, now, op, &error, fx);
        }
    }
}

/// Finish the download op and hand the address to the browser, if it is a put.io download address.
fn open_download(
    state: &mut State,
    now: Millis,
    op: OpId,
    url: &str,
    missing: usize,
    fx: &mut Vec<Effect>,
) {
    state.finish(op, &[Op::Download]);
    if download::is_download_url(url) {
        fx.push(Effect::OpenDownload(url.to_owned()));
        // "Started" is only true once the system has taken the address: see `Event::DownloadHandedOver`.
        state.opening = Some(if missing > 0 {
            Msg::DownloadStartedPartial { missing }
        } else {
            Msg::DownloadStarted
        });
    } else {
        state.say(Msg::DownloadBadUrl, now, ERROR_NOTICE_TIME);
    }
}

fn download_failed(
    state: &mut State,
    now: Millis,
    op: OpId,
    error: &ApiError,
    fx: &mut Vec<Effect>,
) {
    if state.finish(op, &[Op::Download]) {
        if matches!(error, ApiError::Unauthorized) {
            session_expired(state, fx);
        } else {
            state.say(Msg::DownloadFailed, now, ERROR_NOTICE_TIME);
        }
    }
}

fn link_polled(
    state: &mut State,
    now: Millis,
    result: Result<Option<Token>, ApiError>,
    fx: &mut Vec<Effect>,
) {
    let Auth::Linking(link) = &mut state.auth else {
        return;
    };
    link.in_flight = false;
    match result {
        Ok(Some(token)) => {
            fx.push(Effect::StoreToken(token));
            // Polling starts right away.
            become_signed_in(state, now, Msg::Authenticated, fx);
        }
        Ok(None) => link.next_poll_at = now + LINK_POLL_INTERVAL,
        // A temporary problem: keep trying until the code's deadline.
        Err(error) if error.is_transient() => link.next_poll_at = now + LINK_POLL_INTERVAL,
        // The server no longer accepts this code.
        Err(_) => {
            state.auth = Auth::SignedOut(Some(SignedOutReason::CodeExpired));
            state.base = Msg::LinkExpired;
        }
    }
}

fn intake(state: &mut State, item: Intake, fx: &mut Vec<Effect>) {
    if !state.is_signed_in() {
        if state.pending_intake.len() < MAX_QUEUED_INTAKE {
            state.pending_intake.push_back(item);
        }
        return;
    }
    state.close_at = None;
    let parent = state.settings.default_folder_id();
    let name = item.display_name();
    match item {
        Intake::Magnet(magnet) => {
            let op = state.op(Op::AddMagnet);
            state.say_until_replaced(Msg::Adding { name });
            fx.push(Effect::AddMagnet {
                op,
                uri: magnet.uri().to_owned(),
                parent,
            });
        }
        Intake::TorrentFile(path) => {
            let op = state.op(Op::UploadTorrent);
            state.say_until_replaced(Msg::Uploading { name });
            fx.push(Effect::UploadTorrent {
                op,
                path,
                parent,
                trash_after: state.settings.trash_torrent_after_upload,
            });
        }
    }
}

fn add_finished(
    state: &mut State,
    now: Millis,
    op: OpId,
    result: Result<Option<i64>, ApiError>,
    fx: &mut Vec<Effect>,
) {
    let kind = state.ops.get(&op).copied();
    if kind == Some(Op::AddLink) {
        links::finished(state, now, op, result, fx);
        return;
    }
    if !matches!(kind, Some(Op::AddMagnet | Op::UploadTorrent)) {
        return;
    }
    state.finish(op, &[Op::AddMagnet, Op::UploadTorrent]);
    let is_torrent = kind == Some(Op::UploadTorrent);
    match result {
        Ok(transfer_id) => {
            if let Some(id) = transfer_id {
                state.tracker.note_added(id);
            }
            let close = if is_torrent {
                state.settings.close_after_torrent
            } else {
                state.settings.close_after_magnet
            };
            match (is_torrent, close) {
                (false, false) => state.say(Msg::Added, now, NOTICE_TIME),
                (false, true) => state.say_until_replaced(Msg::AddedClosing),
                (true, false) => state.say(Msg::Uploaded, now, NOTICE_TIME),
                (true, true) => state.say_until_replaced(Msg::UploadedClosing),
            }
            if close {
                state.close_at = Some(now + CLOSE_DELAY);
            }
            state.schedule_poll(now);
        }
        Err(ApiError::Unauthorized) => session_expired(state, fx),
        Err(error) => {
            // Something went wrong: stay open so the user can read why.
            state.close_at = None;
            let reason = state.reason(&error);
            let msg = if is_torrent {
                Msg::UploadFailed { reason }
            } else {
                Msg::AddFailed { reason }
            };
            state.say(msg, now, ERROR_NOTICE_TIME);
        }
    }
}

fn polled(
    state: &mut State,
    now: Millis,
    account: Result<Account, ApiError>,
    transfers: Result<Vec<Transfer>, ApiError>,
    fx: &mut Vec<Effect>,
) {
    state.poll_in_flight = false;
    if !state.is_signed_in() {
        return; // signed out while the request was in flight
    }
    if matches!(account, Err(ApiError::Unauthorized))
        || matches!(transfers, Err(ApiError::Unauthorized))
    {
        session_expired(state, fx);
        return;
    }

    // A failed account call keeps what was loaded before; with nothing loaded, the status line says so (below).
    if let Ok(account) = account {
        state.account = Some(account);
    }

    match transfers {
        Ok(mut list) => {
            state.failures = 0;
            state.schedule_poll(now + POLL_INTERVAL);
            for done in state.tracker.observe(&list) {
                fx.push(Effect::Notify(Notification {
                    title: state.text(&Msg::NotificationTitle),
                    body: state.text(&Msg::NotificationBody { name: done.name }),
                    file_id: done.file_id,
                }));
            }
            transfers::sort(&mut list, transfers::SortKey::Default, true);
            state.summary = transfers::summarize(&list);
            state.transfers = list;
            // No account after this poll means loading it failed: say so rather than hide it.
            state.base = if state.account.is_none() {
                Msg::AccountFailed
            } else {
                Msg::Summary {
                    pending: state.summary.pending,
                    finished: state.summary.finished,
                    failed: state.summary.failed,
                }
            };
            let badge = state.summary.average_percent;
            if badge != state.badge {
                state.badge = badge;
                fx.push(Effect::SetBadge(badge));
            }
        }
        Err(error) => {
            let wait = state.backoff(&error);
            state.failures = state.failures.saturating_add(1);
            state.schedule_poll(now + wait);
            state.base = match &error {
                ApiError::Network(_) => Msg::Offline,
                ApiError::RateLimited { retry_after } => Msg::RateLimited {
                    seconds: retry_after.as_secs(),
                },
                ApiError::Api(failure) if failure.status >= 500 => Msg::ServerError,
                _ => Msg::TransfersFailed,
            };
        }
    }
}

fn maybe_poll(state: &mut State, now: Millis, fx: &mut Vec<Effect>) {
    feeds::maybe_load(state, now, fx);
    files::maybe_load(state, now, fx);
    detail::maybe_load(state, now, fx);
    shares::maybe_load(state, now, fx);
    extract::maybe_poll(state, now, fx);
    if state.is_signed_in()
        && !state.poll_in_flight
        && state.next_poll_at.is_some_and(|at| now >= at)
    {
        state.poll_in_flight = true;
        state.next_poll_at = None;
        fx.push(Effect::Poll);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use putio_api::{ApiFailure, TransferStatus};

    use super::*;
    use crate::magnet::Magnet;

    const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

    fn magnet(name: &str) -> Intake {
        Intake::Magnet(Magnet::parse(&format!("magnet:?xt=urn:btih:{HASH}&dn={name}")).unwrap())
    }

    fn transfer(
        id: i64,
        name: &str,
        status: TransferStatus,
        percent: i64,
        file_id: Option<i64>,
    ) -> Transfer {
        let mut t: Transfer = serde_json::from_str(&format!(
            r#"{{"id":{id},"name":"{name}","percent_done":{percent}}}"#
        ))
        .unwrap();
        t.status = status;
        t.file_id = file_id;
        t
    }

    fn account() -> Account {
        serde_json::from_str(r#"{"user_id":1,"username":"nico","mail":"n@example.com"}"#).unwrap()
    }

    fn token() -> Token {
        Token::new("tok").unwrap()
    }

    fn api_error(status: u16, message: &str) -> ApiError {
        ApiError::Api(ApiFailure {
            status,
            error_type: Some("X".into()),
            message: message.into(),
        })
    }

    /// A signed-in state that has not polled yet.
    fn signed_in(settings: Settings) -> (State, Vec<Effect>) {
        let mut state = State::new(settings, Locale::En, "0.1.0");
        let fx = update(&mut state, 0, Event::Started { has_token: true });
        (state, fx)
    }

    fn no_updates() -> Settings {
        Settings {
            check_for_updates: false,
            ..Settings::default()
        }
    }

    fn poll_ok(state: &mut State, now: Millis, list: Vec<Transfer>) -> Vec<Effect> {
        update(
            state,
            now,
            Event::Polled {
                account: Ok(account()),
                transfers: Ok(list),
            },
        )
    }

    fn op_of(fx: &[Effect]) -> OpId {
        fx.iter()
            .find_map(|e| match e {
                Effect::AddMagnet { op, .. }
                | Effect::UploadTorrent { op, .. }
                | Effect::CancelTransfer { op, .. }
                | Effect::RetryTransfer { op, .. }
                | Effect::CleanTransfers { op, .. }
                | Effect::PrepareDownload { op, .. }
                | Effect::PrepareUpdate { op, .. }
                | Effect::Stream { op, .. } => Some(*op),
                _ => None,
            })
            .expect("an effect with an op id")
    }

    // ---- start-up & sign-in -------------------------------------------------------------

    #[test]
    fn start_with_token_polls_immediately_and_checks_for_updates() {
        let (state, fx) = signed_in(Settings::default());
        assert_eq!(fx, [Effect::FetchLatestRelease, Effect::Poll]);
        assert!(state.is_signed_in());
        assert_eq!(state.update, UpdateStatus::Checking);
    }

    #[test]
    fn start_without_token_requests_a_device_code_and_skips_updates_when_disabled() {
        let mut state = State::new(no_updates(), Locale::En, "0.1.0");
        let fx = update(&mut state, 0, Event::Started { has_token: false });
        assert_eq!(fx, [Effect::RequestDeviceCode]);
        assert_eq!(state.auth, Auth::RequestingCode);
        assert_eq!(*state.status(0), Msg::AuthRequired);
    }

    fn linking_state() -> State {
        let mut state = State::new(no_updates(), Locale::En, "0.1.0");
        update(&mut state, 0, Event::Started { has_token: false });
        update(
            &mut state,
            0,
            Event::DeviceCodeReceived(Ok(DeviceCode {
                code: "ABCD1234".into(),
            })),
        );
        state
    }

    #[test]
    fn linking_polls_every_three_seconds_and_never_overlaps() {
        let mut state = linking_state();
        assert!(matches!(&state.auth, Auth::Linking(l) if l.code == "ABCD1234"));
        assert!(update(&mut state, 2_999, Event::Tick).is_empty());
        assert_eq!(
            update(&mut state, 3_000, Event::Tick),
            [Effect::PollDeviceCode {
                code: "ABCD1234".into()
            }]
        );
        assert!(
            update(&mut state, 9_000, Event::Tick).is_empty(),
            "one poll in flight at a time"
        );
        update(&mut state, 9_000, Event::LinkPolled(Ok(None)));
        assert!(update(&mut state, 11_999, Event::Tick).is_empty());
        assert_eq!(update(&mut state, 12_000, Event::Tick).len(), 1);
    }

    #[test]
    fn linking_success_stores_the_token_and_starts_polling_at_once() {
        let mut state = linking_state();
        update(&mut state, 3_000, Event::Tick);
        let fx = update(&mut state, 3_100, Event::LinkPolled(Ok(Some(token()))));
        assert_eq!(fx.len(), 2);
        assert!(matches!(&fx[0], Effect::StoreToken(t) if t.expose() == "tok"));
        assert_eq!(
            fx[1],
            Effect::Poll,
            "polling must start right after sign-in"
        );
        assert!(state.is_signed_in());
        assert_eq!(*state.status(3_100), Msg::Authenticated);
    }

    #[test]
    fn linking_survives_transient_errors_but_not_server_rejection() {
        let mut state = linking_state();
        update(&mut state, 3_000, Event::Tick);
        update(
            &mut state,
            3_000,
            Event::LinkPolled(Err(ApiError::Network("dns".into()))),
        );
        assert!(matches!(state.auth, Auth::Linking(_)));
        update(&mut state, 6_000, Event::Tick);
        update(
            &mut state,
            6_000,
            Event::LinkPolled(Err(api_error(400, "bad code"))),
        );
        assert_eq!(
            state.auth,
            Auth::SignedOut(Some(SignedOutReason::CodeExpired))
        );
        assert_eq!(*state.status(6_000), Msg::LinkExpired);
    }

    #[test]
    fn linking_times_out_and_can_be_retried() {
        let mut state = linking_state();
        assert!(update(&mut state, LINK_TTL, Event::Tick).is_empty());
        assert_eq!(
            state.auth,
            Auth::SignedOut(Some(SignedOutReason::CodeExpired))
        );
        assert_eq!(
            update(&mut state, LINK_TTL, Event::SignInRequested),
            [Effect::RequestDeviceCode]
        );
        assert_eq!(state.auth, Auth::RequestingCode);
    }

    #[test]
    fn a_failed_code_request_is_reported() {
        let mut state = State::new(no_updates(), Locale::En, "0.1.0");
        update(&mut state, 0, Event::Started { has_token: false });
        update(
            &mut state,
            0,
            Event::DeviceCodeReceived(Err(ApiError::Network("x".into()))),
        );
        assert_eq!(
            state.auth,
            Auth::SignedOut(Some(SignedOutReason::CodeRequestFailed))
        );
        assert!(matches!(state.status(1), Msg::SignInFailed { .. }));
    }

    #[test]
    fn stray_link_results_are_ignored_when_not_linking() {
        let (mut state, _) = signed_in(no_updates());
        assert!(
            update(&mut state, 5, Event::LinkPolled(Ok(Some(token()))))
                .iter()
                .all(|e| *e != Effect::StoreToken(token()))
        );
        assert!(state.is_signed_in());
        update(
            &mut state,
            5,
            Event::DeviceCodeReceived(Ok(DeviceCode { code: "X".into() })),
        );
        assert!(state.is_signed_in());
    }

    #[test]
    fn open_link_page_opens_the_fixed_url() {
        let (mut state, _) = signed_in(no_updates());
        assert_eq!(
            update(&mut state, 0, Event::OpenLinkPage),
            [Effect::OpenUrl("https://put.io/link".into())]
        );
    }

    // ---- polling ---------------------------------------------------------------------------

    #[test]
    fn polls_every_twenty_seconds_without_overlap() {
        let (mut state, fx) = signed_in(no_updates());
        assert_eq!(fx, [Effect::Poll]);
        assert!(
            update(&mut state, 100, Event::Tick).is_empty(),
            "poll in flight"
        );
        poll_ok(&mut state, 500, vec![]);
        assert!(update(&mut state, 20_499, Event::Tick).is_empty());
        assert_eq!(update(&mut state, 20_500, Event::Tick), [Effect::Poll]);
    }

    #[test]
    fn poll_updates_state_summary_badge_and_status_line() {
        let (mut state, _) = signed_in(no_updates());
        let fx = poll_ok(
            &mut state,
            10,
            vec![
                transfer(1, "a", TransferStatus::Downloading, 40, None),
                transfer(2, "b", TransferStatus::Completed, 100, Some(9)),
            ],
        );
        assert_eq!(fx, [Effect::SetBadge(Some(40))]);
        assert_eq!(state.account.as_ref().unwrap().username, "nico");
        assert_eq!(state.status_text(10), "1 pending transfer  ·  1 finished");
        assert_eq!(
            state.transfers.first().unwrap().id,
            1,
            "running transfers sort first"
        );
        // Unchanged badge is not re-sent; clearing is.
        assert!(
            poll_ok(
                &mut state,
                20_100,
                vec![transfer(1, "a", TransferStatus::Downloading, 40, None)]
            )
            .is_empty()
        );
        let fx = poll_ok(&mut state, 40_200, vec![]);
        assert_eq!(fx, [Effect::SetBadge(None)]);
    }

    #[test]
    fn completion_notification_for_known_transfers_only() {
        let (mut state, _) = signed_in(no_updates());
        let fx = poll_ok(
            &mut state,
            0,
            vec![
                transfer(1, "Sintel", TransferStatus::Downloading, 90, None),
                transfer(2, "Old", TransferStatus::Completed, 100, Some(2)),
            ],
        );
        assert!(
            !fx.iter().any(|e| matches!(e, Effect::Notify(_))),
            "first poll is silent"
        );
        let fx = poll_ok(
            &mut state,
            20_000,
            vec![
                transfer(1, "Sintel", TransferStatus::Completed, 100, Some(77)),
                transfer(2, "Old", TransferStatus::Completed, 100, Some(2)),
            ],
        );
        let notes: Vec<_> = fx
            .iter()
            .filter_map(|e| {
                if let Effect::Notify(n) = e {
                    Some(n)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].title, "Download finished");
        assert_eq!(notes[0].body, "“Sintel” finished downloading");
        assert_eq!(notes[0].file_id, Some(77));
    }

    #[test]
    fn notifications_are_localised() {
        let mut state = State::new(no_updates(), Locale::De, "0.1.0");
        update(&mut state, 0, Event::Started { has_token: true });
        poll_ok(
            &mut state,
            0,
            vec![transfer(1, "X", TransferStatus::Downloading, 1, None)],
        );
        let fx = poll_ok(
            &mut state,
            20_000,
            vec![transfer(1, "X", TransferStatus::Completed, 100, None)],
        );
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::Notify(n) if n.title == "Download abgeschlossen"))
        );
    }

    #[test]
    fn transfer_poll_failures_back_off_exponentially_and_recover() {
        let (mut state, _) = signed_in(no_updates());
        let fail = |state: &mut State, now| {
            update(
                state,
                now,
                Event::Polled {
                    account: Ok(account()),
                    transfers: Err(ApiError::Network("x".into())),
                },
            )
        };
        fail(&mut state, 0);
        assert_eq!(*state.status(0), Msg::Offline);
        assert!(update(&mut state, 19_999, Event::Tick).is_empty());
        assert_eq!(update(&mut state, 20_000, Event::Tick), [Effect::Poll]);
        fail(&mut state, 20_000);
        assert!(
            update(&mut state, 59_999, Event::Tick).is_empty(),
            "second failure waits 40 s"
        );
        assert_eq!(update(&mut state, 60_000, Event::Tick), [Effect::Poll]);
        poll_ok(&mut state, 60_000, vec![]);
        assert!(update(&mut state, 79_999, Event::Tick).is_empty());
        assert_eq!(
            update(&mut state, 80_000, Event::Tick),
            [Effect::Poll],
            "recovered to the normal interval"
        );
    }

    #[test]
    fn backoff_is_capped() {
        let (mut state, _) = signed_in(no_updates());
        let mut now = 0;
        for _ in 0..12 {
            update(
                &mut state,
                now,
                Event::Polled {
                    account: Ok(account()),
                    transfers: Err(api_error(503, "down")),
                },
            );
            now += MAX_BACKOFF;
            assert_eq!(update(&mut state, now, Event::Tick), [Effect::Poll]);
        }
        assert_eq!(*state.status(0), Msg::ServerError);
    }

    #[test]
    fn rate_limiting_waits_for_the_server_supplied_time() {
        let (mut state, _) = signed_in(no_updates());
        let err = ApiError::RateLimited {
            retry_after: Duration::from_secs(90),
        };
        update(
            &mut state,
            0,
            Event::Polled {
                account: Err(err.clone()),
                transfers: Err(err),
            },
        );
        assert_eq!(*state.status(0), Msg::RateLimited { seconds: 90 });
        assert!(update(&mut state, 89_999, Event::Tick).is_empty());
        assert_eq!(update(&mut state, 90_000, Event::Tick), [Effect::Poll]);
    }

    #[test]
    fn unauthorized_poll_signs_out_and_deletes_the_token() {
        let (mut state, _) = signed_in(no_updates());
        poll_ok(
            &mut state,
            0,
            vec![transfer(1, "a", TransferStatus::Downloading, 50, None)],
        );
        let fx = update(
            &mut state,
            100,
            Event::Polled {
                account: Err(ApiError::Unauthorized),
                transfers: Err(ApiError::Unauthorized),
            },
        );
        assert!(fx.contains(&Effect::DeleteToken));
        assert!(fx.contains(&Effect::SetBadge(None)));
        assert_eq!(
            state.auth,
            Auth::SignedOut(Some(SignedOutReason::SessionExpired))
        );
        assert_eq!(*state.status(100), Msg::AuthExpired);
        assert!(state.transfers.is_empty() && state.account.is_none());
        assert!(
            update(&mut state, 999_999, Event::Tick).is_empty(),
            "no more polling while signed out"
        );
    }

    #[test]
    fn a_poll_result_arriving_after_sign_out_is_dropped() {
        let (mut state, _) = signed_in(no_updates());
        update(&mut state, 5, Event::SignOutRequested);
        assert!(
            poll_ok(
                &mut state,
                10,
                vec![transfer(1, "a", TransferStatus::Downloading, 5, None)]
            )
            .is_empty()
        );
        assert!(state.transfers.is_empty());
    }

    #[test]
    fn account_failure_alone_keeps_transfers_working() {
        let (mut state, _) = signed_in(no_updates());
        update(
            &mut state,
            0,
            Event::Polled {
                account: Err(api_error(500, "x")),
                transfers: Ok(vec![transfer(1, "a", TransferStatus::Waiting, 0, None)]),
            },
        );
        assert_eq!(state.transfers.len(), 1);
        assert!(state.account.is_none());
        // ...and the status line says why the account is missing, until the next poll brings it.
        assert_eq!(state.status(0), &Msg::AccountFailed);
        poll_ok(
            &mut state,
            POLL_INTERVAL,
            vec![transfer(1, "a", TransferStatus::Waiting, 0, None)],
        );
        assert!(state.account.is_some());
        assert!(matches!(state.status(POLL_INTERVAL), Msg::Summary { .. }));
    }

    #[test]
    fn sign_out_clears_everything() {
        let (mut state, _) = signed_in(no_updates());
        poll_ok(
            &mut state,
            0,
            vec![transfer(1, "a", TransferStatus::Downloading, 50, None)],
        );
        let fx = update(&mut state, 5, Event::SignOutRequested);
        assert!(fx.contains(&Effect::DeleteToken));
        assert_eq!(state.auth, Auth::SignedOut(None));
        assert!(state.transfers.is_empty());
        assert_eq!(state.summary, Summary::default());
    }

    #[test]
    fn work_in_flight_for_a_signed_out_account_cannot_sign_out_the_next_session() {
        let (mut state, _) = signed_in(no_updates());
        let op = op_of(&update(&mut state, 1, Event::Intake(magnet("A"))));
        assert!(state.is_busy());
        update(&mut state, 2, Event::SignOutRequested);
        assert!(!state.is_busy(), "nothing is running for nobody");

        // Another account signs in.
        update(&mut state, 3, Event::SignInRequested);
        update(
            &mut state,
            4,
            Event::DeviceCodeReceived(Ok(DeviceCode { code: "X".into() })),
        );
        update(&mut state, 9_000, Event::Tick);
        update(&mut state, 9_100, Event::LinkPolled(Ok(Some(token()))));
        assert!(state.is_signed_in());

        // The first account's add comes back late, rejected.
        let fx = update(
            &mut state,
            9_200,
            Event::AddFinished {
                op,
                result: Err(ApiError::Unauthorized),
            },
        );
        assert!(!fx.contains(&Effect::DeleteToken));
        assert!(state.is_signed_in());
    }

    #[test]
    fn signing_out_keeps_an_update_that_is_being_downloaded() {
        let (mut state, _) = signed_in(no_updates());
        let op = state.op(Op::Update);
        update(&mut state, 2, Event::SignOutRequested);
        assert!(state.is_busy());
        assert!(state.finish(op, &[Op::Update]));
        assert!(!state.is_busy());
    }

    // ---- adding things ---------------------------------------------------------------------

    #[test]
    fn adding_a_magnet_uses_the_default_folder_and_shows_progress() {
        let settings = Settings {
            default_folder: Some(Folder {
                id: 42,
                name: "Movies".into(),
            }),
            ..no_updates()
        };
        let (mut state, _) = signed_in(settings);
        poll_ok(&mut state, 0, vec![]);
        let fx = update(&mut state, 1_000, Event::Intake(magnet("Sintel")));
        assert!(
            matches!(&fx[..], [Effect::AddMagnet { parent: Some(42), uri, .. }] if uri.contains(HASH))
        );
        assert!(state.is_busy());
        assert_eq!(state.status_text(9_999_999), "Adding: Sintel");
    }

    #[test]
    fn successful_add_refreshes_at_once_and_message_expires_into_the_summary() {
        let (mut state, _) = signed_in(no_updates());
        poll_ok(&mut state, 0, vec![]);
        let op = op_of(&update(&mut state, 1_000, Event::Intake(magnet("X"))));
        let fx = update(
            &mut state,
            1_500,
            Event::AddFinished {
                op,
                result: Ok(Some(5)),
            },
        );
        assert_eq!(fx, [Effect::Poll], "refresh immediately after adding");
        assert!(!state.is_busy());
        assert_eq!(*state.status(1_500), Msg::Added);
        assert!(
            matches!(state.status(1_500 + NOTICE_TIME), Msg::Summary { .. }),
            "notice expires"
        );
    }

    #[test]
    fn auto_close_quits_after_five_seconds_for_magnets() {
        let settings = Settings {
            close_after_magnet: true,
            ..no_updates()
        };
        let (mut state, _) = signed_in(settings);
        poll_ok(&mut state, 0, vec![]);
        let op = op_of(&update(&mut state, 1_000, Event::Intake(magnet("X"))));
        update(
            &mut state,
            2_000,
            Event::AddFinished {
                op,
                result: Ok(None),
            },
        );
        assert_eq!(*state.status(60_000), Msg::AddedClosing);
        assert!(state.closing_soon());
        assert!(!update(&mut state, 6_999, Event::Tick).contains(&Effect::Quit));
        assert!(update(&mut state, 7_000, Event::Tick).contains(&Effect::Quit));
    }

    #[test]
    fn a_new_item_cancels_the_pending_auto_close() {
        let settings = Settings {
            close_after_magnet: true,
            ..no_updates()
        };
        let (mut state, _) = signed_in(settings);
        poll_ok(&mut state, 0, vec![]);
        let op = op_of(&update(&mut state, 1_000, Event::Intake(magnet("A"))));
        update(
            &mut state,
            2_000,
            Event::AddFinished {
                op,
                result: Ok(None),
            },
        );
        let op2 = op_of(&update(&mut state, 4_000, Event::Intake(magnet("B"))));
        assert!(!state.closing_soon());
        assert!(!update(&mut state, 8_000, Event::Tick).contains(&Effect::Quit));
        update(
            &mut state,
            8_100,
            Event::AddFinished {
                op: op2,
                result: Ok(None),
            },
        );
        assert!(update(&mut state, 13_100, Event::Tick).contains(&Effect::Quit));
    }

    #[test]
    fn auto_close_for_torrents_uses_its_own_setting() {
        let settings = Settings {
            close_after_magnet: true,
            close_after_torrent: false,
            ..no_updates()
        };
        let (mut state, _) = signed_in(settings);
        poll_ok(&mut state, 0, vec![]);
        let fx = update(
            &mut state,
            0,
            Event::Intake(Intake::TorrentFile("/tmp/a.torrent".into())),
        );
        assert!(matches!(
            &fx[..],
            [Effect::UploadTorrent { parent: None, .. }]
        ));
        assert_eq!(state.status_text(0), "Uploading torrent: a.torrent");
        update(
            &mut state,
            100,
            Event::AddFinished {
                op: op_of(&fx),
                result: Ok(Some(1)),
            },
        );
        assert!(!state.closing_soon());
        assert_eq!(*state.status(100), Msg::Uploaded);
    }

    #[test]
    fn items_received_before_sign_in_are_added_after_it() {
        let mut state = linking_state();
        assert!(update(&mut state, 100, Event::Intake(magnet("Queued"))).is_empty());
        update(&mut state, 3_000, Event::Tick);
        let fx = update(&mut state, 3_100, Event::LinkPolled(Ok(Some(token()))));
        assert!(fx.iter().any(|e| matches!(e, Effect::AddMagnet { .. })));
        assert!(fx.iter().any(|e| matches!(e, Effect::StoreToken(_))));
    }

    #[test]
    fn the_pre_sign_in_queue_is_bounded() {
        let mut state = linking_state();
        for _ in 0..(MAX_QUEUED_INTAKE + 10) {
            update(&mut state, 1, Event::Intake(magnet("Q")));
        }
        update(&mut state, 3_000, Event::Tick);
        let fx = update(&mut state, 3_100, Event::LinkPolled(Ok(Some(token()))));
        assert_eq!(
            fx.iter()
                .filter(|e| matches!(e, Effect::AddMagnet { .. }))
                .count(),
            MAX_QUEUED_INTAKE
        );
    }

    #[test]
    fn add_failures_say_why() {
        let (mut state, _) = signed_in(no_updates());
        poll_ok(&mut state, 0, vec![]);
        let cases = [
            (
                ApiError::Network("x".into()),
                "Couldn't add the transfer: Can't reach put.io. Check your connection.",
            ),
            (
                api_error(400, "Transfer already exists"),
                "Couldn't add the transfer: Transfer already exists",
            ),
            (
                api_error(502, "<html>"),
                "Couldn't add the transfer: put.io is having problems. Trying again soon.",
            ),
        ];
        for (error, expected) in cases {
            let op = op_of(&update(&mut state, 100, Event::Intake(magnet("X"))));
            update(
                &mut state,
                200,
                Event::AddFinished {
                    op,
                    result: Err(error),
                },
            );
            assert_eq!(state.status_text(200), expected);
            assert!(!state.is_busy());
        }
    }

    #[test]
    fn server_messages_are_sanitised_and_bounded() {
        let (mut state, _) = signed_in(no_updates());
        poll_ok(&mut state, 0, vec![]);
        let op = op_of(&update(&mut state, 1, Event::Intake(magnet("X"))));
        let nasty = format!("bad\u{1b}[31m{}", "x".repeat(1000));
        update(
            &mut state,
            2,
            Event::AddFinished {
                op,
                result: Err(api_error(400, &nasty)),
            },
        );
        let text = state.status_text(2);
        assert!(!text.contains('\u{1b}'));
        assert!(text.chars().count() < MAX_REASON_CHARS + 40);
    }

    #[test]
    fn unauthorized_while_adding_signs_out() {
        let (mut state, _) = signed_in(no_updates());
        poll_ok(&mut state, 0, vec![]);
        let op = op_of(&update(&mut state, 1, Event::Intake(magnet("X"))));
        let fx = update(
            &mut state,
            2,
            Event::AddFinished {
                op,
                result: Err(ApiError::Unauthorized),
            },
        );
        assert!(fx.contains(&Effect::DeleteToken));
        assert_eq!(*state.status(2), Msg::AuthExpired);
    }

    #[test]
    fn unknown_and_duplicate_op_results_are_ignored() {
        let (mut state, _) = signed_in(no_updates());
        poll_ok(&mut state, 0, vec![]);
        update(
            &mut state,
            1,
            Event::AddFinished {
                op: OpId(999),
                result: Ok(None),
            },
        );
        assert!(!state.is_busy());
        let op = op_of(&update(&mut state, 1, Event::Intake(magnet("X"))));
        update(
            &mut state,
            2,
            Event::AddFinished {
                op,
                result: Ok(None),
            },
        );
        update(
            &mut state,
            3,
            Event::AddFinished {
                op,
                result: Ok(None),
            },
        );
        assert!(
            !state.is_busy(),
            "busy counter must not underflow or double count"
        );
        // A cancel result must not complete an add op.
        let op = op_of(&update(&mut state, 4, Event::Intake(magnet("Y"))));
        update(&mut state, 5, Event::CancelFinished { op, result: Ok(()) });
        assert!(state.is_busy());
    }

    #[test]
    fn rejected_input_is_reported_without_effects() {
        let (mut state, _) = signed_in(no_updates());
        let fx = update(
            &mut state,
            0,
            Event::IntakeRejected(crate::intake::IntakeError::Magnet(
                crate::magnet::MagnetError::NoInfoHash,
            )),
        );
        assert!(fx.is_empty());
        assert_eq!(*state.status(0), Msg::NotAMagnet);
        update(
            &mut state,
            0,
            Event::IntakeRejected(crate::intake::IntakeError::NotTorrent),
        );
        assert!(matches!(state.status(0), Msg::NotATorrent { .. }));
    }

    #[test]
    fn an_unusable_torrent_file_ends_the_operation() {
        let (mut state, _) = signed_in(no_updates());
        poll_ok(&mut state, 0, vec![]);
        let fx = update(
            &mut state,
            0,
            Event::Intake(Intake::TorrentFile("/tmp/x.torrent".into())),
        );
        update(
            &mut state,
            1,
            Event::TorrentUnusable {
                op: op_of(&fx),
                reason: "the file is empty".into(),
            },
        );
        assert!(!state.is_busy());
        assert_eq!(
            state.status_text(1),
            "Not a usable torrent file: the file is empty"
        );
    }

    // ---- cancel, stream, open ----------------------------------------------------------------

    #[test]
    fn cancel_targets_the_transfer_id_not_a_row() {
        let (mut state, _) = signed_in(no_updates());
        poll_ok(
            &mut state,
            0,
            vec![transfer(7, "a", TransferStatus::Downloading, 1, None)],
        );
        let fx = update(&mut state, 1, Event::CancelRequested(7));
        assert!(matches!(&fx[..], [Effect::CancelTransfer { id: 7, .. }]));
        let op = op_of(&fx);
        let fx = update(&mut state, 2, Event::CancelFinished { op, result: Ok(()) });
        assert_eq!(fx, [Effect::Poll]);
        assert_eq!(*state.status(2), Msg::Canceled);
        let op = op_of(&update(&mut state, 3, Event::CancelRequested(7)));
        update(
            &mut state,
            4,
            Event::CancelFinished {
                op,
                result: Err(api_error(404, "gone")),
            },
        );
        assert_eq!(*state.status(4), Msg::CancelFailed);
    }

    #[test]
    fn retry_targets_a_known_transfer_and_refreshes_on_success() {
        let (mut state, _) = signed_in(no_updates());
        poll_ok(
            &mut state,
            0,
            vec![transfer(7, "a", TransferStatus::Error, 0, None)],
        );
        assert!(update(&mut state, 1, Event::RetryRequested(404)).is_empty());
        let fx = update(&mut state, 1, Event::RetryRequested(7));
        assert!(matches!(&fx[..], [Effect::RetryTransfer { id: 7, .. }]));
        assert_eq!(*state.status(1), Msg::Retrying);
        assert!(state.is_busy());
        let op = op_of(&fx);
        let fx = update(&mut state, 2, Event::RetryFinished { op, result: Ok(()) });
        assert_eq!(fx, [Effect::Poll]);
        assert_eq!(*state.status(2), Msg::Retried);
        assert!(!state.is_busy());
        let op = op_of(&update(&mut state, 3, Event::RetryRequested(7)));
        update(
            &mut state,
            4,
            Event::RetryFinished {
                op,
                result: Err(api_error(400, "no")),
            },
        );
        assert_eq!(*state.status(4), Msg::RetryFailed);
        let op = op_of(&update(&mut state, 5, Event::RetryRequested(7)));
        let fx = update(
            &mut state,
            6,
            Event::RetryFinished {
                op,
                result: Err(ApiError::Unauthorized),
            },
        );
        assert!(fx.contains(&Effect::DeleteToken));
    }

    #[test]
    fn clearing_sends_only_the_finished_transfers() {
        let (mut state, _) = signed_in(no_updates());
        poll_ok(
            &mut state,
            0,
            vec![
                transfer(1, "done", TransferStatus::Completed, 100, Some(5)),
                transfer(2, "seeding", TransferStatus::Seeding, 100, Some(6)),
                transfer(3, "running", TransferStatus::Downloading, 10, None),
                transfer(4, "done too", TransferStatus::Completed, 100, Some(8)),
            ],
        );
        let fx = update(&mut state, 1, Event::ClearRequested);
        let [Effect::CleanTransfers { ids, .. }] = &fx[..] else {
            panic!("expected one clean effect, got {fx:?}");
        };
        let mut ids = ids.clone();
        ids.sort_unstable();
        assert_eq!(ids, [1, 4]);
        assert_eq!(*state.status(1), Msg::Clearing);
        let op = op_of(&fx);
        let fx = update(&mut state, 2, Event::ClearFinished { op, result: Ok(()) });
        assert_eq!(fx, [Effect::Poll]);
        assert_eq!(*state.status(2), Msg::Cleared);
        let op = op_of(&update(&mut state, 3, Event::ClearRequested));
        update(
            &mut state,
            4,
            Event::ClearFinished {
                op,
                result: Err(api_error(500, "x")),
            },
        );
        assert_eq!(*state.status(4), Msg::ClearFailed);
    }

    #[test]
    fn clearing_with_nothing_finished_does_nothing() {
        let (mut state, _) = signed_in(no_updates());
        poll_ok(
            &mut state,
            0,
            vec![transfer(
                3,
                "running",
                TransferStatus::Downloading,
                10,
                None,
            )],
        );
        assert!(update(&mut state, 1, Event::ClearRequested).is_empty());
        assert!(!state.is_busy());
    }

    #[test]
    fn copying_offers_only_validated_text() {
        let (mut state, _) = signed_in(no_updates());
        let mut with_source = transfer(1, "a", TransferStatus::Completed, 100, Some(55));
        with_source.source = Some("magnet:?xt=urn:btih:abc".into());
        let mut unsafe_source = transfer(2, "b", TransferStatus::Completed, 100, None);
        unsafe_source.source = Some("magnet:?x\u{1b}[31m".into());
        poll_ok(&mut state, 0, vec![with_source, unsafe_source]);

        assert_eq!(
            update(&mut state, 1, Event::CopyLinkRequested(1)),
            [Effect::CopyText("https://put.io/files/55".into())]
        );
        assert_eq!(*state.status(1), Msg::Copied);
        assert_eq!(
            update(&mut state, 2, Event::CopySourceRequested(1)),
            [Effect::CopyText("magnet:?xt=urn:btih:abc".into())]
        );
        // No file yet, unsafe source, unknown id: nothing is copied.
        assert!(update(&mut state, 3, Event::CopyLinkRequested(2)).is_empty());
        assert!(update(&mut state, 3, Event::CopySourceRequested(2)).is_empty());
        assert!(update(&mut state, 3, Event::CopySourceRequested(404)).is_empty());
        assert!(update(&mut state, 3, Event::CopyLinkRequested(404)).is_empty());
    }

    // ---- downloads ----------------------------------------------------------------------------

    fn zip_status(state: ZipState, url: Option<&str>, missing: usize) -> ZipStatus {
        let missing_files =
            (missing > 0).then(|| vec![putio_api::models::MissingFile::default(); missing]);
        ZipStatus {
            zip_status: state,
            url: url.map(str::to_owned),
            size: None,
            missing_files,
        }
    }

    /// A signed-in state with one finished transfer (id 1, file 55) and one still running (id 2).
    fn with_downloadable() -> State {
        let (mut state, _) = signed_in(no_updates());
        poll_ok(
            &mut state,
            0,
            vec![
                transfer(1, "done", TransferStatus::Completed, 100, Some(55)),
                transfer(2, "running", TransferStatus::Downloading, 5, None),
            ],
        );
        state
    }

    #[test]
    fn a_file_is_handed_to_the_browser_once_its_address_is_known() {
        let mut state = with_downloadable();
        let fx = update(&mut state, 1, Event::DownloadRequested(1));
        assert!(matches!(
            &fx[..],
            [Effect::PrepareDownload { file_id: 55, .. }]
        ));
        assert_eq!(*state.status(1), Msg::DownloadPreparing);
        assert!(state.is_busy());
        let op = op_of(&fx);
        let url = "https://s1.put.io/v2/files/55/download?token=abc";
        let fx = update(
            &mut state,
            2,
            Event::DownloadPrepared {
                op,
                result: Ok(Prepared::Url(url.into())),
            },
        );
        assert_eq!(fx, [Effect::OpenDownload(url.into())]);
        assert_ne!(
            *state.status(2),
            Msg::DownloadStarted,
            "not before the browser has taken it"
        );
        assert!(!state.is_busy());
        update(&mut state, 3, Event::DownloadHandedOver { opened: true });
        assert_eq!(*state.status(3), Msg::DownloadStarted);
    }

    #[test]
    fn a_browser_that_cannot_be_opened_is_reported_instead_of_a_started_download() {
        let (mut state, _) = signed_in(no_updates());
        poll_ok(
            &mut state,
            0,
            vec![transfer(1, "a", TransferStatus::Completed, 100, Some(55))],
        );
        let op = op_of(&update(&mut state, 1, Event::DownloadRequested(1)));
        update(
            &mut state,
            2,
            Event::DownloadPrepared {
                op,
                result: Ok(Prepared::Url(
                    "https://s1.put.io/v2/files/55/download?t=a".into(),
                )),
            },
        );
        update(&mut state, 3, Event::DownloadHandedOver { opened: false });
        assert_eq!(*state.status(3), Msg::DownloadOpenFailed);
        // A late or repeated answer changes nothing.
        update(&mut state, 4, Event::DownloadHandedOver { opened: true });
        assert_eq!(*state.status(4), Msg::DownloadOpenFailed);
    }

    #[test]
    fn a_page_that_cannot_be_opened_and_a_keychain_that_fails_are_told_to_the_user() {
        let (mut state, _) = signed_in(no_updates());
        update(&mut state, 1, Event::PageOpenFailed);
        assert_eq!(*state.status(1), Msg::OpenFailed);
        for (problem, msg) in [
            (SecretProblem::Unavailable, Msg::SecretStoreUnavailable),
            (SecretProblem::Load, Msg::SecretLoadFailed),
            (SecretProblem::Save, Msg::SecretSaveFailed),
            (SecretProblem::Delete, Msg::SecretDeleteFailed),
        ] {
            update(&mut state, 2, Event::SecretStoreFailed(problem));
            assert_eq!(*state.status(2), msg);
        }
    }

    #[test]
    fn preferences_that_cannot_be_read_or_written_are_told_and_the_message_stays() {
        let (mut state, _) = signed_in(no_updates());
        for (problem, msg) in [
            (SettingsProblem::Unreadable, Msg::SettingsUnreadable),
            (SettingsProblem::NotSaved, Msg::SettingsNotSaved),
        ] {
            update(&mut state, 2, Event::SettingsFailed(problem));
            assert_eq!(*state.status(2), msg);
            assert_eq!(*state.status(10_000_000), msg);
        }
    }

    #[test]
    fn asking_for_updates_twice_sends_one_request_and_keeps_the_user_in_the_loop() {
        let mut state = State::new(Settings::default(), Locale::En, "0.1.0");
        let first = update(
            &mut state,
            0,
            Event::CheckForUpdates {
                user_initiated: false,
            },
        );
        assert_eq!(first, [Effect::FetchLatestRelease]);
        // The user asks while the background check is still out.
        let second = update(
            &mut state,
            1,
            Event::CheckForUpdates {
                user_initiated: true,
            },
        );
        assert!(second.is_empty());
        update(
            &mut state,
            2,
            Event::ReleaseFetched(Err(ApiError::Network("x".into()))),
        );
        assert_eq!(
            *state.status(2),
            Msg::UpdateCheckFailed,
            "their answer is not swallowed"
        );
        // Once answered, a new request can go out.
        let third = update(
            &mut state,
            3,
            Event::CheckForUpdates {
                user_initiated: true,
            },
        );
        assert_eq!(third, [Effect::FetchLatestRelease]);
    }

    #[test]
    fn an_address_that_is_not_put_io_is_never_opened() {
        let mut state = with_downloadable();
        for url in [
            "https://evil.example/x",
            "http://put.io/x",
            "https://put.io@evil.example/",
            "file:///etc/passwd",
        ] {
            let op = op_of(&update(&mut state, 1, Event::DownloadRequested(1)));
            let fx = update(
                &mut state,
                2,
                Event::DownloadPrepared {
                    op,
                    result: Ok(Prepared::Url(url.into())),
                },
            );
            assert!(fx.is_empty(), "{url}");
            assert_eq!(*state.status(2), Msg::DownloadBadUrl);
            assert!(!state.is_busy());
        }
    }

    #[test]
    fn nothing_is_downloaded_for_a_transfer_without_a_file_or_an_unknown_one() {
        let mut state = with_downloadable();
        assert!(update(&mut state, 1, Event::DownloadRequested(2)).is_empty());
        assert_eq!(*state.status(1), Msg::DownloadNotReady);
        assert!(update(&mut state, 1, Event::DownloadRequested(404)).is_empty());
        assert!(!state.is_busy());
        let (mut signed_out, _) = signed_in(no_updates());
        update(&mut signed_out, 0, Event::SignOutRequested);
        assert!(update(&mut signed_out, 1, Event::DownloadRequested(1)).is_empty());
    }

    #[test]
    fn a_folder_is_zipped_polled_and_then_handed_to_the_browser() {
        let mut state = with_downloadable();
        let op = op_of(&update(&mut state, 1_000, Event::DownloadRequested(1)));
        assert!(
            update(
                &mut state,
                1_000,
                Event::DownloadPrepared {
                    op,
                    result: Ok(Prepared::Zip(9))
                }
            )
            .is_empty()
        );
        assert!(state.is_busy());
        // The first poll is due straight away, then one at a time.
        assert_eq!(
            update(&mut state, 1_000, Event::Tick),
            [Effect::PollZip { op, zip_id: 9 }]
        );
        assert!(
            update(&mut state, 1_500, Event::Tick).is_empty(),
            "one poll in flight"
        );
        let building = zip_status(ZipState::Processing, None, 0);
        assert!(
            update(
                &mut state,
                1_600,
                Event::ZipPolled {
                    op,
                    result: Ok(building)
                }
            )
            .is_empty()
        );
        assert!(update(&mut state, 3_599, Event::Tick).is_empty());
        assert_eq!(
            update(&mut state, 3_600, Event::Tick),
            [Effect::PollZip { op, zip_id: 9 }]
        );
        let done = zip_status(ZipState::Done, Some("https://s2.put.io/zip/9?sig=x"), 0);
        let fx = update(
            &mut state,
            3_700,
            Event::ZipPolled {
                op,
                result: Ok(done),
            },
        );
        assert_eq!(
            fx,
            [Effect::OpenDownload("https://s2.put.io/zip/9?sig=x".into())]
        );
        update(
            &mut state,
            3_700,
            Event::DownloadHandedOver { opened: true },
        );
        assert_eq!(*state.status(3_700), Msg::DownloadStarted);
        assert!(!state.is_busy());
        assert!(
            !update(&mut state, 99_000, Event::Tick)
                .iter()
                .any(|e| matches!(e, Effect::PollZip { .. })),
            "no more polling"
        );
    }

    #[test]
    fn files_put_io_left_out_of_a_zip_are_reported() {
        let mut state = with_downloadable();
        let op = op_of(&update(&mut state, 0, Event::DownloadRequested(1)));
        update(
            &mut state,
            0,
            Event::DownloadPrepared {
                op,
                result: Ok(Prepared::Zip(9)),
            },
        );
        update(&mut state, 0, Event::Tick);
        let done = zip_status(ZipState::Done, Some("https://s2.put.io/z"), 3);
        update(
            &mut state,
            1,
            Event::ZipPolled {
                op,
                result: Ok(done),
            },
        );
        update(&mut state, 1, Event::DownloadHandedOver { opened: true });
        assert_eq!(*state.status(1), Msg::DownloadStartedPartial { missing: 3 });
    }

    #[test]
    fn a_failed_or_address_less_zip_ends_the_download_with_a_message() {
        for status in [
            zip_status(ZipState::Error, None, 0),
            zip_status(ZipState::Done, None, 0),
        ] {
            let mut state = with_downloadable();
            let op = op_of(&update(&mut state, 0, Event::DownloadRequested(1)));
            update(
                &mut state,
                0,
                Event::DownloadPrepared {
                    op,
                    result: Ok(Prepared::Zip(9)),
                },
            );
            update(&mut state, 0, Event::Tick);
            let fx = update(
                &mut state,
                1,
                Event::ZipPolled {
                    op,
                    result: Ok(status),
                },
            );
            assert!(fx.is_empty());
            assert_eq!(*state.status(1), Msg::DownloadZipFailed);
            assert!(!state.is_busy());
        }
    }

    #[test]
    fn a_zip_that_takes_too_long_is_given_up_on() {
        let mut state = with_downloadable();
        let op = op_of(&update(&mut state, 0, Event::DownloadRequested(1)));
        update(
            &mut state,
            0,
            Event::DownloadPrepared {
                op,
                result: Ok(Prepared::Zip(9)),
            },
        );
        assert!(
            update(&mut state, ZIP_TTL - 1, Event::Tick)
                .contains(&Effect::PollZip { op, zip_id: 9 })
        );
        let fx = update(&mut state, ZIP_TTL, Event::Tick);
        assert!(fx.is_empty());
        assert_eq!(*state.status(ZIP_TTL), Msg::DownloadZipTimeout);
        assert!(!state.is_busy());
    }

    #[test]
    fn a_temporary_network_problem_does_not_end_a_zip_wait() {
        let mut state = with_downloadable();
        let op = op_of(&update(&mut state, 0, Event::DownloadRequested(1)));
        update(
            &mut state,
            0,
            Event::DownloadPrepared {
                op,
                result: Ok(Prepared::Zip(9)),
            },
        );
        update(&mut state, 0, Event::Tick);
        update(
            &mut state,
            1,
            Event::ZipPolled {
                op,
                result: Err(ApiError::Network("down".into())),
            },
        );
        assert!(state.is_busy());
        assert_eq!(
            update(&mut state, 2_001, Event::Tick),
            [Effect::PollZip { op, zip_id: 9 }]
        );
        update(
            &mut state,
            2_002,
            Event::ZipPolled {
                op,
                result: Err(api_error(404, "gone")),
            },
        );
        assert!(!state.is_busy());
        assert_eq!(*state.status(2_002), Msg::DownloadFailed);
    }

    #[test]
    fn download_errors_are_reported_and_an_expired_session_signs_out() {
        let mut state = with_downloadable();
        let op = op_of(&update(&mut state, 0, Event::DownloadRequested(1)));
        update(
            &mut state,
            1,
            Event::DownloadPrepared {
                op,
                result: Err(api_error(500, "x")),
            },
        );
        assert_eq!(*state.status(1), Msg::DownloadFailed);
        assert!(!state.is_busy());
        let op = op_of(&update(&mut state, 2, Event::DownloadRequested(1)));
        let fx = update(
            &mut state,
            3,
            Event::DownloadPrepared {
                op,
                result: Err(ApiError::Unauthorized),
            },
        );
        assert!(fx.contains(&Effect::DeleteToken));
        // And during a zip wait.
        let mut state = with_downloadable();
        let op = op_of(&update(&mut state, 0, Event::DownloadRequested(1)));
        update(
            &mut state,
            0,
            Event::DownloadPrepared {
                op,
                result: Ok(Prepared::Zip(9)),
            },
        );
        update(&mut state, 0, Event::Tick);
        let fx = update(
            &mut state,
            1,
            Event::ZipPolled {
                op,
                result: Err(ApiError::Unauthorized),
            },
        );
        assert!(fx.contains(&Effect::DeleteToken));
        assert!(!state.is_busy());
    }

    #[test]
    fn signing_out_drops_a_zip_wait_and_ignores_its_late_answers() {
        let mut state = with_downloadable();
        let op = op_of(&update(&mut state, 0, Event::DownloadRequested(1)));
        update(
            &mut state,
            0,
            Event::DownloadPrepared {
                op,
                result: Ok(Prepared::Zip(9)),
            },
        );
        update(&mut state, 0, Event::Tick);
        update(&mut state, 1, Event::SignOutRequested);
        assert!(!state.is_busy());
        let done = zip_status(ZipState::Done, Some("https://s2.put.io/z"), 0);
        assert!(
            update(
                &mut state,
                2,
                Event::ZipPolled {
                    op,
                    result: Ok(done)
                }
            )
            .is_empty()
        );
        assert!(
            !update(&mut state, 99_000, Event::Tick)
                .iter()
                .any(|e| matches!(e, Effect::PollZip { .. }))
        );
    }

    #[test]
    fn only_one_folder_is_prepared_at_a_time_and_stale_answers_are_ignored() {
        let mut state = with_downloadable();
        let op = op_of(&update(&mut state, 0, Event::DownloadRequested(1)));
        update(
            &mut state,
            0,
            Event::DownloadPrepared {
                op,
                result: Ok(Prepared::Zip(9)),
            },
        );
        assert!(update(&mut state, 1, Event::DownloadRequested(1)).is_empty());
        assert_eq!(*state.status(1), Msg::DownloadBusy);
        // A poll answer for some other op changes nothing.
        let done = zip_status(ZipState::Done, Some("https://s2.put.io/z"), 0);
        assert!(
            update(
                &mut state,
                2,
                Event::ZipPolled {
                    op: OpId(999),
                    result: Ok(done)
                }
            )
            .is_empty()
        );
        assert!(state.is_busy());
        // Nor does a duplicate "prepared" for an op that is no longer a download.
        assert!(
            update(
                &mut state,
                3,
                Event::DownloadPrepared {
                    op: OpId(999),
                    result: Ok(Prepared::Url("https://put.io/x".into()))
                }
            )
            .is_empty()
        );
    }

    #[test]
    fn stream_needs_a_file_and_reports_player_problems() {
        let (mut state, _) = signed_in(no_updates());
        poll_ok(
            &mut state,
            0,
            vec![
                transfer(1, "running", TransferStatus::Downloading, 5, None),
                transfer(2, "done", TransferStatus::Completed, 100, Some(55)),
            ],
        );
        assert!(update(&mut state, 1, Event::StreamRequested(1)).is_empty());
        assert_eq!(*state.status(1), Msg::StreamNotReady);
        assert!(update(&mut state, 1, Event::StreamRequested(404)).is_empty());
        let fx = update(&mut state, 2, Event::StreamRequested(2));
        assert!(matches!(&fx[..], [Effect::Stream { file_id: 55, .. }]));
        let op = op_of(&fx);
        update(
            &mut state,
            3,
            Event::StreamFinished {
                op,
                result: Err(StreamFailure::NoPlayer),
            },
        );
        assert_eq!(*state.status(3), Msg::StreamNoPlayer);
        assert!(!state.is_busy());
    }

    #[test]
    fn opening_on_put_io_builds_only_fixed_urls_from_numeric_ids() {
        let (mut state, _) = signed_in(no_updates());
        poll_ok(
            &mut state,
            0,
            vec![
                transfer(1, "a", TransferStatus::Completed, 100, Some(55)),
                transfer(2, "b", TransferStatus::Downloading, 1, None),
            ],
        );
        assert_eq!(
            update(&mut state, 0, Event::OpenTransferOnPutio(1)),
            [Effect::OpenUrl("https://put.io/files/55".into())]
        );
        assert_eq!(
            update(&mut state, 0, Event::OpenTransferOnPutio(2)),
            [Effect::OpenUrl("https://put.io/transfers".into())]
        );
        assert_eq!(
            update(&mut state, 0, Event::OpenTransferOnPutio(999)),
            [Effect::OpenUrl("https://put.io/transfers".into())]
        );
        assert_eq!(
            update(
                &mut state,
                0,
                Event::NotificationClicked { file_id: Some(8) }
            ),
            [Effect::OpenUrl("https://put.io/files/8".into())]
        );
        assert_eq!(
            update(
                &mut state,
                0,
                Event::NotificationClicked { file_id: Some(-1) }
            ),
            [Effect::OpenUrl("https://put.io/transfers".into())]
        );
        assert_eq!(
            update(&mut state, 0, Event::OpenProjectPage),
            [Effect::OpenUrl(PROJECT_URL.into())]
        );
        assert_eq!(
            update(&mut state, 0, Event::OpenSupportPage),
            [Effect::OpenUrl("https://buymeacoffee.com/nicoswd".into())]
        );
    }

    // ---- settings, window, updates --------------------------------------------------------------

    #[test]
    fn a_window_size_is_clamped_and_saved() {
        let (mut state, _) = signed_in(no_updates());
        let fx = update(
            &mut state,
            0,
            Event::SettingsChanged(SettingsChange::WindowSize(WindowSize {
                width: 9,
                height: 99_999,
            })),
        );
        let size = state.settings.window_size.unwrap();
        assert_eq!(
            (size.width, size.height),
            (WindowSize::MIN_WIDTH, WindowSize::MAX_HEIGHT)
        );
        assert!(
            matches!(&fx[..], [Effect::SaveSettings(saved)] if saved.window_size == Some(size))
        );
    }

    // ---- default app for magnet links and .torrent files ------------------------------------

    const NOT_DEFAULT: handler::Status = handler::Status {
        magnet: false,
        torrent: false,
    };
    const IS_DEFAULT: handler::Status = handler::Status {
        magnet: true,
        torrent: true,
    };

    fn checked(state: &mut State, check: handler::Check) {
        update(state, 1, Event::DefaultHandlerChecked(check));
    }

    #[test]
    fn the_shell_asks_the_system_when_told_to() {
        let (mut state, _) = signed_in(no_updates());
        assert_eq!(
            update(&mut state, 0, Event::DefaultHandlerCheckRequested),
            [Effect::CheckDefaultHandler]
        );
    }

    #[test]
    fn the_question_appears_once_when_the_app_is_not_the_default() {
        let (mut state, _) = signed_in(Settings::default());
        checked(&mut state, handler::Check::Known(NOT_DEFAULT));
        assert!(state.handler_prompt);
        // Answered "not now"; a later check in the same run does not ask again.
        update(
            &mut state,
            2,
            Event::DefaultHandlerPromptDismissed { forever: false },
        );
        assert!(!state.handler_prompt);
        assert!(state.settings.ask_default_handler, "not now is not forever");
        checked(&mut state, handler::Check::Known(NOT_DEFAULT));
        assert!(!state.handler_prompt);
    }

    #[test]
    fn nothing_is_asked_when_the_app_is_the_default_or_cannot_be() {
        for check in [
            handler::Check::Known(IS_DEFAULT),
            handler::Check::Unavailable("a development build".into()),
        ] {
            let (mut state, _) = signed_in(Settings::default());
            checked(&mut state, check.clone());
            assert!(!state.handler_prompt, "{check:?}");
            assert_eq!(state.handler_check, Some(check));
        }
    }

    #[test]
    fn dont_ask_again_is_remembered_and_stops_the_question_for_good() {
        let (mut state, _) = signed_in(Settings::default());
        checked(&mut state, handler::Check::Known(NOT_DEFAULT));
        let fx = update(
            &mut state,
            2,
            Event::DefaultHandlerPromptDismissed { forever: true },
        );
        assert!(!state.settings.ask_default_handler);
        assert!(matches!(&fx[..], [Effect::SaveSettings(saved)] if !saved.ask_default_handler));
        // Even a new run (a fresh state with the saved setting) does not ask.
        let (mut next, _) = signed_in(state.settings.clone());
        checked(&mut next, handler::Check::Known(NOT_DEFAULT));
        assert!(!next.handler_prompt);
    }

    #[test]
    fn saying_yes_registers_once_and_reports_the_result() {
        let (mut state, _) = signed_in(Settings::default());
        checked(&mut state, handler::Check::Known(NOT_DEFAULT));
        let fx = update(&mut state, 2, Event::DefaultHandlerRequested);
        assert_eq!(fx, [Effect::RegisterDefaultHandler]);
        assert!(state.handler_busy && !state.handler_prompt);
        assert_eq!(*state.status(2), Msg::HandlerRegistering);
        // A second click while it is working does nothing.
        assert!(update(&mut state, 3, Event::DefaultHandlerRequested).is_empty());
        update(
            &mut state,
            4,
            Event::DefaultHandlerRegistered(Ok(IS_DEFAULT)),
        );
        assert!(!state.handler_busy);
        assert_eq!(state.handler_check, Some(handler::Check::Known(IS_DEFAULT)));
        assert_eq!(*state.status(4), Msg::HandlerSet);
    }

    #[test]
    fn a_failed_or_partial_registration_says_so_and_changes_nothing_else() {
        let (mut state, _) = signed_in(Settings::default());
        checked(&mut state, handler::Check::Known(NOT_DEFAULT));
        update(&mut state, 2, Event::DefaultHandlerRequested);
        update(
            &mut state,
            3,
            Event::DefaultHandlerRegistered(Err("xdg-mime\nfailed\u{1b}[31m".into())),
        );
        assert!(!state.handler_busy);
        assert_eq!(
            *state.status(3),
            Msg::HandlerFailed {
                reason: "xdg-mimefailed[31m".into()
            }
        );
        assert_eq!(
            state.handler_check,
            Some(handler::Check::Known(NOT_DEFAULT)),
            "still not the default"
        );
        // The system took only one of the two.
        update(&mut state, 4, Event::DefaultHandlerRequested);
        let half = handler::Status {
            magnet: true,
            torrent: false,
        };
        update(&mut state, 5, Event::DefaultHandlerRegistered(Ok(half)));
        assert!(matches!(state.status(5), Msg::HandlerFailed { .. }));
        assert_eq!(state.handler_check, Some(handler::Check::Known(half)));
    }

    #[test]
    fn registering_needs_a_system_that_was_looked_at_and_can_do_it() {
        let (mut state, _) = signed_in(Settings::default());
        assert!(
            update(&mut state, 1, Event::DefaultHandlerRequested).is_empty(),
            "never checked"
        );
        checked(
            &mut state,
            handler::Check::Unavailable("a development build".into()),
        );
        assert!(
            update(&mut state, 2, Event::DefaultHandlerRequested).is_empty(),
            "cannot"
        );
        assert!(!state.handler_busy);
    }

    #[test]
    fn settings_changes_are_saved() {
        let (mut state, _) = signed_in(no_updates());
        let fx = update(
            &mut state,
            0,
            Event::SettingsChanged(SettingsChange::CloseAfterMagnet(true)),
        );
        assert!(matches!(&fx[..], [Effect::SaveSettings(s)] if s.close_after_magnet));
        let folder = Folder {
            id: 3,
            name: "TV".into(),
        };
        update(
            &mut state,
            0,
            Event::SettingsChanged(SettingsChange::DefaultFolder(Some(folder.clone()))),
        );
        assert_eq!(state.settings.default_folder, Some(folder));
    }

    #[test]
    fn the_trash_setting_is_applied_saved_and_handed_to_each_upload() {
        let (mut state, _) = signed_in(no_updates());
        poll_ok(&mut state, 0, vec![]);
        let upload = |state: &mut State| {
            update(
                state,
                10,
                Event::Intake(Intake::TorrentFile("/tmp/a.torrent".into())),
            )
        };
        // Off by default: the file is left where it is.
        assert!(matches!(
            &upload(&mut state)[..],
            [Effect::UploadTorrent {
                trash_after: false,
                ..
            }]
        ));
        let fx = update(
            &mut state,
            20,
            Event::SettingsChanged(SettingsChange::TrashTorrentAfterUpload(true)),
        );
        assert!(matches!(&fx[..], [Effect::SaveSettings(s)] if s.trash_torrent_after_upload));
        assert!(matches!(
            &upload(&mut state)[..],
            [Effect::UploadTorrent {
                trash_after: true,
                ..
            }]
        ));
        // Magnet links have no file to move.
        let fx = update(&mut state, 30, Event::Intake(magnet("Show")));
        assert!(matches!(&fx[..], [Effect::AddMagnet { .. }]));
    }

    #[test]
    fn the_theme_choice_is_applied_and_saved() {
        let (mut state, _) = signed_in(no_updates());
        assert_eq!(state.settings.theme, ThemeChoice::System);
        let fx = update(
            &mut state,
            0,
            Event::SettingsChanged(SettingsChange::Theme(ThemeChoice::Dark)),
        );
        assert_eq!(state.settings.theme, ThemeChoice::Dark);
        assert!(matches!(&fx[..], [Effect::SaveSettings(s)] if s.theme == ThemeChoice::Dark));
    }

    #[test]
    fn toggling_transfers_is_remembered() {
        let (mut state, _) = signed_in(no_updates());
        assert!(!state.transfers_expanded);
        let fx = update(&mut state, 0, Event::ToggleTransfers);
        assert!(state.transfers_expanded);
        assert!(matches!(&fx[..], [Effect::SaveSettings(s)] if s.transfers_expanded));
        let restored = State::new(
            Settings {
                transfers_expanded: true,
                ..no_updates()
            },
            Locale::En,
            "0.1.0",
        );
        assert!(restored.transfers_expanded);
    }

    fn release(tag: &str) -> Release {
        Release {
            tag_name: tag.into(),
            html_url: format!("https://github.com/x/y/releases/tag/{tag}"),
            prerelease: false,
            draft: false,
            author: None,
            assets: Vec::new(),
        }
    }

    /// A release the project published, with the one file the test namer expects.
    fn installable_release(tag: &str) -> Release {
        let name = format!("putio-adder-{tag}-test.zip");
        Release {
            author: Some(putio_api::ReleaseAuthor {
                login: "github-actions[bot]".into(),
            }),
            assets: vec![putio_api::ReleaseAsset {
                browser_download_url: format!(
                    "https://github.com/nicoSWD/put.io-adder/releases/download/{tag}/{name}"
                ),
                name,
                size: 5_000_000,
            }],
            ..release(tag)
        }
    }

    #[allow(clippy::unnecessary_wraps)] // the shape the real function has
    fn test_asset_name(tag: &str) -> Option<String> {
        Some(format!("putio-adder-{tag}-test.zip"))
    }

    /// A state running 0.1.0 with v0.2.0 on offer as an installable update, on a copy that can update.
    fn with_installable_update() -> State {
        let (mut state, _) = signed_in(Settings::default());
        state.asset_name = test_asset_name;
        update(&mut state, 0, Event::InstallTargetKnown(Ok(())));
        update(
            &mut state,
            0,
            Event::ReleaseFetched(Ok(installable_release("v0.2.0"))),
        );
        state
    }

    fn start_install(state: &mut State, now: Millis) -> OpId {
        let fx = update(state, now, Event::UpdateInstallRequested);
        assert!(matches!(&fx[..], [Effect::PrepareUpdate { .. }]), "{fx:?}");
        op_of(&fx)
    }

    #[test]
    fn update_check_results() {
        let (mut state, _) = signed_in(Settings::default());
        update(&mut state, 0, Event::ReleaseFetched(Ok(release("v0.2.0"))));
        assert!(
            matches!(&state.update, UpdateStatus::Available { version, .. } if version == "0.2.0")
        );
        assert!(matches!(state.status(0), Msg::UpdateAvailable { .. }));

        update(
            &mut state,
            0,
            Event::CheckForUpdates {
                user_initiated: true,
            },
        );
        assert_eq!(state.update, UpdateStatus::Checking);
        update(
            &mut state,
            100_000,
            Event::ReleaseFetched(Ok(release("v0.1.0"))),
        );
        assert_eq!(state.update, UpdateStatus::UpToDate);
        assert_eq!(*state.status(100_000), Msg::UpToDate);
    }

    #[test]
    fn an_installable_release_offers_its_file_and_asks_for_nothing_yet() {
        let state = with_installable_update();
        assert!(matches!(
            &state.update,
            UpdateStatus::Available { asset: Some(asset), .. } if asset.name == "putio-adder-v0.2.0-test.zip"
        ));
        assert_eq!(state.install, Install::Idle);
        assert!(!state.is_busy());
    }

    #[test]
    fn installing_downloads_the_offered_file_and_shows_progress() {
        let mut state = with_installable_update();
        let fx = update(&mut state, 1, Event::UpdateInstallRequested);
        let [Effect::PrepareUpdate { op, asset }] = &fx[..] else {
            panic!("{fx:?}");
        };
        assert_eq!(asset.size, 5_000_000);
        assert!(state.is_busy());
        assert_eq!(*state.status(1), Msg::UpdateDownloading { percent: 0 });
        let op = *op;
        update(
            &mut state,
            2,
            Event::UpdateProgress {
                op,
                stage: UpdateStage::Downloading,
                fraction: 0.5,
            },
        );
        assert_eq!(*state.status(2), Msg::UpdateDownloading { percent: 50 });
        // The bar never goes backwards, and is clamped.
        update(
            &mut state,
            3,
            Event::UpdateProgress {
                op,
                stage: UpdateStage::Downloading,
                fraction: 0.2,
            },
        );
        assert_eq!(
            state.install,
            Install::Working {
                stage: UpdateStage::Downloading,
                fraction: 0.5
            }
        );
        update(
            &mut state,
            4,
            Event::UpdateProgress {
                op,
                stage: UpdateStage::Unpacking,
                fraction: 7.0,
            },
        );
        assert_eq!(
            state.install,
            Install::Working {
                stage: UpdateStage::Unpacking,
                fraction: 1.0
            }
        );
        assert_eq!(*state.status(4), Msg::UpdateUnpacking);
        // Progress for some other operation changes nothing.
        update(
            &mut state,
            5,
            Event::UpdateProgress {
                op: OpId(999),
                stage: UpdateStage::Downloading,
                fraction: 0.0,
            },
        );
        assert_eq!(
            state.install,
            Install::Working {
                stage: UpdateStage::Unpacking,
                fraction: 1.0
            }
        );
    }

    #[test]
    fn nothing_is_installed_without_a_file_a_target_or_when_already_busy() {
        // No installable file: only a release page.
        let (mut state, _) = signed_in(Settings::default());
        state.asset_name = test_asset_name;
        update(&mut state, 0, Event::InstallTargetKnown(Ok(())));
        update(&mut state, 0, Event::ReleaseFetched(Ok(release("v0.2.0"))));
        assert!(update(&mut state, 1, Event::UpdateInstallRequested).is_empty());
        // A file, but this copy cannot update itself (or has not been looked at).
        for target in [None, Some(Err("development build".to_owned()))] {
            let (mut state, _) = signed_in(Settings::default());
            state.asset_name = test_asset_name;
            if let Some(target) = target {
                update(&mut state, 0, Event::InstallTargetKnown(target));
            }
            update(
                &mut state,
                0,
                Event::ReleaseFetched(Ok(installable_release("v0.2.0"))),
            );
            assert!(update(&mut state, 1, Event::UpdateInstallRequested).is_empty());
            assert_eq!(state.install, Install::Idle);
        }
        // Not twice at once, and not again once it is ready.
        let mut state = with_installable_update();
        let op = start_install(&mut state, 1);
        assert!(update(&mut state, 2, Event::UpdateInstallRequested).is_empty());
        update(&mut state, 3, Event::UpdatePrepared { op, result: Ok(()) });
        assert!(update(&mut state, 4, Event::UpdateInstallRequested).is_empty());
        // And nothing at all when no update is on offer.
        let (mut none, _) = signed_in(Settings::default());
        update(&mut none, 0, Event::InstallTargetKnown(Ok(())));
        assert!(update(&mut none, 1, Event::UpdateInstallRequested).is_empty());
    }

    #[test]
    fn a_prepared_update_waits_for_the_restart_and_a_failed_one_can_be_retried() {
        let mut state = with_installable_update();
        let op = start_install(&mut state, 1);
        update(&mut state, 2, Event::UpdatePrepared { op, result: Ok(()) });
        assert_eq!(
            state.install,
            Install::Ready {
                version: "0.2.0".into()
            }
        );
        assert!(!state.is_busy());
        assert_eq!(
            *state.status(2),
            Msg::UpdateReady {
                version: "0.2.0".into()
            }
        );

        let mut state = with_installable_update();
        let op = start_install(&mut state, 1);
        update(
            &mut state,
            2,
            Event::UpdatePrepared {
                op,
                result: Err("it is not an app\nat all\u{1b}[31m".into()),
            },
        );
        assert_eq!(
            state.install,
            Install::Failed {
                version: "0.2.0".into(),
                message: "it is not an appat all[31m".into()
            }
        );
        assert!(!state.is_busy());
        assert!(matches!(state.status(2), Msg::UpdateFailed { .. }));
        // A second try is allowed.
        start_install(&mut state, 3);
        assert!(matches!(state.install, Install::Working { .. }));
    }

    #[test]
    fn a_late_answer_for_an_unknown_operation_is_ignored() {
        let mut state = with_installable_update();
        start_install(&mut state, 1);
        update(
            &mut state,
            2,
            Event::UpdatePrepared {
                op: OpId(999),
                result: Ok(()),
            },
        );
        assert!(matches!(state.install, Install::Working { .. }));
        assert!(state.is_busy());
    }

    #[test]
    fn restarting_applies_the_update_and_quits_only_when_that_worked() {
        let mut state = with_installable_update();
        // Nothing to restart into yet.
        assert!(update(&mut state, 1, Event::UpdateRestartRequested).is_empty());
        assert!(update(&mut state, 1, Event::UpdateApplied(Ok(()))).is_empty());
        let op = start_install(&mut state, 2);
        update(&mut state, 3, Event::UpdatePrepared { op, result: Ok(()) });
        assert_eq!(
            update(&mut state, 4, Event::UpdateRestartRequested),
            [Effect::ApplyUpdate]
        );
        assert_eq!(*state.status(4), Msg::UpdateRestarting);
        assert_eq!(
            update(&mut state, 5, Event::UpdateApplied(Ok(()))),
            [Effect::Quit]
        );

        let mut state = with_installable_update();
        let op = start_install(&mut state, 1);
        update(&mut state, 2, Event::UpdatePrepared { op, result: Ok(()) });
        update(&mut state, 3, Event::UpdateRestartRequested);
        let fx = update(
            &mut state,
            4,
            Event::UpdateApplied(Err("could not move the app".into())),
        );
        assert!(!fx.contains(&Effect::Quit));
        assert!(
            matches!(&state.install, Install::Failed { message, .. } if message == "could not move the app")
        );
    }

    #[test]
    fn updates_are_rechecked_every_six_hours_and_after_thirty_minutes_when_that_failed() {
        const SIX_HOURS: Millis = 6 * 60 * 60_000;
        let mut state = with_installable_update();
        // Ok at 0: next check in six hours, and the update on offer stays visible meanwhile.
        assert!(
            !update(&mut state, SIX_HOURS - 1, Event::Tick).contains(&Effect::FetchLatestRelease)
        );
        let fx = update(&mut state, SIX_HOURS, Event::Tick);
        assert!(fx.contains(&Effect::FetchLatestRelease));
        assert!(
            matches!(state.update, UpdateStatus::Available { .. }),
            "no flash of 'checking'"
        );
        // One check at a time: nothing more until it answers.
        assert!(
            !update(&mut state, SIX_HOURS + 1, Event::Tick).contains(&Effect::FetchLatestRelease)
        );
        // It failed: try again in 30 minutes, still showing what was known.
        update(
            &mut state,
            SIX_HOURS + 10,
            Event::ReleaseFetched(Err(ApiError::Network("x".into()))),
        );
        assert!(matches!(state.update, UpdateStatus::Available { .. }));
        let retry = SIX_HOURS + 10 + 30 * 60_000;
        assert!(!update(&mut state, retry - 1, Event::Tick).contains(&Effect::FetchLatestRelease));
        assert!(update(&mut state, retry, Event::Tick).contains(&Effect::FetchLatestRelease));
    }

    #[test]
    fn rechecking_does_not_announce_the_same_update_again_but_does_announce_a_newer_one() {
        let mut state = with_installable_update();
        assert!(matches!(state.status(0), Msg::UpdateAvailable { .. }));
        update(
            &mut state,
            100_000,
            Event::ReleaseFetched(Ok(installable_release("v0.2.0"))),
        );
        assert_eq!(*state.status(100_000), Msg::FetchingInfo);
        update(
            &mut state,
            200_000,
            Event::ReleaseFetched(Ok(installable_release("v0.3.0"))),
        );
        assert_eq!(
            *state.status(200_000),
            Msg::UpdateAvailable {
                version: "0.3.0".into()
            }
        );
    }

    #[test]
    fn no_recheck_while_installing_or_waiting_for_the_restart_or_when_checks_are_off() {
        const SIX_HOURS: Millis = 6 * 60 * 60_000;
        let mut state = with_installable_update();
        let op = start_install(&mut state, 1);
        assert!(!update(&mut state, SIX_HOURS, Event::Tick).contains(&Effect::FetchLatestRelease));
        update(
            &mut state,
            SIX_HOURS + 1,
            Event::UpdatePrepared { op, result: Ok(()) },
        );
        assert!(
            !update(&mut state, SIX_HOURS + 2, Event::Tick).contains(&Effect::FetchLatestRelease)
        );

        let mut off = with_installable_update();
        update(
            &mut off,
            1,
            Event::SettingsChanged(SettingsChange::CheckForUpdates(false)),
        );
        assert!(!update(&mut off, SIX_HOURS, Event::Tick).contains(&Effect::FetchLatestRelease));
        assert!(
            !update(&mut off, 2 * SIX_HOURS, Event::Tick).contains(&Effect::FetchLatestRelease)
        );
    }

    #[test]
    fn a_failure_about_an_older_release_is_forgotten_when_a_newer_one_appears() {
        let mut state = with_installable_update();
        let op = start_install(&mut state, 1);
        update(
            &mut state,
            2,
            Event::UpdatePrepared {
                op,
                result: Err("nope".into()),
            },
        );
        assert!(matches!(state.install, Install::Failed { .. }));
        update(
            &mut state,
            3,
            Event::ReleaseFetched(Ok(installable_release("v0.2.0"))),
        );
        assert!(
            matches!(state.install, Install::Failed { .. }),
            "same release: the error stays"
        );
        update(
            &mut state,
            4,
            Event::ReleaseFetched(Ok(installable_release("v0.3.0"))),
        );
        assert_eq!(state.install, Install::Idle);
    }

    #[test]
    fn background_update_checks_are_silent_unless_there_is_news() {
        let (mut state, _) = signed_in(Settings::default());
        update(&mut state, 0, Event::ReleaseFetched(Ok(release("v0.1.0"))));
        assert_eq!(*state.status(0), Msg::FetchingInfo);
        update(
            &mut state,
            0,
            Event::ReleaseFetched(Err(ApiError::Network("x".into()))),
        );
        assert_eq!(*state.status(0), Msg::FetchingInfo);
        assert_eq!(state.update, UpdateStatus::Unknown);
    }

    #[test]
    fn user_initiated_update_failure_is_reported() {
        let (mut state, _) = signed_in(Settings::default());
        update(
            &mut state,
            0,
            Event::CheckForUpdates {
                user_initiated: true,
            },
        );
        update(
            &mut state,
            1,
            Event::ReleaseFetched(Err(ApiError::Network("x".into()))),
        );
        assert_eq!(*state.status(1), Msg::UpdateCheckFailed);
    }

    // ---- regressions found in review -----------------------------------------------------------

    #[test]
    fn a_magnet_received_before_started_is_added_when_a_token_exists() {
        // Cold launch via a magnet link: the OS event arrives before the first `Started`.
        let mut state = State::new(no_updates(), Locale::En, "0.1.0");
        assert!(update(&mut state, 0, Event::Intake(magnet("Cold"))).is_empty());
        let fx = update(&mut state, 5, Event::Started { has_token: true });
        assert!(
            fx.iter().any(|e| matches!(e, Effect::AddMagnet { .. })),
            "{fx:?}"
        );
        assert!(state.is_busy());
    }

    #[test]
    fn a_magnet_received_before_started_survives_a_first_run_sign_in() {
        let mut state = State::new(no_updates(), Locale::En, "0.1.0");
        update(&mut state, 0, Event::Intake(magnet("Cold")));
        update(&mut state, 5, Event::Started { has_token: false });
        update(
            &mut state,
            5,
            Event::DeviceCodeReceived(Ok(DeviceCode {
                code: "ABCD".into(),
            })),
        );
        update(&mut state, 3_005, Event::Tick);
        let fx = update(&mut state, 3_100, Event::LinkPolled(Ok(Some(token()))));
        assert!(fx.iter().any(|e| matches!(e, Effect::AddMagnet { .. })));
    }

    #[test]
    fn auto_close_waits_for_every_running_operation() {
        let settings = Settings {
            close_after_magnet: true,
            ..no_updates()
        };
        let mut state = State::new(settings, Locale::En, "0.1.0");
        update(&mut state, 0, Event::Intake(magnet("A")));
        update(&mut state, 0, Event::Intake(magnet("B")));
        let fx = update(&mut state, 10, Event::Started { has_token: true });
        let ops: Vec<OpId> = fx
            .iter()
            .filter_map(|e| {
                if let Effect::AddMagnet { op, .. } = e {
                    Some(*op)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(ops.len(), 2);

        update(
            &mut state,
            1_000,
            Event::AddFinished {
                op: ops[0],
                result: Ok(None),
            },
        );
        assert!(
            !update(&mut state, 6_000, Event::Tick).contains(&Effect::Quit),
            "second upload is still running"
        );
        assert!(
            !update(&mut state, 60_000, Event::Tick).contains(&Effect::Quit),
            "…however long it takes"
        );

        update(
            &mut state,
            61_000,
            Event::AddFinished {
                op: ops[1],
                result: Ok(None),
            },
        );
        assert!(!update(&mut state, 65_999, Event::Tick).contains(&Effect::Quit));
        assert!(
            update(&mut state, 66_000, Event::Tick).contains(&Effect::Quit),
            "five seconds after the last one"
        );
    }

    #[test]
    fn a_failed_add_cancels_auto_close_so_the_error_stays_visible() {
        let settings = Settings {
            close_after_magnet: true,
            ..no_updates()
        };
        let (mut state, _) = signed_in(settings);
        poll_ok(&mut state, 0, vec![]);
        let a = op_of(&update(&mut state, 100, Event::Intake(magnet("A"))));
        let b = op_of(&update(&mut state, 100, Event::Intake(magnet("B"))));
        update(
            &mut state,
            200,
            Event::AddFinished {
                op: a,
                result: Ok(None),
            },
        );
        update(
            &mut state,
            300,
            Event::AddFinished {
                op: b,
                result: Err(api_error(400, "nope")),
            },
        );
        assert!(!state.closing_soon());
        assert!(!update(&mut state, 99_999, Event::Tick).contains(&Effect::Quit));
    }

    #[test]
    fn interacting_cancels_auto_close() {
        let settings = Settings {
            close_after_magnet: true,
            ..no_updates()
        };
        let (mut state, _) = signed_in(settings);
        poll_ok(
            &mut state,
            0,
            vec![transfer(1, "a", TransferStatus::Completed, 100, Some(5))],
        );
        let op = op_of(&update(&mut state, 100, Event::Intake(magnet("A"))));
        update(
            &mut state,
            200,
            Event::AddFinished {
                op,
                result: Ok(None),
            },
        );
        assert!(state.closing_soon());
        update(&mut state, 300, Event::StreamRequested(1));
        assert!(!state.closing_soon());
    }

    #[test]
    fn the_release_page_comes_from_the_validated_update_status() {
        let (mut state, _) = signed_in(Settings::default());
        assert!(
            update(&mut state, 0, Event::OpenReleasePage).is_empty(),
            "nothing to open before an update is found"
        );
        update(&mut state, 0, Event::ReleaseFetched(Ok(release("v0.2.0"))));
        assert_eq!(
            update(&mut state, 0, Event::OpenReleasePage),
            [Effect::OpenUrl(
                "https://github.com/x/y/releases/tag/v0.2.0".into()
            )]
        );
    }
}
