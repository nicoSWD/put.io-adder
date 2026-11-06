//! Drawing. Reads the core [`State`](putio_core::state::State) and reports user actions as events.

pub(crate) mod details;
pub(crate) mod feed_form;
pub(crate) mod feeds;
pub(crate) mod file_dialogs;
pub(crate) mod files;
pub(crate) mod handler;
pub(crate) mod header;
pub(crate) mod icons;
pub(crate) mod login;
pub(crate) mod menu;
pub(crate) mod prefs;
pub(crate) mod summary;
pub(crate) mod table;
pub(crate) mod tabs;
#[cfg(test)]
pub(crate) mod test_support;
pub(crate) mod theme;
pub(crate) mod update;
pub(crate) mod widgets;

use eframe::egui::{self, Frame, Margin};
use putio_api::ApiError;
use putio_api::client::FileListing;
use putio_core::state::Auth;
use putio_core::transfers::SortKey;

use crate::app::App;

pub(crate) const WIDTH: f32 = 560.0;
/// Height of the status row.
pub(crate) const STRIP_HEIGHT: f32 = 40.0;
/// Height of the logo row.
pub(crate) const TOP_HEIGHT: f32 = 56.0;

/// A drop-down that a preview can ask to be open from the first frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Overlay {
    Menu,
    Sort,
    /// The "⋯" menu of the transfer with this id.
    Card(i64),
    /// The "⋯" menu of the feed with this id.
    FeedCard(i64),
    /// The "⋯" menu of the file with this id.
    FileCard(i64),
}

/// State that only the UI cares about.
#[derive(Debug)]
pub(crate) struct UiState {
    pub(crate) sort: SortKey,
    pub(crate) ascending: bool,
    pub(crate) prefs_open: bool,
    /// Debug previews: a drop-down to open on the first frame.
    pub(crate) open_once: Option<Overlay>,
    /// The transfer last clicked (highlighted).
    pub(crate) selected: Option<i64>,
    /// The feed last clicked (highlighted).
    pub(crate) selected_feed: Option<i64>,
    /// The file last clicked (highlighted).
    pub(crate) selected_file: Option<i64>,
    /// What the user has typed in the search field (a search starts with Enter).
    pub(crate) search_text: String,
    /// The Files tab's naming, moving and delete cards.
    pub(crate) file_dialog: file_dialogs::FileDialog,
    /// The add / edit feed form and the delete confirmation.
    pub(crate) feed_dialog: feed_form::FeedDialog,
    pub(crate) folders: prefs::FolderPicker,
    /// Names of the folders asked about so far (`None`: put.io could not say).
    pub(crate) folder_names: std::collections::HashMap<i64, Option<String>>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            sort: SortKey::Default,
            ascending: true,
            prefs_open: false,
            open_once: None,
            selected: None,
            selected_feed: None,
            selected_file: None,
            search_text: String::new(),
            file_dialog: file_dialogs::FileDialog::default(),
            feed_dialog: feed_form::FeedDialog::default(),
            folders: prefs::FolderPicker::default(),
            folder_names: std::collections::HashMap::new(),
        }
    }
}

impl UiState {
    pub(crate) fn folder_named(&mut self, id: i64, name: Option<String>) {
        self.folder_names.insert(id, name);
    }

    pub(crate) fn folder_listed(&mut self, parent: i64, result: Result<FileListing, ApiError>) {
        self.folders.listed(parent, result);
    }
}

pub(crate) fn show(ui: &mut egui::Ui, app: &mut App) {
    let ctx = ui.ctx().clone();
    // One palette for the whole frame, whatever the theme resolves to right now.
    theme::select(&ctx);
    egui::CentralPanel::default()
        .frame(
            Frame::new()
                .fill(theme::pal().bg)
                .inner_margin(Margin::ZERO),
        )
        .show(ui, |ui| {
            header::show(ui, app);
            let below = ui.available_rect_before_wrap();
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(below.shrink2(egui::vec2(12.0, 8.0))),
                |ui| {
                    if matches!(app.state.auth, Auth::SignedIn) {
                        if app.state.transfers_expanded {
                            tabs::show(ui, app);
                        }
                    } else {
                        login::show(ui, app);
                    }
                },
            );
        });
    prefs::show(&ctx, app);
    feed_form::show(&ctx, app);
    file_dialogs::show(&ctx, app);
    details::show(&ctx, app);
    handler::show(&ctx, app);
}
