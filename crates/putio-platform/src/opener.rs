//! Opening a web page in the user's browser.
//!
//! The app only ever opens a handful of fixed `https` pages. The URLs are built by the core from
//! numeric ids and constants, and this module checks them *again* before handing them to the OS, so
//! a bug elsewhere cannot turn "open a link" into "open an arbitrary file or program".

use std::process::{Command, Stdio};

use crate::paths::Os;

/// Prefixes the app may open. Each ends at a path boundary (`/`), so `https://put.io.evil.example/`
/// and `https://github.com/nicoSWD/put.io-adder-evil` do not match.
const ALLOWED_PREFIXES: [&str; 2] = [
    "https://put.io/",
    "https://github.com/nicoSWD/put.io-adder/",
];
/// The project page itself, without a trailing slash.
const PROJECT_PAGE: &str = "https://github.com/nicoSWD/put.io-adder";

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

    /// Open a download address that put.io handed out. It is not one of the fixed pages that
    /// [`UrlOpener::open`] allows, so it has its own, stricter-about-the-host check
    /// ([`putio_core::download::is_download_url`]). The address is a credential: never log it.
    fn open_download(&self, url: &str) -> Result<(), OpenError>;
}

/// The program and arguments used to open `url` on `os`. Never a shell.
///
/// On Windows the program is `explorer.exe` by absolute path under `%SystemRoot%`: a bare name would be
/// searched for in the application's own directory first.
#[must_use]
pub fn command_for(
    os: Os,
    url: &str,
    env: &dyn Fn(&str) -> Option<String>,
) -> (String, Vec<String>) {
    let program = match os {
        Os::MacOs => "/usr/bin/open".to_owned(),
        Os::Windows => {
            let root = env("SystemRoot")
                .filter(|root| crate::paths::is_absolute_for(Os::Windows, root))
                .unwrap_or_else(|| "C:\\Windows".to_owned());
            format!("{}\\explorer.exe", root.trim_end_matches('\\'))
        }
        Os::Linux => "xdg-open".to_owned(),
    };
    (program, vec![url.to_owned()])
}

#[derive(Debug, Default)]
pub struct SystemOpener;

impl SystemOpener {
    fn spawn(url: &str) -> Result<(), OpenError> {
        let (program, args) = command_for(Os::current(), url, &crate::paths::process_env);
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(())
    }
}

impl UrlOpener for SystemOpener {
    fn open(&self, url: &str) -> Result<(), OpenError> {
        if !is_allowed(url) {
            return Err(OpenError::NotAllowed);
        }
        Self::spawn(url)
    }

    fn open_download(&self, url: &str) -> Result<(), OpenError> {
        if !putio_core::download::is_download_url(url) {
            return Err(OpenError::NotAllowed);
        }
        Self::spawn(url)
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

    fn open_download(&self, url: &str) -> Result<(), OpenError> {
        if !putio_core::download::is_download_url(url) {
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
    fn a_download_address_is_checked_by_the_download_rule_not_the_page_list() {
        let opener = RecordingOpener::default();
        opener
            .open_download("https://s1.put.io/v2/files/9/download?token=a&b=c")
            .unwrap();
        assert_eq!(opener.opened().len(), 1);
        for bad in [
            "https://evil.example/x",
            "https://put.io@evil.example/",
            "http://s1.put.io/x",
            "https://s1.put.io/a b",
            "file:///etc/passwd",
        ] {
            assert!(
                matches!(opener.open_download(bad), Err(OpenError::NotAllowed)),
                "{bad}"
            );
        }
        assert_eq!(opener.opened().len(), 1, "nothing refused was recorded");
    }

    #[test]
    fn a_storage_host_is_not_a_page_the_plain_opener_will_open() {
        let opener = RecordingOpener::default();
        assert!(matches!(
            opener.open("https://s1.put.io/v2/files/9/download"),
            Err(OpenError::NotAllowed)
        ));
    }

    #[test]
    fn allows_the_pages_the_app_opens() {
        for url in [
            "https://put.io/files/123",
            "https://put.io/transfers",
            "https://put.io/link",
            "https://github.com/nicoSWD/put.io-adder",
            "https://github.com/nicoSWD/put.io-adder/releases/tag/v1.2.3",
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
            "https://github.com/nicoSWD/put.io-adder-evil",
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
        let none = |_: &str| None;
        let (program, args) = command_for(Os::MacOs, "https://put.io/link", &none);
        assert_eq!(program, "/usr/bin/open");
        assert_eq!(args, ["https://put.io/link"]);
        assert_eq!(
            command_for(Os::Linux, "https://put.io/link", &none).0,
            "xdg-open"
        );
        assert_eq!(
            command_for(Os::Windows, "https://put.io/link", &none).0,
            "C:\\Windows\\explorer.exe"
        );
        let root = |name: &str| (name == "SystemRoot").then(|| "D:\\WINNT\\".to_owned());
        assert_eq!(
            command_for(Os::Windows, "https://put.io/link", &root).0,
            "D:\\WINNT\\explorer.exe"
        );
        // A relative SystemRoot (hostile environment) is ignored.
        let relative = |name: &str| (name == "SystemRoot").then(|| "evil".to_owned());
        assert_eq!(
            command_for(Os::Windows, "https://put.io/link", &relative).0,
            "C:\\Windows\\explorer.exe"
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
