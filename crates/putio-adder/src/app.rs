//! The eframe application: glue between the pure core state machine, the worker thread and the OS.

use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use eframe::egui::{self, ViewportCommand};
use putio_api::Token;
use putio_core::i18n::Locale;
use putio_core::intake::Intake;
use putio_core::settings::{Settings, ThemeChoice};
use putio_core::state::{Auth, Effect, Event, Millis, SecretProblem, State, update};
use putio_platform::badge;
use putio_platform::ipc::Server;
use putio_platform::notify::Notifier;
use putio_platform::opener::{SystemOpener, UrlOpener};
use putio_platform::secret::SecretStore;
use putio_platform::settings_store::SettingsFile;

use crate::keychain::Keychain;
use crate::ui::{self, UiState};
use crate::window::{Mode, Window};
use crate::worker::{Delivery, Job, WorkerMsg, Workers};

/// How often the UI wakes up by itself (to advance timers and the status line).
const FRAME_INTERVAL: Duration = Duration::from_millis(250);

/// Everything `main` prepares before the event loop starts.
pub(crate) struct Startup {
    pub(crate) settings: Settings,
    pub(crate) settings_file: Option<SettingsFile>,
    pub(crate) locale: Locale,
    pub(crate) secrets: Arc<dyn SecretStore>,
    /// Whether `secrets` keeps the sign-in beyond this run.
    pub(crate) secrets_persist: bool,
    pub(crate) notifier: Arc<dyn Notifier>,
    pub(crate) intake_rx: Receiver<String>,
    pub(crate) click_rx: Receiver<Option<i64>>,
    pub(crate) wake: Arc<OnceLock<egui::Context>>,
    pub(crate) ipc_server: Option<Server>,
    pub(crate) version: &'static str,
}

/// The mode the window starts in (before anything is known about the sign-in).
pub(crate) fn initial_mode(settings: &Settings) -> Mode {
    if settings.transfers_expanded {
        Mode::List
    } else {
        Mode::Collapsed
    }
}

/// Debug builds only: `PUTIO_ADDER_THEME=light|dark|system` overrides the saved theme, for previews.
fn with_debug_theme_override(mut settings: Settings) -> Settings {
    if cfg!(debug_assertions) {
        match std::env::var("PUTIO_ADDER_THEME").as_deref() {
            Ok("light") => settings.theme = ThemeChoice::Light,
            Ok("dark") => settings.theme = ThemeChoice::Dark,
            Ok("system") => settings.theme = ThemeChoice::System,
            _ => {}
        }
    }
    settings
}

/// Debug builds only: an update on offer, installable or not.
#[cfg(debug_assertions)]
fn demo_update(installable: bool) -> putio_core::update::UpdateStatus {
    putio_core::update::UpdateStatus::Available {
        version: "0.2.0".into(),
        url: "https://github.com/nicoSWD/put.io-adder/releases/tag/v0.2.0".into(),
        asset: installable.then(|| putio_core::update::UpdateAsset {
            name: "putio-adder-v0.2.0-macos-arm64.zip".into(),
            url: "https://github.com/nicoSWD/put.io-adder/releases/download/v0.2.0/putio-adder-v0.2.0-macos-arm64.zip".into(),
            size: 9_400_000,
        }),
    }
}

/// Debug builds only: `PUTIO_ADDER_DEMO=list|empty|login` shows sample data and never talks to put.io.
pub(crate) fn demo_requested() -> Option<String> {
    if cfg!(debug_assertions) {
        std::env::var("PUTIO_ADDER_DEMO").ok()
    } else {
        None
    }
}

pub(crate) fn title(progress: Option<u8>) -> String {
    match progress {
        Some(percent) => format!("{percent}% · put.io adder"),
        None => "put.io adder".to_owned(),
    }
}

