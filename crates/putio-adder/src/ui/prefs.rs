//! Preferences window and the default-folder picker.

use eframe::egui::{self, Align2, RichText};
use putio_api::client::FileListing;
use putio_api::{ApiError, File};
use putio_core::i18n::{Locale, Msg};
use putio_core::settings::{Folder, ThemeChoice};
use putio_core::state::{Event, SettingsChange};

use super::theme;
use crate::app::App;
use crate::worker::{Job, Workers};

/// Browsing put.io folders to choose the destination of new transfers.
#[derive(Debug, Default)]
pub(crate) struct FolderPicker {
    open: bool,
    /// Folders from the root down to the one being shown (the root has id 0).
    trail: Vec<(i64, String)>,
    folders: Vec<File>,
    loading: bool,
    failed: bool,
}

impl FolderPicker {
    pub(crate) fn listed(&mut self, parent: i64, result: Result<FileListing, ApiError>) {
        // Ignore an answer for a folder we have already left.
        if self.trail.last().map(|(id, _)| *id) != Some(parent) {
            return;
        }
        self.loading = false;
        if let Ok(listing) = result {
            self.failed = false;
            self.folders = listing.files;
            self.folders
                .sort_by_cached_key(|folder| folder.name.to_lowercase());
        } else {
            self.failed = true;
            self.folders.clear();
        }
    }

    fn enter(&mut self, workers: &Workers, id: i64, name: String) {
        self.trail.push((id, name));
        self.loading = true;
        workers.send(Job::ListFolders { parent: id });
    }
}

pub(crate) fn show(ctx: &egui::Context, app: &mut App) {
    if !app.ui.prefs_open {
        return;
    }
    let locale = app.state.locale;
    let mut open = true;
    egui::Window::new(Msg::PrefsTitle.text(locale))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.label(Msg::PrefsAutoCloseAfter.text(locale));
            let settings = app.state.settings.clone();
            let mut change = None;
            ui.indent("autoclose", |ui| {
                let mut magnet = settings.close_after_magnet;
                if ui
                    .checkbox(&mut magnet, Msg::PrefsAddingMagnet.text(locale))
                    .changed()
                {
                    change = Some(SettingsChange::CloseAfterMagnet(magnet));
                }
                let mut torrent = settings.close_after_torrent;
                if ui
                    .checkbox(&mut torrent, Msg::PrefsUploadingTorrent.text(locale))
                    .changed()
                {
                    change = Some(SettingsChange::CloseAfterTorrent(torrent));
                }
            });
            ui.add_space(6.0);
            let mut updates = settings.check_for_updates;
            if ui
                .checkbox(&mut updates, Msg::PrefsCheckUpdates.text(locale))
                .changed()
            {
                change = Some(SettingsChange::CheckForUpdates(updates));
            }
            ui.add_space(6.0);
            if let Some(theme_change) = appearance_row(ui, settings.theme, locale) {
                change = Some(theme_change);
            }
            ui.add_space(6.0);
            let mut avatar = settings.show_avatar;
            if ui
                .checkbox(&mut avatar, Msg::PrefsShowAvatar.text(locale))
                .changed()
            {
                change = Some(SettingsChange::ShowAvatar(avatar));
            }
            ui.label(
                RichText::new(Msg::PrefsShowAvatarHint.text(locale))
                    .small()
                    .color(theme::pal().muted),
            );

            ui.add_space(8.0);
            ui.separator();
            ui.label(Msg::PrefsDefaultFolder.text(locale));
            ui.horizontal(|ui| {
                let current = settings.default_folder.as_ref().map_or_else(
                    || Msg::PrefsDefaultFolderNone.text(locale),
                    |folder| folder.name.clone(),
                );
                ui.label(RichText::new(current).strong());
                if app.state.is_signed_in()
                    && ui.button(Msg::PrefsChooseFolder.text(locale)).clicked()
                {
                    let workers = app.workers();
                    let picker = &mut app.ui.folders;
                    picker.open = true;
                    picker.trail.clear();
                    picker.folders.clear();
                    picker.failed = false;
                    picker.enter(&workers, 0, "Your Files".into());
                }
                if settings.default_folder.is_some()
                    && ui.button(Msg::PrefsClearFolder.text(locale)).clicked()
                {
                    change = Some(SettingsChange::DefaultFolder(None));
                }
            });

            ui.add_space(8.0);
            if theme::primary_button(ui, Msg::PrefsClose.text(locale)).clicked() {
                app.ui.prefs_open = false;
            }
            if let Some(change) = change {
                app.dispatch(ctx, Event::SettingsChanged(change));
            }
        });
    if !open {
        app.ui.prefs_open = false;
    }
    folder_window(ctx, app);
}

/// The System / Light / Dark choice. Returns the change when the user picks a different one.
fn appearance_row(
    ui: &mut egui::Ui,
    current: ThemeChoice,
    locale: Locale,
) -> Option<SettingsChange> {
    let mut picked = None;
    ui.label(Msg::PrefsAppearance.text(locale));
    ui.horizontal(|ui| {
        for (choice, label) in [
            (ThemeChoice::System, Msg::ThemeSystem),
            (ThemeChoice::Light, Msg::ThemeLight),
            (ThemeChoice::Dark, Msg::ThemeDark),
        ] {
            if ui
                .selectable_label(current == choice, label.text(locale))
                .clicked()
                && current != choice
            {
                picked = Some(SettingsChange::Theme(choice));
            }
        }
    });
    picked
}

fn folder_window(ctx: &egui::Context, app: &mut App) {
    if !app.ui.folders.open {
        return;
    }
    let locale = app.state.locale;
    let mut open = true;
    let mut chosen: Option<Folder> = None;
    let mut enter: Option<(i64, String)> = None;
    let mut up = false;
    egui::Window::new(Msg::PrefsDefaultFolder.text(locale))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 20.0))
        .show(ctx, |ui| {
            let picker = &app.ui.folders;
            let path: Vec<&str> = picker.trail.iter().map(|(_, name)| name.as_str()).collect();
            ui.label(RichText::new(path.join(" / ")).strong());
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(150.0)
                .show(ui, |ui| {
                    if picker.trail.len() > 1 && ui.button("⬆ ..").clicked() {
                        up = true;
                    }
                    if picker.loading {
                        ui.spinner();
                    }
                    if picker.failed {
                        ui.colored_label(theme::pal().error, Msg::TransfersFailed.text(locale));
                    }
                    for folder in &picker.folders {
                        if ui.button(format!("📁 {}", folder.name)).clicked() {
                            enter = Some((folder.id, folder.name.clone()));
                        }
                    }
                });
            ui.separator();
            if let Some((id, name)) = picker.trail.last()
                && ui.button(format!("Use “{name}”")).clicked()
            {
                chosen = Some(Folder {
                    id: *id,
                    name: name.clone(),
                });
            }
        });

    let workers = app.workers();
    if let Some((id, name)) = enter {
        app.ui.folders.enter(&workers, id, name);
    }
    if up {
        let picker = &mut app.ui.folders;
        picker.trail.pop();
        if let Some((id, _)) = picker.trail.last() {
            picker.loading = true;
            workers.send(Job::ListFolders { parent: *id });
        }
    }
    if let Some(folder) = chosen {
        app.ui.folders.open = false;
        app.dispatch(
            ctx,
            Event::SettingsChanged(SettingsChange::DefaultFolder(Some(folder))),
        );
    }
    if !open {
        app.ui.folders.open = false;
    }
}
