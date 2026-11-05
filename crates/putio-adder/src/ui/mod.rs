//! Drawing. Reads the core [`State`](putio_core::state::State) and reports user actions as events.

pub(crate) mod header;
pub(crate) mod login;
pub(crate) mod prefs;
pub(crate) mod table;
pub(crate) mod theme;

use eframe::egui::{self, Frame, Margin};
use putio_api::ApiError;
use putio_api::client::FileListing;
use putio_core::state::Auth;
use putio_core::transfers::SortKey;

use crate::app::App;

pub(crate) const WIDTH: f32 = 550.0;
/// Height of the black strip.
pub(crate) const STRIP_HEIGHT: f32 = 36.0;
/// Space above the strip for the logo and account name.
pub(crate) const TOP_HEIGHT: f32 = 40.0;

/// State that only the UI cares about.
#[derive(Debug)]
pub(crate) struct UiState {
    pub(crate) sort: SortKey,
    pub(crate) ascending: bool,
    pub(crate) prefs_open: bool,
    pub(crate) folders: prefs::FolderPicker,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            sort: SortKey::Default,
            ascending: true,
            prefs_open: false,
            folders: prefs::FolderPicker::default(),
        }
    }
}

impl UiState {
    pub(crate) fn folder_listed(&mut self, parent: i64, result: Result<FileListing, ApiError>) {
        self.folders.listed(parent, result);
    }
}

pub(crate) fn show(ui: &mut egui::Ui, app: &mut App) {
    let ctx = ui.ctx().clone();
    egui::CentralPanel::default()
        .frame(Frame::new().fill(theme::BODY).inner_margin(Margin::ZERO))
        .show(ui, |ui| {
            header::show(ui, app);
            let below = ui.available_rect_before_wrap();
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(below.shrink2(egui::vec2(12.0, 8.0))),
                |ui| {
                    if matches!(app.state.auth, Auth::SignedIn) {
                        if app.state.transfers_expanded {
                            table::show(ui, app);
                        }
                    } else {
                        login::show(ui, app);
                    }
                },
            );
        });
    prefs::show(&ctx, app);
}
