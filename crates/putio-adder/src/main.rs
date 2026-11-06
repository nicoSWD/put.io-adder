//! put.io adder: send magnet links and `.torrent` files to your put.io account.

mod app;
mod app_icon;
#[cfg(debug_assertions)]
mod demo;
mod keychain;
mod ui;
mod window;
mod worker;

use std::sync::mpsc;
use std::sync::{Arc, OnceLock};

use putio_core::i18n::Locale;
use putio_core::settings::Settings;
use putio_core::state::SettingsProblem;
use putio_platform::ipc::{self, Endpoint, Instance};
use putio_platform::notify::{self, ClickHandler};
use putio_platform::paths;
use putio_platform::secret::{KeyringStore, MemoryStore, SecretStore};
use putio_platform::settings_store::SettingsFile;
use putio_platform::update;

const VERSION: &str = env!("CARGO_PKG_VERSION");
/// How long a process started by an update waits for the old version to quit.
const RELAUNCH_WAIT: std::time::Duration = std::time::Duration::from_secs(60);

#[allow(clippy::too_many_lines)] // the start-up sequence reads best top to bottom
/// Where the preferences live, if anywhere. A demo run never writes the real preferences.
fn settings_file(demo: bool) -> Option<SettingsFile> {
    if demo {
        return None;
    }
    paths::config_dir(paths::Os::current(), &paths::process_env).map(SettingsFile::new)
}