pub(crate) struct App {
    pub(crate) state: State,
    pub(crate) ui: UiState,
    start: Instant,
    workers: Workers,
    from_worker: Receiver<Delivery>,
    intake_rx: Receiver<String>,
    click_rx: Receiver<Option<i64>>,
    keychain: Keychain,
    /// What the keychain thread could not do (read, save, delete), to tell the user.
    secret_problems: Receiver<SecretProblem>,
    /// The saved token, on its way from the keychain thread; `None` once it has arrived.
    pending_token: Option<Receiver<Option<Token>>>,
    settings_file: Option<SettingsFile>,
    notifier: Arc<dyn Notifier>,
    opener: SystemOpener,
    badge_percent: Option<u8>,
    /// An update that is downloaded and unpacked, waiting for the user's restart.
    prepared_update: Option<putio_platform::update::Prepared>,
    /// Debug builds only: show sample data and never talk to put.io.
    #[cfg(debug_assertions)]
    demo: Option<String>,
    /// Debug builds only: how many `PUTIO_ADDER_DEMO_TOGGLE` periods have passed.
    #[cfg(debug_assertions)]
    demo_round: u64,
    window: Window,
    last_title: String,
    last_theme: ThemeChoice,
    /// Whether the icon currently shown is the dark one (`None` until the first frame).
    last_icon_dark: Option<bool>,
    /// Whether the window's resize behaviour has been set up for the theme being shown.
    chrome_dark: Option<bool>,
    _ipc_server: Option<Server>,
}

impl App {
    pub(crate) fn new(cc: &eframe::CreationContext<'_>, startup: Startup) -> Self {
        let Startup {
            settings,
            settings_file,
            locale,
            secrets,
            secrets_persist,
            notifier,
            intake_rx,
            click_rx,
            wake,
            ipc_server,
            version,
        } = startup;
        let _ = wake.set(cc.egui_ctx.clone());
        let settings = with_debug_theme_override(settings);
        let theme = settings.theme;
        ui::theme::apply(&cc.egui_ctx, theme);
        // The native title bar follows the choice too (it cannot be set when the window is created).
        cc.egui_ctx
            .send_viewport_cmd(ViewportCommand::SetTheme(ui::theme::window_theme(theme)));

        let (out_tx, from_worker) = mpsc::channel();
        let workers = match Workers::spawn(&cc.egui_ctx, &out_tx, &format!("putio-adder/{version}"))
        {
            Ok(workers) => workers,
            Err(error) => {
                // Without its threads the app cannot work at all; say so plainly.
                eprintln!("could not start the network threads: {error}");
                std::process::exit(1);
            }
        };

        // The saved token comes from the keychain on its own thread: the system may ask the user for
        // permission, and the window must not wait for that.
        let (keychain, pending_token, secret_problems) =
            match Keychain::spawn(secrets, cc.egui_ctx.clone()) {
                Ok(pair) => pair,
                Err(error) => {
                    eprintln!("could not start the keychain thread: {error}");
                    std::process::exit(1);
                }
            };
        let demo = demo_requested();

        let window = Window::new(&settings, initial_mode(&settings));
        #[cfg_attr(not(debug_assertions), allow(unused_mut))]
        // only the debug-only demo start mutates it
        let mut app = Self {
            state: State::new(settings, locale, version),
            ui: UiState::default(),
            start: Instant::now(),
            workers,
            from_worker,
            intake_rx,
            click_rx,
            keychain,
            secret_problems,
            pending_token: demo.is_none().then_some(pending_token),
            settings_file,
            notifier,
            opener: SystemOpener,
            badge_percent: None,
            prepared_update: None,
            #[cfg(debug_assertions)]
            demo,
            #[cfg(debug_assertions)]
            demo_round: 0,
            window,
            last_title: title(None),
            last_theme: theme,
            last_icon_dark: None,
            chrome_dark: None,
            _ipc_server: ipc_server,
        };
        app.state.asset_name = putio_platform::update::asset_name;
        if !secrets_persist && !app.is_demo() {
            app.dispatch(
                &cc.egui_ctx,
                Event::SecretStoreFailed(SecretProblem::Unavailable),
            );
        }
        // `Started` is sent when the keychain thread reports (see `pump`), except in demo mode.
        #[cfg(debug_assertions)]
        if let Some(view) = app.demo.clone() {
            app.start_demo(&cc.egui_ctx, &view);
            return app;
        }
        // Whether this copy can replace itself (not a development build, a folder it may write to).
        let target = putio_platform::update::install_target().map(drop);
        app.dispatch(&cc.egui_ctx, Event::InstallTargetKnown(target));
        // Is this app the one the system opens magnet links and .torrent files with? (If not: ask.)
        app.dispatch(&cc.egui_ctx, Event::DefaultHandlerCheckRequested);
        app
    }

