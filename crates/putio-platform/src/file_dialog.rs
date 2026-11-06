//! The system's "choose files" dialog.

use std::path::PathBuf;

/// Ask the user for files, with `title` on the dialog. Empty when they cancel. Blocks until the dialog is
/// closed, so it must be called from the thread that runs the window (macOS insists on the main thread).
#[must_use]
pub fn pick_files(title: &str) -> Vec<PathBuf> {
    rfd::FileDialog::new()
        .set_title(title)
        .pick_files()
        .unwrap_or_default()
}
