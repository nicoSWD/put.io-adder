//! User preferences: a small, versioned, forgiving JSON document.
//!
//! This module only converts to and from text. Reading and (atomically) writing the file is the
//! shell's job. Secrets never live here; the token is in the OS secret store.

use serde::{Deserialize, Serialize};

/// Bumped when the meaning of a field changes. Older documents are read as-is (missing fields take
/// their defaults); a newer document is read best-effort and rewritten with this version.
pub const CURRENT_VERSION: u32 = 1;
const MAX_FOLDER_NAME: usize = 255;

/// Which colour scheme to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeChoice {
    /// Follow the operating system.
    #[default]
    System,
    Light,
    Dark,
}

/// Which list the main window shows under the header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tab {
    Feeds,
    Files,
    /// Also what a preferences file from a newer version with another tab is read as.
    #[default]
    #[serde(other)]
    Transfers,
}

/// The window's size when the transfers list is open, remembered between runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

impl WindowSize {
    /// The layout is designed for this width: the window cannot be made narrower.
    pub const MIN_WIDTH: u32 = 560;
    pub const MAX_WIDTH: u32 = 1400;
    /// Tall enough for the whole "..." menu (see `MIN_LIST_HEIGHT` in the window code, which a test ties to it).
    pub const MIN_HEIGHT: u32 = 410;
    pub const MAX_HEIGHT: u32 = 3000;

    /// The same size, brought into the range the window supports. A preferences file is untrusted
    /// input: `0` or `4294967295` must not make a window nobody can use.
    #[must_use]
    pub fn clamped(self) -> Self {
        Self {
            width: self.width.clamp(Self::MIN_WIDTH, Self::MAX_WIDTH),
            height: self.height.clamp(Self::MIN_HEIGHT, Self::MAX_HEIGHT),
        }
    }
}

/// A put.io folder chosen as the destination for new transfers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[allow(clippy::struct_excessive_bools)] // independent on/off preferences
pub struct Settings {
    pub version: u32,
    /// Quit 5 s after a magnet link was added.
    pub close_after_magnet: bool,
    /// Quit 5 s after a `.torrent` was uploaded.
    pub close_after_torrent: bool,
    /// Move a `.torrent` file to the Trash once put.io has taken it.
    pub trash_torrent_after_upload: bool,
    pub check_for_updates: bool,
    /// Offer to become the default app for magnet links and `.torrent` files (until the user says
    /// "don't ask again").
    pub ask_default_handler: bool,
    /// `None` means put.io's own default folder.
    pub default_folder: Option<Folder>,
    /// Whether the list under the header was open when the app last ran.
    pub transfers_expanded: bool,
    /// The list that was showing when the app last ran.
    pub tab: Tab,
    pub theme: ThemeChoice,
    /// How big the user made the window (with the list open). `None` until they resize it.
    pub window_size: Option<WindowSize>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            close_after_magnet: false,
            close_after_torrent: false,
            trash_torrent_after_upload: false,
            check_for_updates: true,
            ask_default_handler: true,
            default_folder: None,
            transfers_expanded: false,
            tab: Tab::Transfers,
            theme: ThemeChoice::System,
            window_size: None,
        }
    }
}

impl Settings {
    /// Parse a settings document. Anything unparsable yields the defaults (a corrupt preferences file
    /// must never stop the app from starting), and loaded values are sanitised.
    #[must_use]
    pub fn from_json_or_default(text: Option<&str>) -> Self {
        text.and_then(Self::parse).unwrap_or_default()
    }

