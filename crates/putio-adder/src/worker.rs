//! The background threads that talk to put.io.
//!
//! The UI never blocks on the network: it sends [`Job`]s here and gets [`WorkerMsg`]s back, then is
//! woken with `request_repaint`. No async runtime, just two small thread pools ("lanes"):
//!
//! * **interactive** (3 threads): sign-in, polling, adding a magnet, cancelling, update checks, the
//!   folder picker. These are quick and must never wait behind a big upload.
//! * **heavy** (2 threads): `.torrent` uploads (up to two minutes on a slow link) and streaming set-up.
//!
//! Everything that talks to the account belongs to one *session*: the stretch between two credential
//! changes (a token stored, a token deleted, a new sign-in attempt). Each such job takes the token and
//! the session number when the UI sends it, never when a thread gets round to it, so work queued for
//! one account cannot run as another. The answer carries the same number, and the UI throws away an
//! answer from a session that has ended ([`Workers::accept`]).

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use putio_api::client::FileListing;
use putio_api::{
    ApiError, Client, FileType, RssFeedSettings, Token, Transport, UploadOutcome, UreqTransport,
};
use putio_core::state::{Event, OpId, Prepared, StreamFailure};
use putio_core::update::UpdateAsset;
use putio_platform::handler;
use putio_platform::player::{PlayError, PlayerLauncher, VlcLauncher};
use putio_platform::torrent_file;
use putio_platform::update;

/// Most videos queued from one folder.
const MAX_PLAYLIST: usize = 50;
const INTERACTIVE_THREADS: usize = 3;
const HEAVY_THREADS: usize = 2;

/// What the app identifies itself as in the put.io "linked apps" list.
const CLIENT_NAME: &str = "put.io adder";
/// The put.io application id used for the device (OOB) sign-in. A public identifier, not a secret.
pub(crate) const APP_ID: u32 = 711;

#[derive(Debug)]
pub(crate) enum Job {
    DeviceCode,
    PollCode(String),
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
    },
    Cancel {
        op: OpId,
        id: i64,
    },
    Retry {
        op: OpId,
        id: i64,
    },
    PrepareDownload {
        op: OpId,
        file_id: i64,
    },
    PollZip {
        op: OpId,
        zip_id: i64,
    },
    Clean {
        op: OpId,
        ids: Vec<i64>,
    },
    Stream {
        op: OpId,
        file_id: i64,
    },
    /// Download and unpack an update (progress is reported while it runs; see [`prepare_update`]).
    PrepareUpdate {
        op: OpId,
        asset: UpdateAsset,
    },
    CheckDefaultHandler,
    RegisterDefaultHandler,
    LatestRelease,
    /// Folders inside `parent`, for the folder picker (UI-local, not part of the core state machine).
    ListFolders {
        parent: i64,
    },
    /// The name of a folder, for the feed form (UI-local too).
    FolderName {
        id: i64,
    },
    LoadFeeds,
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
}

impl Job {
    /// Whether the job acts for the signed-in account (or the sign-in attempt in progress), so that its
    /// answer means nothing once the credentials have changed. Update checks, the default-app calls and
    /// installing an update do not.
    fn is_session_bound(&self) -> bool {
        !matches!(
            self,
            Self::PrepareUpdate { .. }
                | Self::CheckDefaultHandler
                | Self::RegisterDefaultHandler
                | Self::LatestRelease
        )
    }

    fn is_heavy(&self) -> bool {
        matches!(
            self,
            Self::UploadTorrent { .. } | Self::Stream { .. } | Self::PrepareUpdate { .. }
        )
    }
}

#[derive(Debug)]
pub(crate) enum WorkerMsg {
    Core(Event),
    Folders {
        parent: i64,
        result: Result<FileListing, ApiError>,
    },
    /// The answer to `Job::FolderName`: `None` when put.io could not say.
    FolderName {
        id: i64,
        name: Option<String>,
    },
    /// An update is downloaded and unpacked. The unpacked update itself (paths) stays with the app,
    /// not in the core state machine, which is told only whether it worked.
    UpdatePrepared {
        op: OpId,
        result: Result<update::Prepared, String>,
    },
}

/// A job with what it needs from the session it was sent in.
#[derive(Debug)]
struct Queued {
    job: Job,
    /// The session the job was sent in, for a job that is bound to one.
    session: Option<u64>,
    token: Option<Token>,
}

/// What the worker threads send back: the answer, and the session it belongs to (if it has one).
#[derive(Debug)]
pub(crate) struct Delivery {
    session: Option<u64>,
    msg: WorkerMsg,
}

