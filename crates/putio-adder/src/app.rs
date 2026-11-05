//! The eframe application: glue between the pure core state machine, the worker thread and the OS.

use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use eframe::egui::{self, ViewportCommand};
use putio_api::Token;
use putio_core::i18n::Locale;
use putio_core::intake::Intake;
use putio_core::settings::{Settings, ThemeChoice};
use putio_core::state::{Auth, Effect, Event, Millis, State, update};
use putio_platform::badge;
use putio_platform::ipc::Server;
use putio_platform::notify::Notifier;
use putio_platform::opener::{SystemOpener, UrlOpener};
use putio_platform::secret::SecretStore;
use putio_platform::settings_store::SettingsFile;

use crate::keychain::Keychain;
use crate::ui::{self, UiState};
use crate::worker::{Job, WorkerMsg, Workers};

const COLLAPSED_HEIGHT: f32 = 96.0;
const EXPANDED_HEIGHT: f32 = 600.0;
/// Height while Preferences is open: tall enough for every section without scrolling.
const PREFS_HEIGHT: f32 = 760.0;
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

/// The window is tall when the list is open, and whenever the sign-in panel is showing (the code needs room).
pub(crate) fn window_height(expanded: bool, needs_room: bool) -> f32 {
    if expanded || needs_room {
        EXPANDED_HEIGHT
    } else {
        COLLAPSED_HEIGHT
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
        Some(percent) => format!("{percent}% · Put.io Adder"),
        None => "Put.io Adder".to_owned(),
    }
}

pub(crate) struct App {
    pub(crate) state: State,
    pub(crate) ui: UiState,
    start: Instant,
    workers: Workers,
    from_worker: Receiver<WorkerMsg>,
    intake_rx: Receiver<String>,
    click_rx: Receiver<Option<i64>>,
    keychain: Keychain,
    /// The saved token, on its way from the keychain thread; `None` once it has arrived.
    pending_token: Option<Receiver<Option<Token>>>,
    settings_file: Option<SettingsFile>,
    notifier: Arc<dyn Notifier>,
    opener: SystemOpener,
    badge_percent: Option<u8>,
    /// Debug builds only: show sample data and never talk to put.io.
    #[cfg(debug_assertions)]
    demo: Option<String>,
    last_height: f32,
    last_title: String,
    last_theme: ThemeChoice,
    /// Whether the icon currently shown is the dark one (`None` until the first frame).
    last_icon_dark: Option<bool>,
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
        let (keychain, pending_token) = match Keychain::spawn(secrets, cc.egui_ctx.clone()) {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("could not start the keychain thread: {error}");
                std::process::exit(1);
            }
        };
        let demo = demo_requested();

        let height = window_height(settings.transfers_expanded, false);
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
            pending_token: demo.is_none().then_some(pending_token),
            settings_file,
            notifier,
            opener: SystemOpener,
            badge_percent: None,
            #[cfg(debug_assertions)]
            demo,
            last_height: height,
            last_title: title(None),
            last_theme: theme,
            last_icon_dark: None,
            _ipc_server: ipc_server,
        };
        // `Started` is sent when the keychain thread reports (see `pump`), except in demo mode.
        #[cfg(debug_assertions)]
        if let Some(view) = app.demo.clone() {
            app.start_demo(&cc.egui_ctx, &view);
        }
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
        self.state.transfers_expanded = true;
        // The demo drops the update check, so show its finished state instead of a spinner.
        self.state.update = putio_core::update::UpdateStatus::UpToDate;
        self.ui.prefs_open = view == "prefs" || view == "folders";
        self.ui.open_once = match view {
            "menu" | "menu-update" => Some(crate::ui::Overlay::Menu),
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
        if view == "menu-update" {
            self.state.update = putio_core::update::UpdateStatus::Available {
                version: "0.2.0".into(),
                url: "https://github.com/nicoSWD/putio-adder-rs/releases/tag/v0.2.0".into(),
            };
        }
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
    }

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
            Effect::RequestDeviceCode | Effect::PollDeviceCode { .. } | Effect::Poll
                if self.is_demo() => {}
            Effect::RequestDeviceCode => self.send(Job::DeviceCode),
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
            Effect::UploadTorrent { op, path, parent } => {
                self.send(Job::UploadTorrent { op, path, parent });
            }
            Effect::CancelTransfer { op, id } => self.send(Job::Cancel { op, id }),
            Effect::RetryTransfer { op, id } => self.send(Job::Retry { op, id }),
            Effect::CleanTransfers { op, ids } => self.send(Job::Clean { op, ids }),
            Effect::PrepareDownload { op, file_id } => {
                self.send(Job::PrepareDownload { op, file_id });
            }
            Effect::PollZip { op, zip_id } => self.send(Job::PollZip { op, zip_id }),
            // The address is a credential: on failure say that it failed, never what it was.
            Effect::OpenDownload(url) if self.is_demo() => drop(url),
            Effect::OpenDownload(url) => {
                if self.opener.open_download(&url).is_err() {
                    eprintln!("could not open the download in the browser");
                }
            }
            Effect::CopyText(text) => ctx.copy_text(text),
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
        if ctx.input_mut(|input| input.consume_shortcut(&ui::menu::preferences_shortcut())) {
            self.ui.prefs_open = true;
        }
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

    fn sync_window(&mut self, ctx: &egui::Context) {
        // The sign-in panel and the Preferences dialog need the full height, whatever the list is doing.
        let needs_room = self.ui.prefs_open
            || matches!(
                self.state.auth,
                Auth::RequestingCode | Auth::Linking(_) | Auth::SignedOut(_)
            );
        let height = if self.ui.prefs_open {
            PREFS_HEIGHT
        } else {
            window_height(self.state.transfers_expanded, needs_room)
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
        assert_eq!(title(None), "Put.io Adder");
        assert_eq!(title(Some(42)), "42% · Put.io Adder");
        assert!(!title(None).contains(char::is_numeric));
    }
}