/// The saved preferences, and what is wrong with them if anything is. No file yet is not a problem (first
/// run); a file that cannot be read or is not valid means the defaults are used, and the user is told. No
/// place to keep preferences at all (outside a demo run) means changes cannot last.
fn load_settings(file: Option<&SettingsFile>, demo: bool) -> (Settings, Option<SettingsProblem>) {
    let Some(file) = file else {
        return (
            Settings::default(),
            (!demo).then_some(SettingsProblem::NotSaved),
        );
    };
    match file.load() {
        Ok(None) => (Settings::default(), None),
        Ok(Some(text)) => Settings::parse(&text).map_or_else(
            || (Settings::default(), Some(SettingsProblem::Unreadable)),
            |settings| (settings, None),
        ),
        Err(error) => {
            eprintln!("could not read settings: {error}");
            (Settings::default(), Some(SettingsProblem::Unreadable))
        }
    }
}

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
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let relaunched = take_relaunch_flag(&mut args);

    // Debug builds only: a demo run is isolated from the real app. It uses sample data and touches no
    // keychain, no settings file and no single-instance socket (so it can run next to the real app).
    let demo = app::demo_requested().is_some();

    // Single instance: hand our arguments to a running instance and exit, or become the primary one.
    let ipc_server = if demo {
        None
    } else {
        let started = Endpoint::for_current_user().and_then(|endpoint| {
            if relaunched {
                ipc::start_or_wait(&endpoint, &args, &intake_tx, wake_up, RELAUNCH_WAIT)
            } else {
                ipc::start_or_forward(&endpoint, &args, intake_tx.clone(), wake_up)
            }
        });
        match started {
            Ok(Instance::Forwarded) => return Ok(()),
            Ok(Instance::Primary(server)) => Some(server),
            Err(error) => {
                eprintln!("single-instance forwarding is unavailable: {error}");
                None
            }
        }
    };
    for arg in args {
        let _ = intake_tx.send(arg);
    }
    if !demo {
        spawn_update_cleanup();
    }

    let settings_file = settings_file(demo);
    let (settings, settings_problem) = load_settings(settings_file.as_ref(), demo);
    let locale = sys_locale::get_locale().map_or(Locale::En, |tag| Locale::from_tag(&tag));
    // Debug builds only: `PUTIO_ADDER_LOCALE=de` (or `es`) to look at a translation.
    #[cfg(debug_assertions)]
    let locale = std::env::var("PUTIO_ADDER_LOCALE").map_or(locale, |tag| Locale::from_tag(&tag));

    let (secrets, secrets_persist): (Arc<dyn SecretStore>, bool) = if demo {
        (Arc::new(MemoryStore::new()), true) // a demo run never touches the OS keychain, so it never prompts
    } else {
        open_keychain()
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
        settings_problem,
        locale,
        secrets,
        secrets_persist,
        notifier,
        intake_rx,
        click_rx,
        wake,
        ipc_server,
        version: VERSION,
    };
    let window = window::Window::new(&startup.settings, app::initial_mode(&startup.settings));
    let (min_size, max_size) = window.initial_limits();
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size(window.initial_size())
        .with_min_inner_size(min_size)
        .with_max_inner_size(max_size)
        .with_resizable(true)
        .with_maximize_button(false)
        .with_app_id("es.nico.putio-adder")
        .with_title(app::title(None));
    // Starts with the light icon; the app switches to the dark one when its theme is dark.
    if let Some(icon) = app_icon::window_icon(false) {
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

/// Remove the previous version of the app that an update left next to this one. Not before the
/// single instance is ours: on Windows and Linux the old process may have been running from it.
fn spawn_update_cleanup() {
    let _ = std::thread::Builder::new()
        .name("putio-update-cleanup".into())
        .spawn(update::cleanup_stale);
}

/// Whether an update started this process (Windows/Linux: the old version may still be quitting).
/// If so, `--relaunch-after <pid>` is taken out of `args`: it is not something for the app to act on.
fn take_relaunch_flag(args: &mut Vec<String>) -> bool {
    // `predecessor_pid` expects the program name first, as `std::env::args` gives it.
    let with_program =
        std::iter::once(std::ffi::OsString::new()).chain(args.iter().map(std::ffi::OsString::from));
    let relaunched = update::predecessor_pid(with_program).is_some();
    if relaunched && let Some(at) = args.iter().position(|arg| arg == update::RELAUNCH_FLAG) {
        args.drain(at..(at + 2).min(args.len()));
    }
    relaunched
}

/// The OS secret store, or (when there is none) a store that only lives for this session. There is no
/// file fallback: the token is never written anywhere readable. The flag says whether the sign-in
/// outlives this run, so the app can tell the user when it does not.
fn open_keychain() -> (Arc<dyn SecretStore>, bool) {
    match KeyringStore::open() {
        Ok(store) => (Arc::new(store), true),
        Err(error) => {
            eprintln!("{error}; you will have to sign in every time the app starts");
            (Arc::new(MemoryStore::new()), false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn the_relaunch_flag_and_its_pid_are_taken_out_of_the_arguments() {
        let mut list = args(&["--relaunch-after", "4242"]);
        assert!(take_relaunch_flag(&mut list));
        assert_eq!(list, Vec::<String>::new());
        let mut list = args(&[
            "a.torrent",
            "--relaunch-after",
            "7",
            "magnet:?xt=urn:btih:x",
        ]);
        assert!(take_relaunch_flag(&mut list));
        assert_eq!(list, ["a.torrent", "magnet:?xt=urn:btih:x"]);
    }

    #[test]
    fn ordinary_arguments_and_a_flag_without_a_pid_are_left_alone() {
        for original in [
            args(&[]),
            args(&["a.torrent"]),
            args(&["--relaunch-after"]),
            args(&["--relaunch-after", "abc"]),
        ] {
            let mut list = original.clone();
            assert!(!take_relaunch_flag(&mut list));
            assert_eq!(list, original);
        }
    }

    fn dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("putio-adder-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_malformed_and_unreadable_preferences_are_told_apart() {
        // First run: nothing saved, nothing wrong.
        let first = dir("settings-first");
        let file = SettingsFile::new(first.clone());
        assert_eq!(load_settings(Some(&file), false).1, None);

        // A saved file loads.
        let saved = Settings {
            close_after_magnet: true,
            ..Settings::default()
        };
        file.save(&saved.to_json()).unwrap();
        let (loaded, problem) = load_settings(Some(&file), false);
        assert!(loaded.close_after_magnet && problem.is_none());

        // Garbage: the defaults, and the user is told.
        std::fs::write(file.path(), "{ not json").unwrap();
        let (loaded, problem) = load_settings(Some(&file), false);
        assert_eq!(loaded, Settings::default());
        assert_eq!(problem, Some(SettingsProblem::Unreadable));

        // A path that cannot be read as a file (a directory in its place).
        std::fs::remove_file(file.path()).unwrap();
        std::fs::create_dir(file.path()).unwrap();
        assert_eq!(
            load_settings(Some(&file), false).1,
            Some(SettingsProblem::Unreadable)
        );
        let _ = std::fs::remove_dir_all(first);
    }

    #[test]
    fn no_place_for_preferences_is_a_problem_except_in_a_demo() {
        assert_eq!(
            load_settings(None, false).1,
            Some(SettingsProblem::NotSaved)
        );
        assert_eq!(load_settings(None, true).1, None);
    }
}