    /// Parse a settings document (loaded values are sanitised); `None` when it is not one.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        serde_json::from_str::<Self>(text).ok().map(Self::sanitised)
    }

    /// Pretty JSON for writing to disk.
    #[must_use]
    pub fn to_json(&self) -> String {
        // Serialising plain data cannot fail; fall back to an empty document rather than panic.
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }

    fn sanitised(mut self) -> Self {
        self.version = CURRENT_VERSION;
        self.window_size = self.window_size.map(WindowSize::clamped);
        if let Some(folder) = &mut self.default_folder {
            let name: String = folder
                .name
                .chars()
                .filter(|c| !c.is_control())
                .take(MAX_FOLDER_NAME)
                .collect();
            folder.name = name;
        }
        if self
            .default_folder
            .as_ref()
            .is_some_and(|folder| folder.id < 0)
        {
            self.default_folder = None;
        }
        self
    }

    /// The `save_parent_id` / `parent_id` to send, if a folder was chosen.
    #[must_use]
    pub fn default_folder_id(&self) -> Option<i64> {
        self.default_folder.as_ref().map(|folder| folder.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_that_is_not_settings_does_not_parse() {
        assert!(Settings::parse("{ not json").is_none());
        assert!(Settings::parse("").is_none());
        assert!(
            Settings::parse(r#"{"close_after_magnet": true}"#)
                .is_some_and(|s| s.close_after_magnet)
        );
    }

    #[test]
    fn defaults() {
        let s = Settings::default();
        assert!(s.check_for_updates && !s.close_after_magnet && s.default_folder.is_none());
    }

    #[test]
    fn a_preference_that_no_longer_exists_is_ignored_when_loading() {
        let s = Settings::from_json_or_default(Some(
            r#"{"show_avatar": true, "close_after_magnet": true}"#,
        ));
        assert!(s.close_after_magnet);
    }

    #[test]
    fn round_trip() {
        let s = Settings {
            close_after_magnet: true,
            default_folder: Some(Folder {
                id: 7,
                name: "Movies".into(),
            }),
            ..Settings::default()
        };
        assert_eq!(Settings::from_json_or_default(Some(&s.to_json())), s);
    }

    #[test]
    fn the_window_size_round_trips_and_is_optional() {
        assert_eq!(Settings::default().window_size, None);
        let s = Settings {
            window_size: Some(WindowSize {
                width: 700,
                height: 800,
            }),
            ..Settings::default()
        };
        assert_eq!(Settings::from_json_or_default(Some(&s.to_json())), s);
        // A document from before the window could be resized has no such field.
        let old = Settings::from_json_or_default(Some(r#"{"close_after_magnet": true}"#));
        assert_eq!(old.window_size, None);
    }

    #[test]
    fn a_hostile_window_size_is_brought_into_range() {
        for (width, height) in [
            (0, 0),
            (1, 1),
            (u32::MAX, u32::MAX),
            (559, 359),
            (100_000, 100_000),
        ] {
            let text = format!(r#"{{"window_size": {{"width": {width}, "height": {height}}}}}"#);
            let size = Settings::from_json_or_default(Some(&text))
                .window_size
                .unwrap();
            assert!(
                (WindowSize::MIN_WIDTH..=WindowSize::MAX_WIDTH).contains(&size.width),
                "{width}"
            );
            assert!(
                (WindowSize::MIN_HEIGHT..=WindowSize::MAX_HEIGHT).contains(&size.height),
                "{height}"
            );
        }
        let fine = WindowSize {
            width: 640,
            height: 700,
        };
        assert_eq!(fine.clamped(), fine);
        // Negative or fractional numbers are not sizes: the whole setting falls back to the defaults.
        assert_eq!(
            Settings::from_json_or_default(Some(r#"{"window_size": {"width": -5, "height": 10}}"#)),
            Settings::default()
        );
    }

    #[test]
    fn the_default_app_question_is_asked_unless_declined_for_good() {
        assert!(Settings::default().ask_default_handler);
        // A document from before the question existed: still asked.
        assert!(
            Settings::from_json_or_default(Some(r#"{"close_after_magnet": true}"#))
                .ask_default_handler
        );
        let declined = Settings {
            ask_default_handler: false,
            ..Settings::default()
        };
        assert!(!Settings::from_json_or_default(Some(&declined.to_json())).ask_default_handler);
    }

    #[test]
    fn garbage_and_missing_files_give_defaults() {
        assert_eq!(Settings::from_json_or_default(None), Settings::default());
        assert_eq!(
            Settings::from_json_or_default(Some("not json")),
            Settings::default()
        );
        assert_eq!(
            Settings::from_json_or_default(Some("[1,2]")),
            Settings::default()
        );
        assert_eq!(
            Settings::from_json_or_default(Some("")),
            Settings::default()
        );
    }

    #[test]
    fn old_and_partial_documents_fill_in_defaults() {
        let s = Settings::from_json_or_default(Some(r#"{"close_after_torrent": true}"#));
        assert!(s.close_after_torrent);
        assert!(s.check_for_updates);
        assert_eq!(s.version, CURRENT_VERSION);
    }

    #[test]
    fn the_open_tab_is_remembered_and_an_unknown_one_is_read_as_the_default() {
        assert_eq!(Settings::default().tab, Tab::Transfers);
        let feeds = Settings {
            tab: Tab::Feeds,
            ..Settings::default()
        };
        let json = feeds.to_json();
        assert!(json.contains(r#""tab": "feeds""#), "{json}");
        assert_eq!(Settings::from_json_or_default(Some(&json)), feeds);
        // The Files tab is remembered too.
        let files = Settings {
            tab: Tab::Files,
            ..Settings::default()
        };
        assert_eq!(
            Settings::from_json_or_default(Some(&files.to_json())),
            files
        );
        // A tab a newer version added, and a file from before tabs existed.
        let newer = Settings::from_json_or_default(Some(
            r#"{"tab": "shares", "close_after_magnet": true}"#,
        ));
        assert_eq!(newer.tab, Tab::Transfers);
        assert!(newer.close_after_magnet, "the rest of the file is kept");
        assert_eq!(
            Settings::from_json_or_default(Some(r#"{"close_after_magnet": true}"#)).tab,
            Tab::Transfers
        );
    }

    #[test]
    fn a_torrent_stays_where_it_is_unless_the_user_says_otherwise() {
        assert!(!Settings::default().trash_torrent_after_upload);
        // A preferences file from before the setting existed.
        let old = Settings::from_json_or_default(Some(r#"{"close_after_torrent": true}"#));
        assert!(!old.trash_torrent_after_upload);
        let on = Settings {
            trash_torrent_after_upload: true,
            ..Settings::default()
        };
        assert_eq!(Settings::from_json_or_default(Some(&on.to_json())), on);
        // The wrong type is not taken for "yes".
        assert_eq!(
            Settings::from_json_or_default(Some(r#"{"trash_torrent_after_upload": "yes"}"#)),
            Settings::default()
        );
    }

    #[test]
    fn newer_documents_with_unknown_fields_still_load() {
        let s = Settings::from_json_or_default(Some(
            r#"{"version": 99, "close_after_magnet": true, "hologram": 1}"#,
        ));
        assert!(s.close_after_magnet);
        assert_eq!(s.version, CURRENT_VERSION);
    }

    #[test]
    fn wrong_types_fall_back_to_defaults_instead_of_crashing() {
        assert_eq!(
            Settings::from_json_or_default(Some(r#"{"close_after_magnet": "yes"}"#)),
            Settings::default()
        );
    }

    #[test]
    fn folder_is_sanitised() {
        let s = Settings::from_json_or_default(Some(
            r#"{"default_folder": {"id": 3, "name": "a\u0007b\nc"}}"#,
        ));
        assert_eq!(s.default_folder.unwrap().name, "abc");
        let negative =
            Settings::from_json_or_default(Some(r#"{"default_folder": {"id": -1, "name": "x"}}"#));
        assert_eq!(negative.default_folder, None);
        let long = format!(
            r#"{{"default_folder": {{"id": 3, "name": "{}"}}}}"#,
            "n".repeat(1000)
        );
        assert_eq!(
            Settings::from_json_or_default(Some(&long))
                .default_folder
                .unwrap()
                .name
                .len(),
            MAX_FOLDER_NAME
        );
    }

    #[test]
    fn theme_defaults_to_the_system_and_round_trips() {
        assert_eq!(Settings::default().theme, ThemeChoice::System);
        for theme in [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark] {
            let settings = Settings {
                theme,
                ..Settings::default()
            };
            assert_eq!(
                Settings::from_json_or_default(Some(&settings.to_json())).theme,
                theme
            );
        }
        assert!(
            Settings::default()
                .to_json()
                .contains("\"theme\": \"system\"")
        );
    }

    #[test]
    fn old_files_without_a_theme_follow_the_system() {
        let s = Settings::from_json_or_default(Some(r#"{"close_after_magnet": true}"#));
        assert_eq!(s.theme, ThemeChoice::System);
    }

    #[test]
    fn an_unknown_theme_value_falls_back_to_the_defaults_instead_of_failing() {
        // Same rule as every other bad value: a settings file must never stop the app from starting.
        assert_eq!(
            Settings::from_json_or_default(Some(r#"{"theme": "neon"}"#)),
            Settings::default()
        );
    }

    #[test]
    fn default_folder_id() {
        let mut s = Settings::default();
        assert_eq!(s.default_folder_id(), None);
        s.default_folder = Some(Folder {
            id: 0,
            name: "Your Files".into(),
        });
        assert_eq!(s.default_folder_id(), Some(0));
    }
}