    /// A saved token arrived from the keychain thread: hand it to the workers and start up.
    fn token_loaded(&mut self, ctx: &egui::Context) {
        let Some(loaded) = &self.pending_token else {
            return;
        };
        let Ok(token) = loaded.try_recv() else { return };
        self.pending_token = None;
        let has_token = token.is_some();
        self.workers.set_token(token);
        self.dispatch(ctx, Event::Started { has_token });
    }

    /// Debug builds only: put the app into a state worth looking at, without any network.
    #[cfg(debug_assertions)]
    fn start_demo(&mut self, ctx: &egui::Context, view: &str) {
        if view == "login" {
            self.dispatch(ctx, Event::Started { has_token: false });
            let code = putio_api::DeviceCode {
                code: "YJUXZC".into(),
            };
            self.dispatch(ctx, Event::DeviceCodeReceived(Ok(code)));
            return;
        }
        self.dispatch(ctx, Event::Started { has_token: true });
        self.state.transfers_expanded = !view.starts_with("collapsed");
        // The demo drops the update check, so show its finished state instead of a spinner.
        self.state.update = putio_core::update::UpdateStatus::UpToDate;
        self.ui.prefs_open = matches!(view, "prefs" | "folders" | "prefs-update");
        self.ui.open_once = match view {
            "menu" | "menu-update" | "collapsed-menu" | "collapsed-menu-update" => {
                Some(crate::ui::Overlay::Menu)
            }
            "sort" => Some(crate::ui::Overlay::Sort),
            "card-menu" => Some(crate::ui::Overlay::Card(1)),
            "card-menu-seeding" => Some(crate::ui::Overlay::Card(6)),
            "card-menu-failed" => Some(crate::ui::Overlay::Card(7)),
            "card-menu-done" => Some(crate::ui::Overlay::Card(5)),
            _ => None,
        };
        self.ui.details = match view {
            "details" => Some(6),
            "details-failed" => Some(7),
            "details-downloading" => Some(1),
            "details-long" => Some(2),
            _ => None,
        };
        if matches!(view, "menu-update" | "collapsed-menu-update") {
            self.state.update = demo_update(true);
        }
        self.start_demo_update(view);
        // The default-app question and Preferences row, with a system that is not set up yet.
        self.state.handler_check = Some(putio_core::handler::Check::Known(
            putio_core::handler::Status {
                magnet: false,
                torrent: false,
            },
        ));
        self.state.handler_prompt = view == "default-app";
        if view == "folders" {
            self.ui.folders = crate::ui::prefs::FolderPicker::demo();
        }
        let transfers = if view == "empty" {
            Vec::new()
        } else {
            crate::demo::transfers()
        };
        let mut transfers = transfers;
        if view == "details-long"
            && let Some(item) = transfers.iter_mut().find(|t| t.id == 2)
        {
            item.name = "Some.Extremely.Long.Release.Name.With.No.Spaces.2160p.UHD.BluRay.REMUX.HDR10Plus.DV.TrueHD.7.1.Atmos.x265-GROUP".into();
        }
        let account = crate::demo::account();
        self.dispatch(
            ctx,
            Event::Polled {
                account: Ok(account),
                transfers: Ok(transfers),
            },
        );
        self.start_demo_feeds(ctx, view);
        self.start_demo_files(ctx, view);
    }

