//! The eframe application: glue between the pure core state machine, the worker thread and the OS.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use eframe::egui::{self, ViewportCommand};
use putio_api::Token;
use putio_core::i18n::Locale;
use putio_core::intake::Intake;
use putio_core::settings::Settings;
use putio_core::state::{Effect, Event, Millis, State, update};
use putio_platform::badge;
use putio_platform::ipc::Server;
use putio_platform::notify::Notifier;
use putio_platform::opener::{SystemOpener, UrlOpener};
use putio_platform::secret::SecretStore;
use putio_platform::settings_store::SettingsFile;

use crate::ui::{self, UiState};
use crate::worker::{self, Job, WorkerMsg};

const COLLAPSED_HEIGHT: f32 = 86.0;
const EXPANDED_HEIGHT: f32 = 300.0;
/// How often the UI wakes up by itself (to advance timers and the status line).
const FRAME_INTERVAL: Duration = Duration::from_millis(250);

/// Everything `main` prepares before the event loop starts.
pub(crate) struct Startup {
    pub(crate) settings: Settings,
    pub(crate) settings_file: Option<SettingsFile>,
    pub(crate) locale: Locale,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) notifier: Arc<dyn Notifier>,
    pub(crate) intake_rx: Receiver<String>,
    pub(crate) click_rx: Receiver<Option<i64>>,
    pub(crate) wake: Arc<OnceLock<egui::Context>>,
    pub(crate) ipc_server: Option<Server>,
    pub(crate) version: &'static str,
}

/// The window is tall when the list is open, and always when signing in (the code needs room).
pub(crate) fn window_height(expanded: bool, signed_in: bool) -> f32 {
    if expanded || !signed_in {
        EXPANDED_HEIGHT
    } else {
        COLLAPSED_HEIGHT
    }
}

pub(crate) fn title(version: &str, progress: Option<u8>) -> String {
    match progress {
        Some(percent) => format!("{percent}% · Put.io Adder v{version}"),
        None => format!("Put.io Adder v{version}"),
    }
}

/// Send side of the worker queue.
#[derive(Debug, Clone)]
pub(crate) struct JobSender(Sender<Job>);

impl JobSender {
    pub(crate) fn send_job(&self, job: Job) {
        let _ = self.0.send(job);
    }
}

pub(crate) struct App {
    pub(crate) state: State,
    pub(crate) ui: UiState,
    start: Instant,
    jobs: Sender<Job>,
    from_worker: Receiver<WorkerMsg>,
    intake_rx: Receiver<String>,
    click_rx: Receiver<Option<i64>>,
    secrets: Arc<dyn SecretStore>,
    settings_file: Option<SettingsFile>,
    notifier: Arc<dyn Notifier>,
    opener: SystemOpener,
    version: &'static str,
    badge_percent: Option<u8>,
    last_height: f32,
    last_title: String,
    _ipc_server: Option<Server>,
}

impl App {
    pub(crate) fn new(cc: &eframe::CreationContext<'_>, startup: Startup) -> Self {
        let Startup {
            settings,
            settings_file,
            locale,
            secrets,
            notifier,
            intake_rx,
            click_rx,
            wake,
            ipc_server,
            version,
        } = startup;
        let _ = wake.set(cc.egui_ctx.clone());
        ui::theme::apply(&cc.egui_ctx);

        let (jobs, job_rx) = mpsc::channel();
        let (out_tx, from_worker) = mpsc::channel();
        if let Err(error) = worker::spawn(
            cc.egui_ctx.clone(),
            job_rx,
            out_tx,
            format!("putio-adder/{version}"),
        ) {
            eprintln!("could not start the network thread: {error}");
        }

        // Token from the secret store (if any) goes to the worker before the first poll.
        let token = match secrets.load() {
            Ok(token) => token,
            Err(error) => {
                eprintln!("could not read the saved sign-in: {error}");
                None
            }
        };
        let has_token = token.is_some();
        let _ = jobs.send(Job::SetToken(token));

        let height = window_height(settings.transfers_expanded, has_token);
        let mut app = Self {
            state: State::new(settings, locale, version),
            ui: UiState::default(),
            start: Instant::now(),
            jobs,
            from_worker,
            intake_rx,
            click_rx,
            secrets,
            settings_file,
            notifier,
            opener: SystemOpener,
            version,
            badge_percent: None,
            last_height: height,
            last_title: title(version, None),
            _ipc_server: ipc_server,
        };
        app.dispatch(&cc.egui_ctx, Event::Started { has_token });
        app
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
        let _ = self.jobs.send(job);
    }

    /// A cloneable handle for sending jobs from UI code that also borrows `self` mutably.
    pub(crate) fn jobs_sender(&self) -> JobSender {
        JobSender(self.jobs.clone())
    }

    fn run(&mut self, ctx: &egui::Context, effect: Effect) {
        match effect {
            Effect::RequestDeviceCode => self.send(Job::DeviceCode),
            Effect::PollDeviceCode { code } => self.send(Job::PollCode(code)),
            Effect::StoreToken(token) => self.store_token(token),
            Effect::DeleteToken => {
                if let Err(error) = self.secrets.delete() {
                    eprintln!("could not remove the saved sign-in: {error}");
                }
                self.send(Job::SetToken(None));
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
            Effect::UploadTorrent { op, path, parent } => {
                self.send(Job::UploadTorrent { op, path, parent });
            }
            Effect::CancelTransfer { op, id } => self.send(Job::Cancel { op, id }),
            Effect::Stream { op, file_id } => self.send(Job::Stream { op, file_id }),
            Effect::FetchLatestRelease => self.send(Job::LatestRelease),
            Effect::Notify(notification) => self.notifier.notify(&notification),
            Effect::SetBadge(percent) => self.badge_percent = percent,
            Effect::OpenUrl(url) => {
                if let Err(error) = self.opener.open(&url) {
                    eprintln!("could not open {url}: {error}");
                }
            }
            Effect::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
        }
    }

    fn store_token(&mut self, token: Token) {
        if let Err(error) = self.secrets.save(&token) {
            eprintln!("could not save the sign-in to the secret store: {error}");
        }
        self.send(Job::SetToken(Some(token)));
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
        while let Ok(message) = self.from_worker.try_recv() {
            match message {
                WorkerMsg::Core(event) => self.dispatch(ctx, event),
                WorkerMsg::Folders { parent, result } => self.ui.folder_listed(parent, result),
            }
        }
        while let Ok(raw) = self.intake_rx.try_recv() {
            self.intake(ctx, &raw);
        }
        while let Ok(file_id) = self.click_rx.try_recv() {
            self.dispatch(ctx, Event::NotificationClicked { file_id });
        }
        self.dispatch(ctx, Event::Tick);
    }

    /// Resize the window and keep its title current.
    fn sync_window(&mut self, ctx: &egui::Context) {
        let height = window_height(self.state.transfers_expanded, self.state.is_signed_in());
        if (height - self.last_height).abs() > f32::EPSILON {
            self.last_height = height;
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(egui::vec2(ui::WIDTH, height)));
        }
        // No Dock badge off macOS: show progress in the title instead.
        let progress = if cfg!(target_os = "macos") {
            None
        } else {
            self.badge_percent
        };
        let wanted = title(self.version, progress);
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
