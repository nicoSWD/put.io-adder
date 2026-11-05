//! The background threads that talk to put.io.
//!
//! The UI never blocks on the network: it sends [`Job`]s here and gets [`WorkerMsg`]s back, then is
//! woken with `request_repaint`. No async runtime, just two small thread pools ("lanes"):
//!
//! * **interactive** (3 threads): sign-in, polling, adding a magnet, cancelling, update checks, the
//!   folder picker. These are quick and must never wait behind a big upload.
//! * **heavy** (2 threads): `.torrent` uploads (up to two minutes on a slow link) and streaming set-up.
//!
//! The token is shared state that the UI sets directly (not a queued job), so a job can never run
//! with a token older than the one the UI just set. Each job takes a snapshot when it starts.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use putio_api::client::FileListing;
use putio_api::{ApiError, Client, FileType, Token, Transport, UploadOutcome, UreqTransport};
use putio_core::state::{Event, OpId, Prepared, StreamFailure};
use putio_core::update::UpdateAsset;
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
    LatestRelease,
    /// Folders inside `parent`, for the folder picker (UI-local, not part of the core state machine).
    ListFolders {
        parent: i64,
    },
}

impl Job {
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
    /// An update is downloaded and unpacked. The unpacked update itself (paths) stays with the app,
    /// not in the core state machine, which is told only whether it worked.
    UpdatePrepared {
        op: OpId,
        result: Result<update::Prepared, String>,
    },
}

/// Handle for sending work. Cheap to clone.
#[derive(Debug, Clone)]
pub(crate) struct Workers {
    interactive: Sender<Job>,
    heavy: Sender<Job>,
    token: Arc<Mutex<Option<Token>>>,
}

impl Workers {
    pub(crate) fn spawn(
        ctx: &eframe::egui::Context,
        out: &Sender<WorkerMsg>,
        user_agent: &str,
    ) -> std::io::Result<Self> {
        let transport = Arc::new(UreqTransport::new(user_agent));
        let token = Arc::new(Mutex::new(None));
        let lane = |name: &str, threads: usize| -> std::io::Result<Sender<Job>> {
            let (tx, rx) = mpsc::channel::<Job>();
            let rx = Arc::new(Mutex::new(rx));
            for index in 0..threads {
                let (rx, out, ctx) = (Arc::clone(&rx), out.clone(), ctx.clone());
                let (transport, token) = (Arc::clone(&transport), Arc::clone(&token));
                thread::Builder::new()
                    .name(format!("putio-{name}-{index}"))
                    .spawn(move || run(&rx, &out, &ctx, &transport, &token))?;
            }
            Ok(tx)
        };
        Ok(Self {
            interactive: lane("interactive", INTERACTIVE_THREADS)?,
            heavy: lane("heavy", HEAVY_THREADS)?,
            token,
        })
    }

    /// Set (or clear) the token used by every job that starts from now on.
    pub(crate) fn set_token(&self, token: Option<Token>) {
        if let Ok(mut slot) = self.token.lock() {
            *slot = token;
        }
    }

    pub(crate) fn send(&self, job: Job) {
        let lane = if job.is_heavy() {
            &self.heavy
        } else {
            &self.interactive
        };
        let _ = lane.send(job);
    }
}

fn run(
    jobs: &Mutex<Receiver<Job>>,
    out: &Sender<WorkerMsg>,
    ctx: &eframe::egui::Context,
    transport: &Arc<UreqTransport>,
    token: &Mutex<Option<Token>>,
) {
    loop {
        // Hold the queue lock only while waiting for the next job, never while working on it.
        let job = match jobs.lock() {
            Ok(queue) => queue.recv(),
            Err(_) => return,
        };
        let Ok(job) = job else { return };
        if let Job::PrepareUpdate { op, asset } = job {
            let result = prepare_update(op, &asset, out, ctx);
            if out.send(WorkerMsg::UpdatePrepared { op, result }).is_err() {
                return;
            }
            ctx.request_repaint();
            continue;
        }
        let snapshot = token.lock().ok().and_then(|slot| slot.clone());
        let client = match snapshot {
            Some(token) => Client::with_token(Arc::clone(transport), token),
            None => Client::new(Arc::clone(transport)),
        };
        if out.send(execute(&client, job)).is_err() {
            return; // the UI is gone
        }
        ctx.request_repaint();
    }
}

/// Download and unpack the update, telling the UI how far along it is.
fn prepare_update(
    op: OpId,
    asset: &UpdateAsset,
    out: &Sender<WorkerMsg>,
    ctx: &eframe::egui::Context,
) -> Result<update::Prepared, String> {
    let target = update::install_target()?;
    update::prepare(&target, asset, &|stage, fraction| {
        let _ = out.send(WorkerMsg::Core(Event::UpdateProgress {
            op,
            stage,
            fraction,
        }));
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
        Job::LatestRelease => WorkerMsg::Core(Event::ReleaseFetched(client.latest_release())),
        Job::ListFolders { parent } => WorkerMsg::Folders {
            parent,
            result: client.list_files(parent, Some(FileType::Folder)),
        },
    }
}

/// Decide how to download `file_id`: a plain file has a signed address of its own, a folder has to be
/// zipped first (put.io builds the ZIP in the background; the reducer polls for it).
fn prepare_download<T: Transport>(client: &Client<T>, file_id: i64) -> Result<Prepared, ApiError> {
    let is_folder = match client.list_files(file_id, None) {
        Ok(listing) => listing
            .parent
            .is_some_and(|parent| parent.file_type == FileType::Folder),
        Err(ApiError::Unauthorized) => return Err(ApiError::Unauthorized),
        // Not listable: it is a plain file.
        Err(_) => false,
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
        Err(ApiError::Unauthorized) => return Err(api(ApiError::Unauthorized)),
        // Not listable: treat it as a plain file.
        Err(_) => None,
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
            Job::AddMagnet {
                op: OpId(1),
                uri: String::new(),
                parent: None,
            },
        ] {
            assert!(!job.is_heavy(), "{job:?}");
        }
    }
}