    /// Debug builds only: the Feeds tab in each of its states (`feeds`, `feeds-empty`, `feeds-loading`,
    /// `feeds-failed`, `feed-card-menu`) and its two cards (`feed-add`, `feed-edit`, `feed-delete`).
    #[cfg(debug_assertions)]
    fn start_demo_feeds(&mut self, ctx: &egui::Context, view: &str) {
        if !view.starts_with("feed") {
            return;
        }
        self.state.transfers_expanded = true;
        self.dispatch(ctx, Event::SelectTab(putio_core::settings::Tab::Feeds));
        let items = match view {
            "feeds-empty" | "feeds-loading" | "feeds-failed" => Vec::new(),
            _ => crate::demo::feeds(),
        };
        self.state.feeds = putio_core::state::FeedList {
            loaded: !matches!(view, "feeds-loading" | "feeds-failed"),
            failed: view == "feeds-failed",
            items,
        };
        if view == "feed-card-menu" {
            self.ui.open_once = Some(crate::ui::Overlay::FeedCard(5));
        }
        ui::feed_form::demo(self, ctx, view);
    }

    /// Debug builds only: the Files tab in each of its states (`files`, `files-empty`, `files-loading`,
    /// `files-failed`, `files-search`, `files-deep`, `file-card-menu`) and its cards (`file-rename`,
    /// `file-new-folder`, `file-move`, `file-delete`, `file-delete-folder`).
    #[cfg(debug_assertions)]
    fn start_demo_files(&mut self, ctx: &egui::Context, view: &str) {
        use putio_core::state::{Crumb, FilesView, Search};
        if !view.starts_with("file") {
            return;
        }
        self.state.transfers_expanded = true;
        self.dispatch(ctx, Event::SelectTab(putio_core::settings::Tab::Files));
        let mut items = crate::demo::files();
        putio_core::files::sort_files(&mut items);
        let trail = if view == "files-deep" {
            vec![
                Crumb {
                    id: 0,
                    name: String::new(),
                },
                Crumb {
                    id: 11,
                    name: "Movies".into(),
                },
                Crumb {
                    id: 31,
                    name: "2024".into(),
                },
                Crumb {
                    id: 32,
                    name: "Documentaries and other long things".into(),
                },
                Crumb {
                    id: 33,
                    name: "Behind the scenes".into(),
                },
            ]
        } else if view == "files-long" {
            vec![
                Crumb {
                    id: 0,
                    name: String::new(),
                },
                Crumb {
                    id: 11,
                    name: "A folder with a very long name that goes on and on and on and on and on"
                        .into(),
                },
            ]
        } else {
            vec![Crumb {
                id: 0,
                name: String::new(),
            }]
        };
        self.state.files = FilesView {
            trail,
            items: if matches!(view, "files-empty" | "files-loading" | "files-failed") {
                Vec::new()
            } else {
                items.clone()
            },
            loaded: !matches!(view, "files-loading" | "files-failed"),
            failed: view == "files-failed",
            search: (view == "files-search").then(|| Search {
                query: "sintel".into(),
                items: items.iter().take(4).cloned().collect(),
                total: 128,
                loading: false,
                failed: false,
            }),
        };
        if view == "files-search" {
            self.ui.search_text = "sintel".into();
        }
        if view == "file-card-menu" {
            self.ui.open_once = Some(crate::ui::Overlay::FileCard(14));
        }
        if matches!(view, "file-extract" | "file-password") {
            // The archive of the demo files (id 17), being unpacked or waiting for a password.
            self.dispatch(
                ctx,
                Event::FileExtractRequested {
                    id: 17,
                    password: None,
                },
            );
            let status = if view == "file-extract" {
                putio_api::ExtractStatus::Extracting
            } else {
                putio_api::ExtractStatus::Password
            };
            self.dispatch(
                ctx,
                Event::ExtractionsListed(Ok(vec![putio_api::Extraction {
                    id: 1,
                    name: "Big.Buck.Bunny.2008.2160p.zip".into(),
                    status,
                    message: None,
                    files: vec![17],
                }])),
            );
        }
        ui::file_dialogs::demo(self, ctx, view);
    }

