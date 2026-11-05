//! Where the app keeps its files. Pure functions over an environment lookup, so they are testable
//! for every OS on any machine.

use std::path::PathBuf;

/// Directory name used under the OS config / runtime directories.
pub const APP_DIR: &str = "putio-adder";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Windows,
    Linux,
}

impl Os {
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// An environment variable that must be an absolute path to be trusted.
fn absolute(env: &dyn Fn(&str) -> Option<String>, name: &str) -> Option<PathBuf> {
    let value = env(name)?;
    let path = PathBuf::from(value);
    (path.is_absolute() && !path.as_os_str().is_empty()).then_some(path)
}

/// Where `settings.json` lives.
///
/// * macOS: `~/Library/Application Support/putio-adder`
/// * Windows: `%APPDATA%\putio-adder`
/// * Linux: `$XDG_CONFIG_HOME/putio-adder`, else `~/.config/putio-adder`
#[must_use]
pub fn config_dir(os: Os, env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let base = match os {
        Os::MacOs => absolute(env, "HOME")?
            .join("Library")
            .join("Application Support"),
        Os::Windows => absolute(env, "APPDATA")?,
        Os::Linux => absolute(env, "XDG_CONFIG_HOME")
            .or_else(|| absolute(env, "HOME").map(|home| home.join(".config")))?,
    };
    Some(base.join(APP_DIR))
}

/// A per-user directory for the IPC socket (Unix only; Windows uses a named pipe).
///
/// The directory must be private to the user. `XDG_RUNTIME_DIR` (Linux) and `TMPDIR` (macOS) already
/// are; the fallback is a subdirectory of the config directory. The IPC layer still verifies the
/// permissions before trusting it.
#[must_use]
pub fn runtime_dir(os: Os, env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let base = match os {
        Os::Windows => return None,
        Os::MacOs => absolute(env, "TMPDIR"),
        Os::Linux => absolute(env, "XDG_RUNTIME_DIR"),
    };
    base.map(|dir| dir.join(APP_DIR))
        .or_else(|| config_dir(os, env))
}

/// Production environment lookup.
#[must_use]
pub fn process_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        let map: HashMap<&str, &str> = pairs.iter().copied().collect();
        move |name| map.get(name).map(|v| (*v).to_owned())
    }

    #[test]
    fn config_dirs_per_os() {
        let e = env(&[
            ("HOME", "/Users/me"),
            ("APPDATA", "C:\\Users\\me\\AppData\\Roaming"),
        ]);
        assert_eq!(
            config_dir(Os::MacOs, &e),
            Some(PathBuf::from(
                "/Users/me/Library/Application Support/putio-adder"
            ))
        );
        assert_eq!(
            config_dir(Os::Linux, &e),
            Some(PathBuf::from("/Users/me/.config/putio-adder"))
        );
        let xdg = env(&[("HOME", "/home/me"), ("XDG_CONFIG_HOME", "/home/me/cfg")]);
        assert_eq!(
            config_dir(Os::Linux, &xdg),
            Some(PathBuf::from("/home/me/cfg/putio-adder"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_config_dir_uses_appdata() {
        let e = env(&[("APPDATA", "C:\\Users\\me\\AppData\\Roaming")]);
        assert_eq!(
            config_dir(Os::Windows, &e),
            Some(PathBuf::from(
                "C:\\Users\\me\\AppData\\Roaming\\putio-adder"
            ))
        );
    }

    #[test]
    fn relative_or_missing_variables_are_not_trusted() {
        assert_eq!(
            config_dir(Os::Linux, &env(&[("HOME", "relative/home")])),
            None
        );
        assert_eq!(config_dir(Os::Linux, &env(&[])), None);
        assert_eq!(config_dir(Os::MacOs, &env(&[("HOME", "")])), None);
        // A relative XDG_CONFIG_HOME is ignored in favour of HOME.
        let e = env(&[("HOME", "/home/me"), ("XDG_CONFIG_HOME", "evil")]);
        assert_eq!(
            config_dir(Os::Linux, &e),
            Some(PathBuf::from("/home/me/.config/putio-adder"))
        );
    }

    #[test]
    fn runtime_dirs() {
        let e = env(&[
            ("HOME", "/home/me"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
            ("TMPDIR", "/var/folders/ab/T/"),
        ]);
        assert_eq!(
            runtime_dir(Os::Linux, &e),
            Some(PathBuf::from("/run/user/1000/putio-adder"))
        );
        assert_eq!(
            runtime_dir(Os::MacOs, &e),
            Some(PathBuf::from("/var/folders/ab/T/putio-adder"))
        );
        assert_eq!(runtime_dir(Os::Windows, &e), None);
        // Falls back to the config dir.
        let bare = env(&[("HOME", "/home/me")]);
        assert_eq!(
            runtime_dir(Os::Linux, &bare),
            Some(PathBuf::from("/home/me/.config/putio-adder"))
        );
    }
}
