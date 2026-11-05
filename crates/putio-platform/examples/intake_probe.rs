//! Manual check for macOS intake, notifications and the Dock badge.
//!
//! Bundle this example as an `.app` that registers the `magnet` scheme and the `torrent` extension (see
//! `tools/probe-macos-intake.sh`), then launch it with `open`. Everything it receives is appended to
//! `/tmp/putio-adder-probe.log`.

#[cfg(target_os = "macos")]
fn main() {
    use std::io::Write;
    use std::sync::Arc;
    use std::time::Duration;

    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use putio_platform::notify::{Notification, system_notifier};

    const LOG: &str = "/tmp/putio-adder-probe.log";
    fn log(line: &str) {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(LOG)
        {
            let _ = writeln!(file, "{line}");
        }
    }

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let (tx, rx) = std::sync::mpsc::channel();
    log(&format!(
        "start installed={}",
        putio_platform::macos_events::install(tx)
    ));

    std::thread::spawn(move || {
        while let Ok(text) = rx.recv() {
            let parsed = putio_core::intake::Intake::parse(&text);
            log(&format!(
                "received {text:?} -> {}",
                match &parsed {
                    Ok(item) => format!("ok: {}", item.display_name()),
                    Err(error) => format!("rejected: {error}"),
                }
            ));
        }
    });

    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

    let notifier = system_notifier(Arc::new(|file_id| {
        log(&format!("notification clicked: {file_id:?}"));
    }));
    notifier.notify(&Notification {
        title: "Download finished".into(),
        body: "“Probe” finished downloading".into(),
        file_id: Some(42),
    });
    log(&format!(
        "badge set: {}",
        putio_platform::badge::set_dock_badge(Some(42))
    ));

    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(10));
        log("exit");
        std::process::exit(0);
    });
    app.run();
}

#[cfg(not(target_os = "macos"))]
fn main() {}