    /// Debug builds only: the update flow in each of its states (`update`, `update-installing`,
    /// `update-ready`, `update-failed`, `update-manual`).
    #[cfg(debug_assertions)]
    fn start_demo_update(&mut self, view: &str) {
        use putio_core::state::{Install, InstallTarget, UpdateStage};
        if !view.starts_with("update") && view != "prefs-update" {
            return;
        }
        self.state.update = demo_update(view != "update-manual");
        self.state.install_target = if view == "update-manual" {
            InstallTarget::Unavailable("A development build does not update itself.".into())
        } else {
            InstallTarget::Ready
        };
        self.state.install = match view {
            "update-installing" => Install::Working {
                stage: UpdateStage::Downloading,
                fraction: 0.46,
            },
            "update-ready" => Install::Ready {
                version: "0.2.0".into(),
            },
            "update-failed" => Install::Failed {
                version: "0.2.0".into(),
                message: "The download is not a put.io adder app.".into(),
            },
            _ => Install::Idle,
        };
    }

    #[cfg_attr(not(debug_assertions), allow(clippy::unused_self))] // demo mode is compiled out of release builds
    fn is_demo(&self) -> bool {
        #[cfg(debug_assertions)]
        return self.demo.is_some();
        #[cfg(not(debug_assertions))]
        false
    }

