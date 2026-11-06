//! Files: how they are told apart, ordered and named. Pure functions over what put.io sent and what the
//! user typed; the calls and the state around them are in [`crate::state`].

use putio_api::{File, FileType};

/// Longest name put.io is given.
pub const MAX_NAME_CHARS: usize = 255;

/// What is wrong with a name the user typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameProblem {
    Empty,
    /// put.io names cannot contain a slash (it would be a path).
    HasSlash,
    TooLong,
}

/// A name as it will be sent: trimmed, and checked.
pub fn check_name(text: &str) -> Result<String, NameProblem> {
    let name = text.trim();
    if name.is_empty() || name == "." || name == ".." {
        return Err(NameProblem::Empty);
    }
    if name.contains(['/', '\\']) {
        return Err(NameProblem::HasSlash);
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(NameProblem::TooLong);
    }
    if name.chars().any(char::is_control) {
        return Err(NameProblem::Empty);
    }
    Ok(name.to_owned())
}

/// What a row looks like, which is not the same as what put.io calls it: documents are one kind, and
/// what put.io cannot tell is just a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Folder,
    Video,
    Audio,
    Image,
    Archive,
    Document,
    Other,
}

#[must_use]
pub fn kind(file: &File) -> Kind {
    match file.file_type {
        FileType::Folder => Kind::Folder,
        FileType::Video => Kind::Video,
        FileType::Audio => Kind::Audio,
        FileType::Image => Kind::Image,
        FileType::Archive => Kind::Archive,
        FileType::Pdf | FileType::Text => Kind::Document,
        FileType::File | FileType::Swf | FileType::Unknown => Kind::Other,
    }
}

#[must_use]
pub fn is_folder(file: &File) -> bool {
    file.file_type == FileType::Folder
}

/// Whether "stream" makes sense: a video or sound, or a folder that may hold some.
#[must_use]
pub fn can_stream(file: &File) -> bool {
    matches!(kind(file), Kind::Folder | Kind::Video | Kind::Audio)
}

/// Folders first, then by name (without regard to case), and by id when names are the same, so the order
/// never flickers.
pub fn sort_files(files: &mut [File]) {
    files.sort_by_cached_key(|file| (!is_folder(file), file.name.to_lowercase(), file.id));
}

/// A file's name for showing: control characters removed, `…` when there is none.
#[must_use]
pub fn display_name(file: &File) -> String {
    let name: String = file.name.chars().filter(|c| !c.is_control()).collect();
    if name.trim().is_empty() {
        "…".to_owned()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(id: i64, name: &str, kind: FileType) -> File {
        File {
            id,
            name: name.into(),
            parent_id: Some(0),
            file_type: kind,
            content_type: String::new(),
            size: 0,
            screenshot: None,
            created_at: None,
        }
    }

    #[test]
    fn names_are_trimmed_and_checked() {
        assert_eq!(check_name("  Season 1  ").as_deref(), Ok("Season 1"));
        assert_eq!(
            check_name("Ünïcode ✓ 日本語").as_deref(),
            Ok("Ünïcode ✓ 日本語")
        );
        for (bad, problem) in [
            ("", NameProblem::Empty),
            ("   ", NameProblem::Empty),
            (".", NameProblem::Empty),
            ("..", NameProblem::Empty),
            ("a/b", NameProblem::HasSlash),
            ("a\\b", NameProblem::HasSlash),
            ("tab\there", NameProblem::Empty),
        ] {
            assert_eq!(check_name(bad), Err(problem), "{bad:?}");
        }
        assert_eq!(
            check_name(&"x".repeat(MAX_NAME_CHARS)).map(|n| n.len()),
            Ok(MAX_NAME_CHARS)
        );
        assert_eq!(
            check_name(&"x".repeat(MAX_NAME_CHARS + 1)),
            Err(NameProblem::TooLong)
        );
        // Characters, not bytes: 255 two-byte letters are fine.
        assert!(check_name(&"é".repeat(MAX_NAME_CHARS)).is_ok());
    }

    #[test]
    fn folders_come_first_then_names_without_regard_to_case() {
        let mut list = vec![
            file(1, "b.mkv", FileType::Video),
            file(2, "Zebra", FileType::Folder),
            file(3, "A.mkv", FileType::Video),
            file(4, "apple", FileType::Folder),
            file(5, "a.mkv", FileType::Video),
        ];
        sort_files(&mut list);
        let names: Vec<&str> = list.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["apple", "Zebra", "A.mkv", "a.mkv", "b.mkv"]);
        // The same names keep a stable order by id.
        assert!(list[2].id < list[3].id);
    }

    #[test]
    fn every_type_has_a_kind_and_streaming_is_for_media_and_folders() {
        let kinds = [
            (FileType::Folder, Kind::Folder, true),
            (FileType::Video, Kind::Video, true),
            (FileType::Audio, Kind::Audio, true),
            (FileType::Image, Kind::Image, false),
            (FileType::Archive, Kind::Archive, false),
            (FileType::Pdf, Kind::Document, false),
            (FileType::Text, Kind::Document, false),
            (FileType::File, Kind::Other, false),
            (FileType::Swf, Kind::Other, false),
            (FileType::Unknown, Kind::Other, false),
        ];
        for (file_type, expected, streams) in kinds {
            let f = file(1, "x", file_type);
            assert_eq!(kind(&f), expected, "{file_type:?}");
            assert_eq!(can_stream(&f), streams, "{file_type:?}");
        }
    }

    #[test]
    fn a_name_for_showing_has_no_control_characters_and_is_never_blank() {
        assert_eq!(display_name(&file(1, "a\u{7}b\nc", FileType::File)), "abc");
        assert_eq!(display_name(&file(1, "  ", FileType::File)), "…");
        assert_eq!(display_name(&file(1, "", FileType::File)), "…");
    }
}
