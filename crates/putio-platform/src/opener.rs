//! Opening a web page in the user's browser.
//!
//! The app only ever opens a handful of fixed `https` pages. The URLs are built by the core from
//! numeric ids and constants, and this module checks them *again* before handing them to the OS, so
//! a bug elsewhere cannot turn "open a link" into "open an arbitrary file or program".

use std::process::{Command, Stdio};

use crate::paths::Os;

/// Prefixes the app may open. Each ends at a path boundary (`/`), so `https://put.io.evil.example/`
/// and `https://github.com/nicoSWD/putio-adder-rs-evil` do not match.
const ALLOWED_PREFIXES: [&str; 2] = [
    "https://put.io/",
    "https://github.com/nicoSWD/putio-adder-rs/",
];
/// The project page itself, without a trailing slash.
const PROJECT_PAGE: &str = "https://github.com/nicoSWD/putio-adder-rs";

#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("refusing to open a URL that is not on the allow-list")]
    NotAllowed,
    #[error("could not start the system opener: {0}")]
    Spawn(#[from] std::io::Error),
}

/// Whether `url` may be opened: an allow-listed `https` prefix and nothing that a shell, a URL
/// handler or a command line could treat specially.
#[must_use]
pub fn is_allowed(url: &str) -> bool {
    let clean = url.bytes().all(|b| {
        b.is_ascii_graphic()
            && !matches!(
                b,
                b'"' | b'\'' | b'<' | b'>' | b'`' | b'\\' | b'^' | b'|' | b'{' | b'}'
            )
    });
    let on_list = url == PROJECT_PAGE
        || ALLOWED_PREFIXES
            .iter()
            .any(|prefix| url.starts_with(prefix));
    clean && url.len() <= 2048 && on_list && !url.contains("..")
}

pub trait UrlOpener: Send + Sync {
    fn open(&self, url: &str) -> Result<(), OpenError>;
}

/// The program and arguments used to open `url` on `os`. Never a shell.
#[must_use]
pub fn command_for(os: Os, url: &str) -> (&'static str, Vec<String>) {
    match os {
        Os::MacOs => ("/usr/bin/open", vec![url.to_owned()]),
        Os::Windows => ("explorer.exe", vec![url.to_owned()]),
        Os::Linux => ("xdg-open", vec![url.to_owned()]),
    }
}

#[derive(Debug, Default)]
pub struct SystemOpener;

impl UrlOpener for SystemOpener {
    fn open(&self, url: &str) -> Result<(), OpenError> {
        if !is_allowed(url) {
            return Err(OpenError::NotAllowed);
        }
        let (program, args) = command_for(Os::current(), url);
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(())
    }
}

/// Records what would have been opened. For tests.
#[derive(Debug, Default)]
pub struct RecordingOpener {
    opened: std::sync::Mutex<Vec<String>>,
}

impl RecordingOpener {
    #[must_use]
    pub fn opened(&self) -> Vec<String> {
        self.opened.lock().map(|o| o.clone()).unwrap_or_default()
    }
}

impl UrlOpener for RecordingOpener {
    fn open(&self, url: &str) -> Result<(), OpenError> {
        if !is_allowed(url) {
            return Err(OpenError::NotAllowed);
        }
        if let Ok(mut opened) = self.opened.lock() {
            opened.push(url.to_owned());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_the_pages_the_app_opens() {
        for url in [
            "https://put.io/files/123",
            "https://put.io/transfers",
            "https://put.io/link",
            "https://github.com/nicoSWD/putio-adder-rs",
            "https://github.com/nicoSWD/putio-adder-rs/releases/tag/v1.2.3",
        ] {
            assert!(is_allowed(url), "{url}");
        }
    }

    #[test]
    fn refuses_everything_else() {
        for url in [
            "http://put.io/files/1",
            "https://put.io.evil.example/",
            "https://put.io@evil.example/",
            "https://evilput.io/",
            "https://github.com/nicoSWD/putio-adder-rs-evil",
            "https://github.com/other/repo",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://put.io/\" --flag",
            "https://put.io/a b",
            "https://put.io/a\nb",
            "https://put.io/a'b",
            "https://put.io/a`b",
            "https://put.io/../../x",
            "",
            "-a/Applications/Calculator.app",
            "https://put.io/\u{e9}",
        ] {
            assert!(!is_allowed(url), "{url:?}");
        }
        assert!(!is_allowed(&format!("https://put.io/{}", "a".repeat(2100))));
    }

    #[test]
    fn user_info_tricks_are_refused() {
        // `https://put.io/` is a prefix of this, but the authority is still put.io; make sure the
        // allow-list cannot be passed by smuggling an authority after the host.
        assert!(!is_allowed("https://put.io.evil.example/files/1"));
        assert!(!is_allowed("https://put.io:443@evil.example/files/1"));
    }

    #[test]
    fn commands_never_go_through_a_shell() {
        let (program, args) = command_for(Os::MacOs, "https://put.io/link");
        assert_eq!(
            (program, args.as_slice()),
            (
                "/usr/bin/open",
                ["https://put.io/link".to_owned()].as_slice()
            )
        );
        assert_eq!(command_for(Os::Linux, "https://put.io/link").0, "xdg-open");
        assert_eq!(
            command_for(Os::Windows, "https://put.io/link").0,
            "explorer.exe"
        );
    }

    #[test]
    fn system_opener_refuses_before_spawning() {
        assert!(matches!(
            SystemOpener.open("file:///etc/passwd"),
            Err(OpenError::NotAllowed)
        ));
    }

    #[test]
    fn recording_opener_applies_the_same_rule() {
        let opener = RecordingOpener::default();
        opener.open("https://put.io/link").unwrap();
        assert!(opener.open("https://evil.example/").is_err());
        assert_eq!(opener.opened(), ["https://put.io/link"]);
    }
}