    pub(crate) fn now(&self) -> Millis {
        u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// Feed one event to the core and carry out what it asks for.
    pub(crate) fn dispatch(&mut self, ctx: &egui::Context, event: Event) {
        let now = self.now();
        let effects = update(&mut self.state, now, event);
        for effect in effects {
            self.run(ctx, effect);
        }
    }

    pub(crate) fn send(&self, job: Job) {
        // A demo run never talks to the network (not even for the update check).
        if self.is_demo() {
            return;
        }
        self.workers.send(job);
    }

    /// A handle for sending jobs from UI code that also borrows `self` mutably.
    pub(crate) fn workers(&self) -> Workers {
        self.workers.clone()
    }

    fn run(&mut self, ctx: &egui::Context, effect: Effect) {
        match effect {
            // Demo mode never touches the network.
            Effect::RequestDeviceCode
            | Effect::PollDeviceCode { .. }
            | Effect::Poll
            | Effect::ApplyUpdate
                if self.is_demo() => {}
            Effect::RequestDeviceCode => {
                // A new attempt: the answers to an earlier code are no longer wanted.
                self.workers.new_session();
                self.send(Job::DeviceCode);
            }
            Effect::PollDeviceCode { code } => self.send(Job::PollCode(code)),
            Effect::StoreToken(token) => self.store_token(token),
            Effect::DeleteToken => {
                self.keychain.delete();
                self.workers.set_token(None);
            }
            Effect::SaveSettings(settings) => {
                if let Some(file) = &self.settings_file
                    && let Err(error) = file.save(&settings.to_json())
                {
                    eprintln!("could not save settings: {error}");
                }
            }
            Effect::Poll => self.send(Job::Poll),
            Effect::AddMagnet { op, uri, parent } => self.send(Job::AddMagnet { op, uri, parent }),
            Effect::UploadTorrent {
                op,
                path,
                parent,
                trash_after,
            } => self.send(Job::UploadTorrent {
                op,
                path,
                parent,
                trash_after,
            }),
            Effect::LoadFeeds => self.send(Job::LoadFeeds),
            Effect::SaveFeed { op, id, settings } => self.send(Job::SaveFeed { op, id, settings }),
            Effect::SetFeedPaused { op, id, paused } => {
                self.send(Job::SetFeedPaused { op, id, paused });
            }
            Effect::DeleteFeed { op, id } => self.send(Job::DeleteFeed { op, id }),
            Effect::ListFiles { parent } => self.send(Job::ListFiles { parent }),
            Effect::SearchFiles { query } => self.send(Job::SearchFiles { query }),
            Effect::RenameFile { op, id, name } => self.send(Job::RenameFile { op, id, name }),
            Effect::CreateFolder { op, parent, name } => {
                self.send(Job::CreateFolder { op, parent, name });
            }
            Effect::MoveFile { op, id, to } => self.send(Job::MoveFile { op, id, to }),
            Effect::DeleteFile { op, id } => self.send(Job::DeleteFile { op, id }),
            Effect::ExtractFile { op, id, password } => {
                self.send(Job::ExtractFile { op, id, password });
            }
            Effect::ListExtractions => self.send(Job::ListExtractions),
            Effect::CancelTransfer { op, id } => self.send(Job::Cancel { op, id }),
            Effect::RetryTransfer { op, id } => self.send(Job::Retry { op, id }),
            Effect::CleanTransfers { op, ids } => self.send(Job::Clean { op, ids }),
            Effect::PrepareDownload { op, file_id } => {
                self.send(Job::PrepareDownload { op, file_id });
            }
            Effect::PollZip { op, zip_id } => self.send(Job::PollZip { op, zip_id }),
            // The address is a credential: on failure say that it failed, never what it was.
            Effect::OpenDownload(url) if self.is_demo() => {
                drop(url);
                self.dispatch(ctx, Event::DownloadHandedOver { opened: true });
            }
            Effect::OpenDownload(url) => {
                let opened = self.opener.open_download(&url).is_ok();
                if !opened {
                    eprintln!("could not open the download in the browser");
                }
                self.dispatch(ctx, Event::DownloadHandedOver { opened });
            }
            Effect::PrepareUpdate { op, asset } => self.send(Job::PrepareUpdate { op, asset }),
            Effect::ApplyUpdate => self.apply_update(ctx),
            Effect::CheckDefaultHandler => self.send(Job::CheckDefaultHandler),
            Effect::RegisterDefaultHandler => self.send(Job::RegisterDefaultHandler),
            Effect::CopyText(text) => ctx.copy_text(text),
            Effect::Stream { op, file_id } => self.send(Job::Stream { op, file_id }),
            Effect::FetchLatestRelease => self.send(Job::LatestRelease),
            Effect::Notify(notification) => self.notifier.notify(&notification),
            Effect::SetBadge(percent) => self.badge_percent = percent,
            Effect::OpenUrl(url) => {
                if let Err(error) = self.opener.open(&url) {
                    eprintln!("could not open {url}: {error}");
                    self.dispatch(ctx, Event::PageOpenFailed);
                }
            }
            Effect::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
        }
    }

    /// Put the prepared update in place and relaunch, and tell the core how it went.
    fn apply_update(&mut self, ctx: &egui::Context) {
        let result = match self.prepared_update.clone() {
            Some(prepared) => putio_platform::update::apply(&prepared).map(|()| {
                // The new version is in place. If nobody can reopen the app, say so before it quits:
                // "try again" would only download the same update once more.
                if let Err(reason) = putio_platform::update::relaunch(&prepared) {
                    self.notifier.notify(&putio_core::state::Notification {
                        title: "put.io adder".to_owned(),
                        body: reason,
                        file_id: None,
                    });
                }
            }),
            None => Err("There is no update to install.".to_owned()),
        };
        self.dispatch(ctx, Event::UpdateApplied(result));
    }

    fn store_token(&mut self, token: Token) {
        self.keychain.save(token.clone());
        self.workers.set_token(Some(token));
    }

    /// Handle one string handed over by the OS or another launch.
    fn intake(&mut self, ctx: &egui::Context, raw: &str) {
        if raw.trim().is_empty() {
            // A relaunch without arguments: bring the window forward.
            ctx.send_viewport_cmd(ViewportCommand::Focus);
            return;
        }
        match Intake::parse(raw) {
            Ok(item) => {
                ctx.send_viewport_cmd(ViewportCommand::Focus);
                self.dispatch(ctx, Event::Intake(item));
            }
            Err(error) => self.dispatch(ctx, Event::IntakeRejected(error)),
        }
    }

    fn pump(&mut self, ctx: &egui::Context) {
        self.token_loaded(ctx);
        if self.state.is_signed_in()
            && !self.ui.prefs_open
            && !self.ui.feed_dialog.is_open()
            && !self.ui.file_dialog.is_open()
            && self.state.extract_ask().is_none()
            && let Some(tab) = ui::tabs::pressed(ctx)
        {
            // A shortcut for a list also opens the list if it was folded away.
            if !self.state.transfers_expanded {
                self.dispatch(ctx, Event::ToggleTransfers);
            }
            self.dispatch(ctx, Event::SelectTab(tab));
        }
        if ctx.input_mut(|input| input.consume_shortcut(&ui::menu::preferences_shortcut())) {
            self.ui.prefs_open = true;
        }
        while let Ok(delivery) = self.from_worker.try_recv() {
            let Some(message) = self.workers.accept(delivery) else {
                continue; // from a session that has ended
            };
            match message {
                WorkerMsg::Core(event) => self.dispatch(ctx, event),
                WorkerMsg::Folders { parent, result } => self.ui.folder_listed(parent, result),
                WorkerMsg::FolderName { id, name } => self.ui.folder_named(id, name),
                WorkerMsg::UpdatePrepared { op, result } => {
                    let result = result.map(|prepared| self.prepared_update = Some(prepared));
                    self.dispatch(ctx, Event::UpdatePrepared { op, result });
                }
            }
        }
        while let Ok(problem) = self.secret_problems.try_recv() {
            self.dispatch(ctx, Event::SecretStoreFailed(problem));
        }
        while let Ok(raw) = self.intake_rx.try_recv() {
            self.intake(ctx, &raw);
        }
        while let Ok(file_id) = self.click_rx.try_recv() {
            self.dispatch(ctx, Event::NotificationClicked { file_id });
        }
        self.dispatch(ctx, Event::Tick);
        #[cfg(debug_assertions)]
        self.demo_toggle(ctx);
    }

    /// Debug builds only: `PUTIO_ADDER_DEMO_TOGGLE=<ms>` opens and closes the list every `<ms>`
    /// milliseconds, to look at the window animation without clicking.
    #[cfg(debug_assertions)]
    fn demo_toggle(&mut self, ctx: &egui::Context) {
        let Some(every) = std::env::var("PUTIO_ADDER_DEMO_TOGGLE")
            .ok()
            .and_then(|ms| ms.parse::<u64>().ok())
            .filter(|ms| *ms >= 200 && self.demo.is_some())
        else {
            return;
        };
        let round = self.now() / every;
        if round > 0 && round != self.demo_round {
            self.demo_round = round;
            self.dispatch(ctx, Event::ToggleTransfers);
        }
    }

    /// Resize the window and keep its title current.
    /// Keep the app icon in step with the theme being shown.
    fn sync_icon(&mut self, ctx: &egui::Context) {
        let dark = ctx.theme() == egui::Theme::Dark;
        if self.last_icon_dark == Some(dark) {
            return;
        }
        self.last_icon_dark = Some(dark);
        // macOS sets the Dock icon directly; elsewhere the toolkit sets the window icon.
        if !putio_platform::app_icon::set_dock_icon(crate::app_icon::dock_png(dark))
            && let Some(icon) = crate::app_icon::window_icon(dark)
        {
            ctx.send_viewport_cmd(ViewportCommand::Icon(Some(icon)));
        }
        #[cfg(all(debug_assertions, target_os = "macos"))]
        if let Some(path) = std::env::var_os("PUTIO_ADDER_ICON_DUMP") {
            let _ = putio_platform::app_icon::dump_dock_icon(std::path::Path::new(&path));
        }
    }

    /// Pin the window's content to its corner while it is resized, and paint what is behind it in the
    /// app's background colour (see `putio_platform::window_chrome`). Retried every frame until the
    /// renderer's layer exists, then again whenever the theme changes.
    fn sync_chrome(&mut self, ctx: &egui::Context) {
        let dark = ctx.theme() == egui::Theme::Dark;
        if self.chrome_dark == Some(dark) {
            return;
        }
        ui::theme::select(ctx);
        let bg = ui::theme::pal().bg;
        if putio_platform::window_chrome::calm_live_resize([bg.r(), bg.g(), bg.b()]) {
            self.chrome_dark = Some(dark);
        }
    }

    /// Ease the window to the size its mode calls for, follow the user resizing it, and remember a size
    /// the user settled on.
    fn sync_size(&mut self, ctx: &egui::Context, mode: Mode) {
        let now = Instant::now();
        let actual = ctx.input(|input| input.viewport().inner_rect.map(|rect| rect.size()));
        let step = self.window.step(mode, actual, now);
        for command in step.commands {
            ctx.send_viewport_cmd(command);
        }
        if step.animating {
            ctx.request_repaint();
        }
        if let Some(wait) = self.window.save_due_in(now) {
            ctx.request_repaint_after(wait);
        }
        if let Some(size) = step.save {
            self.dispatch(
                ctx,
                Event::SettingsChanged(putio_core::state::SettingsChange::WindowSize(size)),
            );
        }
    }

    fn sync_window(&mut self, ctx: &egui::Context) {
        // The sign-in panel and the Preferences dialog need a fixed height, whatever the list is doing.
        let mode = if self.ui.prefs_open {
            Mode::Prefs
        } else if matches!(
            self.state.auth,
            Auth::RequestingCode | Auth::Linking(_) | Auth::SignedOut(_)
        ) {
            Mode::Login
        } else if self.state.transfers_expanded {
            Mode::List
        } else if let Some(update_row) = ui::menu::window_room(self) {
            // The menu is drawn inside the window, which has no room for it while collapsed.
            Mode::Menu { update_row }
        } else {
            Mode::Collapsed
        };
        self.sync_icon(ctx);
        // The theme preference changed (Preferences): switch the palette and the native title bar.
        if self.state.settings.theme != self.last_theme {
            self.last_theme = self.state.settings.theme;
            ctx.set_theme(ui::theme::preference(self.last_theme));
            ctx.send_viewport_cmd(ViewportCommand::SetTheme(ui::theme::window_theme(
                self.last_theme,
            )));
        }
        self.sync_chrome(ctx);
        self.sync_size(ctx, mode);
        // No Dock badge off macOS: show progress in the title instead.
        let progress = if cfg!(target_os = "macos") {
            None
        } else {
            self.badge_percent
        };
        let wanted = title(progress);
        if wanted != self.last_title {
            ctx.send_viewport_cmd(ViewportCommand::Title(wanted.clone()));
            self.last_title = wanted;
        }
        // The Dock badge must be set on the main thread, which is this one.
        let _ = badge::set_dock_badge(self.badge_percent);
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump(ctx);
        self.sync_window(ctx);
        ctx.request_repaint_after(FRAME_INTERVAL);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui::show(ui, self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_title_has_no_version() {
        assert_eq!(title(None), "put.io adder");
        assert_eq!(title(Some(42)), "42% · put.io adder");
        assert!(!title(None).contains(char::is_numeric));
    }
}
