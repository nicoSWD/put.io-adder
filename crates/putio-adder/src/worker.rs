//! The background thread that talks to put.io.
//!
//! The UI never blocks on the network: it sends [`Job`]s here and gets [`WorkerMsg`]s back, then is
//! woken with `request_repaint`. One thread, one job at a time, no async runtime.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::thread::{self, JoinHandle};

use putio_api::client::FileListing;
use putio_api::{ApiError, Client, FileType, Token, UploadOutcome, UreqTransport};
use putio_core::state::{Event, OpId, StreamFailure};
use putio_platform::player::{PlayError, PlayerLauncher, VlcLauncher};
use putio_platform::torrent_file;

/// Most videos queued from one folder.
const MAX_PLAYLIST: usize = 50;

#[derive(Debug)]
pub(crate) enum Job {
    SetToken(Option<Token>),
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
    Stream {
        op: OpId,
        file_id: i64,
    },
    LatestRelease,
    /// Folders inside `parent`, for the folder picker (UI-local, not part of the core state machine).
    ListFolders {
        parent: i64,
    },
}

#[derive(Debug)]
pub(crate) enum WorkerMsg {
    Core(Event),
    Folders {
        parent: i64,
        result: Result<FileListing, ApiError>,
    },
}

/// What the app identifies itself as in the put.io "linked apps" list.
const CLIENT_NAME: &str = "put.io adder";
/// The put.io application id used for the device (OOB) sign-in. A public identifier, not a secret.
pub(crate) const APP_ID: u32 = 711;

pub(crate) fn spawn(
    ctx: eframe::egui::Context,
    jobs: Receiver<Job>,
    out: Sender<WorkerMsg>,
    user_agent: String,
) -> std::io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("putio-worker".into())
        .spawn(move || {
            let mut client = Client::new(UreqTransport::new(&user_agent));
            let launcher = VlcLauncher;
            while let Ok(job) = jobs.recv() {
                let msg = match job {
                    Job::SetToken(token) => {
                        client.set_token(token);
                        continue;
                    }
                    Job::DeviceCode => WorkerMsg::Core(Event::DeviceCodeReceived(
                        client.oob_code(APP_ID, CLIENT_NAME),
                    )),
                    Job::PollCode(code) => {
                        WorkerMsg::Core(Event::LinkPolled(client.oob_poll(&code)))
                    }
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
                    Job::UploadTorrent { op, path, parent } => {
                        match torrent_file::read_torrent(&path) {
                            Err(error) => WorkerMsg::Core(Event::TorrentUnusable {
                                op,
                                reason: error.to_string(),
                            }),
                            Ok(file) => {
                                let result = client
                                    .upload_torrent(&file.name, &file.bytes, parent)
                                    .map(|outcome| match outcome {
                                        UploadOutcome::Transfer(transfer) => Some(transfer.id),
                                        UploadOutcome::File(_) => None,
                                    });
                                WorkerMsg::Core(Event::AddFinished { op, result })
                            }
                        }
                    }
                    Job::Cancel { op, id } => WorkerMsg::Core(Event::CancelFinished {
                        op,
                        result: client.cancel_transfers(&[id]),
                    }),
                    Job::Stream { op, file_id } => WorkerMsg::Core(Event::StreamFinished {
                        op,
                        result: stream(&client, &launcher, file_id),
                    }),
                    Job::LatestRelease => {
                        WorkerMsg::Core(Event::ReleaseFetched(client.latest_release()))
                    }
                    Job::ListFolders { parent } => WorkerMsg::Folders {
                        parent,
                        result: client.list_files(parent, Some(FileType::Folder)),
                    },
                };
                if out.send(msg).is_err() {
                    break; // the UI is gone
                }
                ctx.request_repaint();
            }
        })
}

/// Resolve playable URLs and hand them to the player. A folder becomes a playlist of its videos in
/// name order; a single file is played directly. Only signed, IP-bound URLs reach the player's command
/// line, never the account token.
fn stream(
    client: &Client<UreqTransport>,
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
