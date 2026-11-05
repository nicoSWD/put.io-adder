//! User preferences: a small, versioned, forgiving JSON document.
//!
//! This module only converts to and from text. Reading and (atomically) writing the file is the
//! shell's job. Secrets never live here; the token is in the OS secret store.

use serde::{Deserialize, Serialize};

/// Bumped when the meaning of a field changes. Older documents are read as-is (missing fields take
/// their defaults); a newer document is read best-effort and rewritten with this version.
pub const CURRENT_VERSION: u32 = 1;
const MAX_FOLDER_NAME: usize = 255;

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
    pub check_for_updates: bool,
    /// Off by default: Gravatar needs a hash of the account e-mail.
    pub show_avatar: bool,
    /// `None` means put.io's own default folder.
    pub default_folder: Option<Folder>,
    /// Whether the transfers list was open when the app last ran.
    pub transfers_expanded: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            close_after_magnet: false,
            close_after_torrent: false,
            check_for_updates: true,
            show_avatar: false,
            default_folder: None,
            transfers_expanded: false,
        }
    }
}

impl Settings {
    /// Parse a settings document. Anything unparsable yields the defaults (a corrupt preferences file
    /// must never stop the app from starting), and loaded values are sanitised.
    #[must_use]
    pub fn from_json_or_default(text: Option<&str>) -> Self {
        let parsed = text.and_then(|text| serde_json::from_str::<Self>(text).ok());
        parsed.map_or_else(Self::default, Self::sanitised)
    }

    /// Pretty JSON for writing to disk.
    #[must_use]
    pub fn to_json(&self) -> String {
        // Serialising plain data cannot fail; fall back to an empty document rather than panic.
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }

    fn sanitised(mut self) -> Self {
        self.version = CURRENT_VERSION;
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
    fn defaults() {
        let s = Settings::default();
        assert!(
            s.check_for_updates
                && !s.show_avatar
                && !s.close_after_magnet
                && s.default_folder.is_none()
        );
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