#[derive(Debug, Default)]
struct Session {
    number: u64,
    token: Option<Token>,
}

/// Handle for sending work. Cheap to clone.
#[derive(Debug, Clone)]
pub(crate) struct Workers {
    interactive: Sender<Queued>,
    heavy: Sender<Queued>,
    session: Arc<Mutex<Session>>,
}

impl Workers {
    pub(crate) fn spawn(
        ctx: &eframe::egui::Context,
        out: &Sender<Delivery>,
        user_agent: &str,
    ) -> std::io::Result<Self> {
        let transport = Arc::new(UreqTransport::new(user_agent));
        let lane = |name: &str, threads: usize| -> std::io::Result<Sender<Queued>> {
            let (tx, rx) = mpsc::channel::<Queued>();
            let rx = Arc::new(Mutex::new(rx));
            for index in 0..threads {
                let (rx, out, ctx) = (Arc::clone(&rx), out.clone(), ctx.clone());
                let transport = Arc::clone(&transport);
                thread::Builder::new()
                    .name(format!("putio-{name}-{index}"))
                    .spawn(move || run(&rx, &out, &ctx, &transport))?;
            }
            Ok(tx)
        };
        Ok(Self {
            interactive: lane("interactive", INTERACTIVE_THREADS)?,
            heavy: lane("heavy", HEAVY_THREADS)?,
            session: Arc::new(Mutex::new(Session::default())),
        })
    }

    /// Workers whose jobs go nowhere, for tests of code that only needs to be able to send.
    #[cfg(test)]
    pub(crate) fn detached() -> Self {
        let (interactive, _) = mpsc::channel();
        let (heavy, _) = mpsc::channel();
        Self {
            interactive,
            heavy,
            session: Arc::new(Mutex::new(Session::default())),
        }
    }

    /// The credentials changed: a token was stored or deleted. Ends the current session; what was sent
    /// before is answered to nobody.
    pub(crate) fn set_token(&self, token: Option<Token>) {
        if let Ok(mut session) = self.session.lock() {
            session.number += 1;
            session.token = token;
        }
    }

    /// A new sign-in attempt starts: whatever an earlier attempt still has in flight must not count.
    pub(crate) fn new_session(&self) {
        if let Ok(mut session) = self.session.lock() {
            session.number += 1;
        }
    }

    /// Queue `job` with the token and session of this moment.
    pub(crate) fn send(&self, job: Job) {
        let (session, token) = match self.session.lock() {
            Ok(session) => (session.number, session.token.clone()),
            Err(_) => return,
        };
        let lane = if job.is_heavy() {
            &self.heavy
        } else {
            &self.interactive
        };
        let session = job.is_session_bound().then_some(session);
        let _ = lane.send(Queued {
            job,
            session,
            token,
        });
    }

    /// The answer, unless it comes from a session that has ended.
    pub(crate) fn accept(&self, delivery: Delivery) -> Option<WorkerMsg> {
        let current = self.session.lock().ok()?.number;
        match delivery.session {
            Some(session) if session != current => None,
            _ => Some(delivery.msg),
        }
    }
}

fn run(
    jobs: &Mutex<Receiver<Queued>>,
    out: &Sender<Delivery>,
    ctx: &eframe::egui::Context,
    transport: &Arc<UreqTransport>,
) {
    loop {
        // Hold the queue lock only while waiting for the next job, never while working on it.
        let queued = match jobs.lock() {
            Ok(queue) => queue.recv(),
            Err(_) => return,
        };
        let Ok(Queued {
            job,
            session,
            token,
        }) = queued
        else {
            return;
        };
        let msg = if let Job::PrepareUpdate { op, asset } = job {
            WorkerMsg::UpdatePrepared {
                op,
                result: prepare_update(op, &asset, out, ctx),
            }
        } else {
            let client = match token {
                Some(token) => Client::with_token(Arc::clone(transport), token),
                None => Client::new(Arc::clone(transport)),
            };
            execute(&client, job)
        };
        if out.send(Delivery { session, msg }).is_err() {
            return; // the UI is gone
        }
        ctx.request_repaint();
    }
}

/// Download and unpack the update, telling the UI how far along it is.
fn prepare_update(
    op: OpId,
    asset: &UpdateAsset,
    out: &Sender<Delivery>,
    ctx: &eframe::egui::Context,
) -> Result<update::Prepared, String> {
    let target = update::install_target()?;
    update::prepare(&target, asset, &|stage, fraction| {
        let _ = out.send(Delivery {
            session: None,
            msg: WorkerMsg::Core(Event::UpdateProgress {
                op,
                stage,
                fraction,
            }),
        });
        ctx.request_repaint();
    })
}

