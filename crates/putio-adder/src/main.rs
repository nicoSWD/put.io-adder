//! put.io adder: send magnet links and `.torrent` files to your put.io account.

mod app;
#[cfg(debug_assertions)]
mod demo;
mod keychain;
mod ui;
mod worker;

use std::sync::mpsc;
use std::sync::{Arc, OnceLock};

use putio_core::i18n::Locale;
use putio_core::settings::Settings;
use putio_platform::ipc::{self, Endpoint, Instance};
use putio_platform::notify::{self, ClickHandler};
use putio_platform::paths;
use putio_platform::secret::{KeyringStore, MemoryStore, SecretStore};
use putio_platform::settings_store::SettingsFile;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> eframe::Result {
    // Magnet links and files handed to us by the OS or another launch arrive here as raw strings; the
    // app validates every one of them (`Intake::parse`) before acting.
    let (intake_tx, intake_rx) = mpsc::channel::<String>();
    let wake: Arc<OnceLock<eframe::egui::Context>> = Arc::new(OnceLock::new());
    let wake_up = {
        let wake = Arc::clone(&wake);
        move || {
            if let Some(ctx) = wake.get() {
                ctx.request_repaint();
            }
        }
    };

    // macOS: Launch Services delivers magnet links and files as Apple Events, possibly before any UI exists.
    #[cfg(target_os = "macos")]
    if !putio_platform::macos_events::install(intake_tx.clone()) {
        eprintln!("could not install the Apple Event handlers; magnet links will not be received");
    }

    // Command line (Windows/Linux, and `open --args` on macOS).
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Single instance: hand our arguments to a running instance and exit, or become the primary one.
    let ipc_server = match Endpoint::for_current_user()
        .and_then(|endpoint| ipc::start_or_forward(&endpoint, &args, intake_tx.clone(), wake_up))
    {
        Ok(Instance::Forwarded) => return Ok(()),
        Ok(Instance::Primary(server)) => {
            for arg in args {
                let _ = intake_tx.send(arg);
            }
            Some(server)
        }
        Err(error) => {
            eprintln!("single-instance forwarding is unavailable: {error}");
            for arg in args {
                let _ = intake_tx.send(arg);
            }
            None
        }
    };

    let settings_file =
        paths::config_dir(paths::Os::current(), &paths::process_env).map(SettingsFile::new);
    let settings = Settings::from_json_or_default(
        settings_file
            .as_ref()
            .and_then(|file| file.load().ok().flatten())
            .as_deref(),
    );
    let locale = sys_locale::get_locale().map_or(Locale::En, |tag| Locale::from_tag(&tag));

    let secrets: Arc<dyn SecretStore> = match KeyringStore::open() {
        Ok(store) => Arc::new(store),
        Err(error) => {
            // No file fallback: without a secret store the token only lives for this session.
            eprintln!("{error}; you will have to sign in every time the app starts");
            Arc::new(MemoryStore::new())
        }
    };

    // Created here, before the event loop, so a notification that launches the app is delivered.
    let (click_tx, click_rx) = mpsc::channel::<Option<i64>>();
    let on_click: ClickHandler = {
        let wake = Arc::clone(&wake);
        Arc::new(move |file_id| {
            let _ = click_tx.send(file_id);
            if let Some(ctx) = wake.get() {
                ctx.request_repaint();
            }
        })
    };
    let notifier = notify::system_notifier(on_click);

    let startup = app::Startup {
        settings,
        settings_file,
        locale,
        secrets,
        notifier,
        intake_rx,
        click_rx,
        wake,
        ipc_server,
        version: VERSION,
    };
    let height = app::window_height(startup.settings.transfers_expanded, false);
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([ui::WIDTH, height])
        .with_resizable(false)
        .with_maximize_button(false)
        .with_app_id("es.nico.putio-adder")
        .with_title(app::title(VERSION, None));
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "put.io adder",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, startup)))),
    )
}

/// The app icon for the window (Windows/Linux title bar, and the Dock while running unbundled on macOS).
fn window_icon() -> Option<eframe::egui::IconData> {
    const ICON_PNG: &[u8] = include_bytes!("../../../assets/icon/icon-256.png");
    let image = image::load_from_memory_with_format(ICON_PNG, image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    let (width, height) = image.dimensions();
    Some(eframe::egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}