fn execute<T: Transport>(client: &Client<T>, job: Job) -> WorkerMsg {
    match job {
        Job::DeviceCode => WorkerMsg::Core(Event::DeviceCodeReceived(
            client.oob_code(APP_ID, CLIENT_NAME),
        )),
        Job::PollCode(code) => WorkerMsg::Core(Event::LinkPolled(client.oob_poll(&code))),
        Job::Poll => {
            let account = client.account();
            let transfers = client.transfers();
            WorkerMsg::Core(Event::Polled { account, transfers })
        }
        Job::AddMagnet { op, uri, parent } => {
            let result = client
                .add_transfer(&uri, parent)
                .map(|transfer| Some(transfer.id));
            WorkerMsg::Core(Event::AddFinished { op, result })
        }
        Job::UploadTorrent { op, path, parent } => match torrent_file::read_torrent(&path) {
            Err(error) => WorkerMsg::Core(Event::TorrentUnusable {
                op,
                reason: error.to_string(),
            }),
            Ok(file) => {
                let result =
                    client
                        .upload_torrent(&file.name, &file.bytes, parent)
                        .map(|outcome| match outcome {
                            UploadOutcome::Transfer(transfer) => Some(transfer.id),
                            UploadOutcome::File(_) => None,
                        });
                WorkerMsg::Core(Event::AddFinished { op, result })
            }
        },
        Job::Cancel { op, id } => WorkerMsg::Core(Event::CancelFinished {
            op,
            result: client.cancel_transfers(&[id]),
        }),
        Job::Retry { op, id } => WorkerMsg::Core(Event::RetryFinished {
            op,
            result: client.retry_transfer(id),
        }),
        Job::PrepareDownload { op, file_id } => WorkerMsg::Core(Event::DownloadPrepared {
            op,
            result: prepare_download(client, file_id),
        }),
        Job::PollZip { op, zip_id } => WorkerMsg::Core(Event::ZipPolled {
            op,
            result: client.zip_status(zip_id),
        }),
        Job::Clean { op, ids } => WorkerMsg::Core(Event::ClearFinished {
            op,
            result: client.clean_transfers(&ids),
        }),
        Job::Stream { op, file_id } => WorkerMsg::Core(Event::StreamFinished {
            op,
            result: stream(client, &VlcLauncher, file_id),
        }),
        // Handled in `run`, which has the channel progress is reported on.
        Job::PrepareUpdate { op, .. } => WorkerMsg::UpdatePrepared {
            op,
            result: Err("internal error: the update job ran without a progress channel".to_owned()),
        },
        Job::CheckDefaultHandler => WorkerMsg::Core(Event::DefaultHandlerChecked(handler::check())),
        Job::RegisterDefaultHandler => {
            WorkerMsg::Core(Event::DefaultHandlerRegistered(handler::register()))
        }
        Job::LatestRelease => WorkerMsg::Core(Event::ReleaseFetched(client.latest_release())),
        Job::LoadFeeds => WorkerMsg::Core(Event::FeedsLoaded(client.rss_feeds())),
        Job::SaveFeed { op, id, settings } => {
            let result = match id {
                Some(id) => client.update_rss_feed(id, &settings),
                None => client.create_rss_feed(&settings),
            };
            WorkerMsg::Core(Event::FeedSaved { op, result })
        }
        Job::SetFeedPaused { op, id, paused } => WorkerMsg::Core(Event::FeedPauseFinished {
            op,
            id,
            paused,
            result: client.set_rss_feed_paused(id, paused),
        }),
        Job::DeleteFeed { op, id } => WorkerMsg::Core(Event::FeedDeleteFinished {
            op,
            id,
            result: client.delete_rss_feed(id),
        }),
        Job::ListFiles { parent } => WorkerMsg::Core(Event::FilesListed {
            parent,
            result: client.list_files(parent, None),
        }),
        Job::SearchFiles { query } => {
            let result = client.search_files(&query);
            WorkerMsg::Core(Event::FilesSearched { query, result })
        }
        Job::RenameFile { op, id, name } => WorkerMsg::Core(Event::FileChangeFinished {
            op,
            result: client.rename_file(id, &name),
        }),
        Job::CreateFolder { op, parent, name } => WorkerMsg::Core(Event::FileChangeFinished {
            op,
            result: client.create_folder(&name, parent),
        }),
        Job::MoveFile { op, id, to } => WorkerMsg::Core(Event::FileChangeFinished {
            op,
            result: client.move_files(&[id], to),
        }),
        Job::DeleteFile { op, id } => WorkerMsg::Core(Event::FileDeleteFinished {
            op,
            id,
            result: client.delete_files(&[id]),
        }),
        Job::FolderName { id } => WorkerMsg::FolderName {
            id,
            name: client.file(id).ok().map(|file| file.name),
        },
        Job::ListFolders { parent } => WorkerMsg::Folders {
            parent,
            result: client.list_files(parent, Some(FileType::Folder)),
        },
    }
}

/// Whether put.io answered "this is not something you can list" (a plain file) rather than failing:
/// a client error other than a rejected login. A dropped connection, a rate limit, a server error or
/// an answer that cannot be read says nothing about what the item is, and must reach the user as the
/// failure it is instead of sending a folder down the single-file path.
fn is_not_a_folder(error: &ApiError) -> bool {
    matches!(error, ApiError::Api(failure) if (400..500).contains(&failure.status) && failure.status != 403)
}

/// Decide how to download `file_id`: a plain file has a signed address of its own, a folder has to be
/// zipped first (put.io builds the ZIP in the background; the reducer polls for it).
fn prepare_download<T: Transport>(client: &Client<T>, file_id: i64) -> Result<Prepared, ApiError> {
    let is_folder = match client.list_files(file_id, None) {
        Ok(listing) => listing
            .parent
            .is_some_and(|parent| parent.file_type == FileType::Folder),
        Err(error) if is_not_a_folder(&error) => false,
        Err(error) => return Err(error),
    };
    if is_folder {
        client.create_zip(&[file_id]).map(Prepared::Zip)
    } else {
        client.file_url(file_id).map(Prepared::Url)
    }
}

/// Resolve playable URLs and hand them to the player. A folder becomes a playlist of its videos in
/// name order; a single file is played directly. Only signed, IP-bound URLs reach the player's command
/// line, never the account token.
fn stream<T: Transport>(
    client: &Client<T>,
    launcher: &dyn PlayerLauncher,
    file_id: i64,
) -> Result<(), StreamFailure> {
    let api = StreamFailure::Api;
    let folder = match client.list_files(file_id, None) {
        Ok(listing) => listing
            .parent
            .filter(|parent| parent.file_type == FileType::Folder)
            .map(|_| listing.files),
        Err(error) if is_not_a_folder(&error) => None,
        Err(error) => return Err(api(error)),
    };
    let ids: Vec<i64> = match folder {
        Some(mut files) => {
            files.retain(|file| file.file_type == FileType::Video);
            files.sort_by_cached_key(|file| file.name.to_lowercase());
            files
                .into_iter()
                .take(MAX_PLAYLIST)
                .map(|file| file.id)
                .collect()
        }
        None => vec![file_id],
    };
    if ids.is_empty() {
        return Err(StreamFailure::NothingToPlay);
    }
    let urls = ids
        .into_iter()
        .map(|id| client.file_url(id))
        .collect::<Result<Vec<_>, _>>()
        .map_err(api)?;
    launcher.play(&urls).map_err(|error| match error {
        PlayError::NoPlayer => StreamFailure::NoPlayer,
        PlayError::NothingToPlay => StreamFailure::NothingToPlay,
        PlayError::BadUrl | PlayError::Spawn(_) => StreamFailure::LaunchFailed,
    })
}

#[cfg(test)]
mod tests {
    use putio_api::testing::FakeTransport;
    use putio_platform::player::PlayError;

    use super::*;

    #[derive(Default)]
    struct FakeLauncher {
        played: Mutex<Vec<Vec<String>>>,
        missing: bool,
    }

    impl PlayerLauncher for FakeLauncher {
        fn play(&self, urls: &[String]) -> Result<(), PlayError> {
            if self.missing {
                return Err(PlayError::NoPlayer);
            }
            self.played.lock().unwrap().push(urls.to_vec());
            Ok(())
        }
    }

    fn client(fake: &FakeTransport) -> Client<&FakeTransport> {
        Client::with_token(fake, Token::new("SECRETTOKEN123").unwrap())
    }

    #[test]
    fn a_single_file_is_played_through_its_signed_url() {
        let fake = FakeTransport::new();
        fake.push_json(
            404,
            r#"{"error_type":"NotFound","error_message":"not a folder"}"#,
        ); // list: not listable
        fake.push_json(200, r#"{"url":"https://signed.invalid/f?sig=1"}"#);
        let launcher = FakeLauncher::default();
        stream(&client(&fake), &launcher, 7).unwrap();
        assert_eq!(
            launcher.played.lock().unwrap().as_slice(),
            [vec!["https://signed.invalid/f?sig=1".to_owned()]]
        );
        // The account token never reaches the player's command line.
        let played = launcher.played.lock().unwrap();
        assert!(
            played
                .iter()
                .flatten()
                .all(|url| !url.contains("SECRETTOKEN123"))
        );
    }

    #[test]
    fn a_plain_file_is_downloaded_through_its_own_signed_address() {
        let fake = FakeTransport::new();
        fake.push_json(
            404,
            r#"{"error_type":"NotFound","error_message":"not a folder"}"#,
        );
        fake.push_json(200, r#"{"url":"https://s1.put.io/f?sig=1"}"#);
        assert_eq!(
            prepare_download(&client(&fake), 7).unwrap(),
            Prepared::Url("https://s1.put.io/f?sig=1".into())
        );
    }

    #[test]
    fn a_listing_that_fails_for_any_other_reason_is_not_taken_for_a_plain_file() {
        let failures: [(u16, &str); 3] = [
            (500, r#"{"error_type":"Internal","error_message":"x"}"#),
            (503, "<html>bad gateway</html>"),
            (403, r#"{"error_type":"Forbidden","error_message":"x"}"#),
        ];
        for (status, body) in failures {
            let fake = FakeTransport::new();
            fake.push_json(status, body);
            assert!(
                prepare_download(&client(&fake), 7).is_err(),
                "{status} must not become a file download"
            );
            let fake = FakeTransport::new();
            fake.push_json(status, body);
            assert!(
                matches!(
                    stream(&client(&fake), &FakeLauncher::default(), 7),
                    Err(StreamFailure::Api(_))
                ),
                "{status} must not start the player"
            );
            assert_eq!(fake.requests().len(), 1, "no second request after {status}");
        }
    }

    #[test]
    fn a_dropped_connection_and_a_rate_limit_are_not_taken_for_a_plain_file() {
        assert!(!is_not_a_folder(&ApiError::Network("reset".into())));
        assert!(!is_not_a_folder(&ApiError::RateLimited {
            retry_after: std::time::Duration::from_secs(5)
        }));
        assert!(!is_not_a_folder(&ApiError::Decode("x".into())));
        assert!(!is_not_a_folder(&ApiError::Unauthorized));
        assert!(!is_not_a_folder(&ApiError::TooLarge));
    }

    #[test]
    fn a_folder_is_downloaded_as_a_zip() {
        let fake = FakeTransport::new();
        fake.push_json(
            200,
            r#"{"parent":{"id":5,"name":"Show","file_type":"FOLDER"},"files":[]}"#,
        );
        fake.push_json(200, r#"{"zip_id":31}"#);
        assert_eq!(
            prepare_download(&client(&fake), 5).unwrap(),
            Prepared::Zip(31)
        );
        let requests = fake.requests();
        assert_eq!(requests[1].path, "/zips/create");
        assert_eq!(requests[1].body, putio_api::Body::Form("file_ids=5".into()));
    }

    #[test]
    fn a_download_for_an_expired_session_says_so() {
        let fake = FakeTransport::new();
        fake.push_json(401, r#"{"error_type":"Unauthorized","error_message":"no"}"#);
        assert!(matches!(
            prepare_download(&client(&fake), 7),
            Err(ApiError::Unauthorized)
        ));
    }

    #[test]
    fn a_folder_becomes_a_name_ordered_playlist_of_its_videos() {
        let fake = FakeTransport::new();
        fake.push_json(
            200,
            r#"{"parent":{"id":5,"name":"Show","file_type":"FOLDER"},"files":[
                {"id":12,"name":"b.mkv","file_type":"VIDEO"},
                {"id":13,"name":"notes.txt","file_type":"TEXT"},
                {"id":11,"name":"A.mkv","file_type":"VIDEO"}]}"#,
        );
        fake.push_json(200, r#"{"url":"https://signed.invalid/a"}"#);
        fake.push_json(200, r#"{"url":"https://signed.invalid/b"}"#);
        let launcher = FakeLauncher::default();
        stream(&client(&fake), &launcher, 5).unwrap();
        assert_eq!(
            launcher.played.lock().unwrap()[0],
            ["https://signed.invalid/a", "https://signed.invalid/b"]
        );
        let paths: Vec<String> = fake.requests().iter().map(|r| r.path.clone()).collect();
        assert_eq!(paths[1], "/files/11/url");
        assert_eq!(paths[2], "/files/12/url");
    }

    #[test]
    fn a_folder_without_videos_has_nothing_to_play() {
        let fake = FakeTransport::new();
        fake.push_json(200, r#"{"parent":{"id":5,"name":"Docs","file_type":"FOLDER"},"files":[{"id":1,"name":"a.txt","file_type":"TEXT"}]}"#);
        assert_eq!(
            stream(&client(&fake), &FakeLauncher::default(), 5),
            Err(StreamFailure::NothingToPlay)
        );
    }

    #[test]
    fn a_missing_player_and_an_expired_session_are_reported() {
        let fake = FakeTransport::new();
        fake.push_json(404, "{}");
        fake.push_json(200, r#"{"url":"https://signed.invalid/f"}"#);
        let missing = FakeLauncher {
            missing: true,
            ..FakeLauncher::default()
        };
        assert_eq!(
            stream(&client(&fake), &missing, 1),
            Err(StreamFailure::NoPlayer)
        );

        let fake = FakeTransport::new();
        fake.push_json(401, "{}");
        assert_eq!(
            stream(&client(&fake), &FakeLauncher::default(), 1),
            Err(StreamFailure::Api(ApiError::Unauthorized))
        );
    }

    /// A `Workers` whose queues the test reads itself (no threads, no network).
    fn workers_without_threads() -> (Workers, Receiver<Queued>) {
        let (interactive, queue) = mpsc::channel();
        let (heavy, _) = mpsc::channel();
        let workers = Workers {
            interactive,
            heavy,
            session: Arc::new(Mutex::new(Session::default())),
        };
        (workers, queue)
    }

    fn token_named(value: &str) -> Token {
        Token::new(value).unwrap()
    }

    #[test]
    fn a_job_keeps_the_token_of_the_moment_it_was_sent() {
        let (workers, queue) = workers_without_threads();
        workers.set_token(Some(token_named("account-a")));
        workers.send(Job::Poll);
        // Another account signs in before a thread picks the job up.
        workers.set_token(Some(token_named("account-b")));
        let queued = queue.try_recv().unwrap();
        assert_eq!(queued.token.unwrap().expose(), "account-a");
    }

    #[test]
    fn an_answer_from_an_ended_session_is_thrown_away() {
        let (workers, queue) = workers_without_threads();
        workers.set_token(Some(token_named("account-a")));
        workers.send(Job::Poll);
        let session = queue.try_recv().unwrap().session;
        let answer = |session| Delivery {
            session,
            msg: WorkerMsg::Folders {
                parent: 0,
                result: Err(ApiError::Unauthorized),
            },
        };
        assert!(workers.accept(answer(session)).is_some());
        workers.set_token(None);
        assert!(workers.accept(answer(session)).is_none());
    }

    #[test]
    fn a_new_sign_in_attempt_ends_the_answers_to_the_old_code() {
        let (workers, queue) = workers_without_threads();
        workers.send(Job::PollCode("OLD".into()));
        let old = queue.try_recv().unwrap().session;
        workers.new_session();
        let delivery = Delivery {
            session: old,
            msg: WorkerMsg::Core(Event::LinkPolled(Ok(None))),
        };
        assert!(workers.accept(delivery).is_none());
    }

    #[test]
    fn answers_that_do_not_belong_to_an_account_survive_a_sign_out() {
        let (workers, queue) = workers_without_threads();
        workers.send(Job::LatestRelease);
        let queued = queue.try_recv().unwrap();
        assert_eq!(queued.session, None);
        workers.set_token(None);
        let delivery = Delivery {
            session: queued.session,
            msg: WorkerMsg::Core(Event::DefaultHandlerRegistered(Err("x".into()))),
        };
        assert!(workers.accept(delivery).is_some());
    }

    #[test]
    fn lanes_keep_uploads_away_from_polling() {
        assert!(
            Job::UploadTorrent {
                op: OpId(1),
                path: "x".into(),
                parent: None
            }
            .is_heavy()
        );
        assert!(
            Job::Stream {
                op: OpId(1),
                file_id: 1
            }
            .is_heavy()
        );
        for job in [
            Job::Poll,
            Job::DeviceCode,
            Job::LatestRelease,
            Job::Cancel { op: OpId(1), id: 1 },
            Job::LoadFeeds,
            Job::DeleteFeed { op: OpId(1), id: 1 },
            Job::FolderName { id: 1 },
            Job::AddMagnet {
                op: OpId(1),
                uri: String::new(),
                parent: None,
            },
        ] {
            assert!(!job.is_heavy(), "{job:?}");
        }
    }

    fn feed_settings() -> RssFeedSettings {
        RssFeedSettings {
            title: "Releases".into(),
            url: "https://example.invalid/feed.xml".into(),
            parent_dir_id: 3,
            ..RssFeedSettings::default()
        }
    }

    #[test]
    fn feeds_are_listed_and_the_answer_goes_to_the_core() {
        let fake = FakeTransport::new();
        fake.push_json(
            200,
            r#"{"status":"OK","feeds":[{"id":4,"title":"A","rss_source_url":"https://e.invalid/f"}]}"#,
        );
        let msg = execute(&client(&fake), Job::LoadFeeds);
        assert_eq!(fake.last_request().unwrap().describe(), "GET /rss/list");
        match msg {
            WorkerMsg::Core(Event::FeedsLoaded(Ok(feeds))) => {
                assert_eq!(feeds.len(), 1);
                assert_eq!(feeds[0].id, 4);
            }
            other => panic!("{other:?}"),
        }
        // A failure is carried to the core, which decides what to say.
        fake.push_json(500, r#"{"error_type":"Boom","error_message":"x"}"#);
        assert!(matches!(
            execute(&client(&fake), Job::LoadFeeds),
            WorkerMsg::Core(Event::FeedsLoaded(Err(_)))
        ));
    }

    #[test]
    fn saving_a_feed_creates_a_new_one_and_updates_an_existing_one() {
        let fake = FakeTransport::new();
        fake.push_json(200, r#"{"status":"OK"}"#);
        fake.push_json(200, r#"{"status":"OK"}"#);
        let create = execute(
            &client(&fake),
            Job::SaveFeed {
                op: OpId(5),
                id: None,
                settings: feed_settings(),
            },
        );
        assert!(matches!(
            create,
            WorkerMsg::Core(Event::FeedSaved {
                op: OpId(5),
                result: Ok(())
            })
        ));
        let update = execute(
            &client(&fake),
            Job::SaveFeed {
                op: OpId(6),
                id: Some(12),
                settings: feed_settings(),
            },
        );
        assert!(matches!(
            update,
            WorkerMsg::Core(Event::FeedSaved {
                op: OpId(6),
                result: Ok(())
            })
        ));
        let sent: Vec<String> = fake
            .requests()
            .iter()
            .map(putio_api::Request::describe)
            .collect();
        assert_eq!(sent, ["POST /rss/create", "POST /rss/12"]);
    }

    #[test]
    fn pausing_resuming_and_deleting_report_which_feed_they_were_about() {
        let fake = FakeTransport::new();
        for _ in 0..3 {
            fake.push_json(200, r#"{"status":"OK"}"#);
        }
        let c = client(&fake);
        let paused = execute(
            &c,
            Job::SetFeedPaused {
                op: OpId(1),
                id: 8,
                paused: true,
            },
        );
        assert!(matches!(
            paused,
            WorkerMsg::Core(Event::FeedPauseFinished {
                op: OpId(1),
                id: 8,
                paused: true,
                result: Ok(())
            })
        ));
        let resumed = execute(
            &c,
            Job::SetFeedPaused {
                op: OpId(2),
                id: 8,
                paused: false,
            },
        );
        assert!(matches!(
            resumed,
            WorkerMsg::Core(Event::FeedPauseFinished { paused: false, .. })
        ));
        let deleted = execute(&c, Job::DeleteFeed { op: OpId(3), id: 8 });
        assert!(matches!(
            deleted,
            WorkerMsg::Core(Event::FeedDeleteFinished {
                op: OpId(3),
                id: 8,
                result: Ok(())
            })
        ));
        let sent: Vec<String> = fake
            .requests()
            .iter()
            .map(putio_api::Request::describe)
            .collect();
        assert_eq!(
            sent,
            [
                "POST /rss/8/pause",
                "POST /rss/8/resume",
                "POST /rss/8/delete"
            ]
        );
    }

    #[test]
    fn a_folders_name_comes_back_or_not_at_all() {
        let fake = FakeTransport::new();
        fake.push_json(200, r#"{"file":{"id":3,"name":"TV Shows"}}"#);
        match execute(&client(&fake), Job::FolderName { id: 3 }) {
            WorkerMsg::FolderName { id: 3, name } => assert_eq!(name.as_deref(), Some("TV Shows")),
            other => panic!("{other:?}"),
        }
        fake.push_json(404, r#"{"error_type":"NotFound","error_message":"gone"}"#);
        match execute(&client(&fake), Job::FolderName { id: 3 }) {
            WorkerMsg::FolderName { id: 3, name } => assert_eq!(name, None),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn feed_jobs_belong_to_the_session_that_sent_them() {
        for job in [
            Job::LoadFeeds,
            Job::DeleteFeed { op: OpId(1), id: 1 },
            Job::SetFeedPaused {
                op: OpId(1),
                id: 1,
                paused: true,
            },
            Job::SaveFeed {
                op: OpId(1),
                id: None,
                settings: feed_settings(),
            },
            Job::FolderName { id: 1 },
        ] {
            assert!(job.is_session_bound(), "{job:?}");
        }
    }

    #[test]
    fn the_files_of_a_folder_are_listed_with_their_types_and_searched_by_name() {
        let fake = FakeTransport::new();
        fake.push_json(
            200,
            r#"{"parent":{"id":4,"name":"TV"},"files":[{"id":9,"name":"a.mkv","file_type":"VIDEO","size":5}]}"#,
        );
        match execute(&client(&fake), Job::ListFiles { parent: 4 }) {
            WorkerMsg::Core(Event::FilesListed {
                parent: 4,
                result: Ok(listing),
            }) => {
                assert_eq!(listing.files.len(), 1);
            }
            other => panic!("{other:?}"),
        }
        // Everything is listed, not only folders (the folder chooser asks for those).
        assert_eq!(
            fake.last_request().unwrap().describe(),
            "GET /files/list?parent_id=4&per_page=1000"
        );
        fake.push_json(200, r#"{"files":[],"total":0}"#);
        match execute(
            &client(&fake),
            Job::SearchFiles {
                query: "spring".into(),
            },
        ) {
            WorkerMsg::Core(Event::FilesSearched {
                query,
                result: Ok(found),
            }) => {
                assert_eq!((query.as_str(), found.total), ("spring", 0));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn renaming_making_a_folder_moving_and_deleting_report_their_own_operation() {
        let fake = FakeTransport::new();
        for _ in 0..4 {
            fake.push_json(200, r#"{"status":"OK"}"#);
        }
        let c = client(&fake);
        for job in [
            Job::RenameFile {
                op: OpId(1),
                id: 7,
                name: "new".into(),
            },
            Job::CreateFolder {
                op: OpId(2),
                parent: 3,
                name: "Season".into(),
            },
            Job::MoveFile {
                op: OpId(3),
                id: 7,
                to: 9,
            },
        ] {
            let expected = match &job {
                Job::RenameFile { op, .. }
                | Job::CreateFolder { op, .. }
                | Job::MoveFile { op, .. } => *op,
                _ => unreachable!(),
            };
            match execute(&c, job) {
                WorkerMsg::Core(Event::FileChangeFinished { op, result: Ok(()) }) => {
                    assert_eq!(op, expected);
                }
                other => panic!("{other:?}"),
            }
        }
        assert!(matches!(
            execute(&c, Job::DeleteFile { op: OpId(4), id: 7 }),
            WorkerMsg::Core(Event::FileDeleteFinished {
                op: OpId(4),
                id: 7,
                result: Ok(())
            })
        ));
        let sent: Vec<String> = fake
            .requests()
            .iter()
            .map(putio_api::Request::describe)
            .collect();
        assert_eq!(
            sent,
            [
                "POST /files/rename",
                "POST /files/create-folder",
                "POST /files/move",
                "POST /files/delete"
            ]
        );
    }

    #[test]
    fn file_jobs_belong_to_the_session_that_sent_them_and_are_not_heavy() {
        for job in [
            Job::ListFiles { parent: 0 },
            Job::SearchFiles { query: "x".into() },
            Job::RenameFile {
                op: OpId(1),
                id: 1,
                name: "x".into(),
            },
            Job::CreateFolder {
                op: OpId(1),
                parent: 0,
                name: "x".into(),
            },
            Job::MoveFile {
                op: OpId(1),
                id: 1,
                to: 0,
            },
            Job::DeleteFile { op: OpId(1), id: 1 },
        ] {
            assert!(job.is_session_bound(), "{job:?}");
            assert!(!job.is_heavy(), "{job:?}");
        }
    }
}
