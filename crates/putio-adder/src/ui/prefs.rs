//! Preferences: a modal card with grouped settings, and the folder chooser as its second page.

use eframe::egui::{self, Align, CornerRadius, Layout, Pos2, Rect, RichText, Sense, Vec2};
use putio_api::client::FileListing;
use putio_api::{ApiError, File};
use putio_core::i18n::{Locale, Msg};
use putio_core::settings::{Folder, ThemeChoice};
use putio_core::state::{Event, SettingsChange};
use putio_core::update::UpdateStatus;

use super::theme;
use super::update;
use super::widgets;
use crate::app::App;
use crate::worker::{Job, Workers};

/// Width of the content inside the card (the window is 560 wide).
const CONTENT_WIDTH: f32 = 456.0;
const FOLDER_ROW_HEIGHT: f32 = 40.0;
/// put.io's name for the top of an account's files.
const ROOT_NAME: &str = "Your Files";

/// Browsing put.io folders to choose the destination of new transfers.
#[derive(Debug, Default)]
pub(crate) struct FolderPicker {
    /// Whether the folder page is showing (instead of the settings page).
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

    fn start(&mut self, workers: &Workers) {
        self.open = true;
        self.trail.clear();
        self.folders.clear();
        self.failed = false;
        self.enter(workers, 0, ROOT_NAME.into());
    }

    fn enter(&mut self, workers: &Workers, id: i64, name: String) {
        self.trail.push((id, name));
        self.loading = true;
        workers.send(Job::ListFolders { parent: id });
    }

    /// Go back up to the `index`-th folder of the trail.
    fn jump_to(&mut self, workers: &Workers, index: usize) {
        self.trail.truncate(index + 1);
        if let Some((id, _)) = self.trail.last() {
            self.loading = true;
            workers.send(Job::ListFolders { parent: *id });
        }
    }

    /// Debug builds only: a populated folder page for previews.
    #[cfg(debug_assertions)]
    pub(crate) fn demo() -> Self {
        let folder = |id: i64, name: &str| File {
            id,
            name: name.into(),
            parent_id: Some(0),
            file_type: putio_api::FileType::Folder,
            content_type: String::new(),
            size: 0,
            screenshot: None,
        };
        Self {
            open: true,
            trail: vec![(0, ROOT_NAME.into()), (10, "Movies".into())],
            folders: vec![
                folder(11, "2023"),
                folder(12, "2024"),
                folder(13, "Documentaries"),
                folder(14, "Kids"),
            ],
            loading: false,
            failed: false,
        }
    }
}

/// Everything the dialog can ask the app to do, collected while drawing and applied afterwards.
enum Action {
    Settings(SettingsChange),
    OpenProjectPage,
    CheckForUpdates,
    Update(update::Click),
    MakeDefaultApp,
    SignOut,
}

pub(crate) fn show(ctx: &egui::Context, app: &mut App) {
    if !app.ui.prefs_open {
        return;
    }
    let (frame, backdrop) = widgets::dialog_frame();

    let workers = app.workers();
    let locale = app.state.locale;
    let mut actions = Vec::new();
    let mut folder_chosen: Option<Folder> = None;
    let response = egui::Modal::new(egui::Id::new("preferences"))
        .backdrop_color(backdrop)
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(CONTENT_WIDTH);
            if app.ui.folders.open {
                folder_page(ui, app, &workers, locale, &mut folder_chosen)
            } else {
                settings_page(ui, app, &workers, locale, &mut actions)
            }
        });

    let close = response.inner || response.should_close();
    for action in actions {
        match action {
            Action::Settings(change) => app.dispatch(ctx, Event::SettingsChanged(change)),
            Action::OpenProjectPage => app.dispatch(ctx, Event::OpenProjectPage),
            Action::CheckForUpdates => app.dispatch(
                ctx,
                Event::CheckForUpdates {
                    user_initiated: true,
                },
            ),
            Action::Update(click) => app.dispatch(ctx, click.event()),
            Action::MakeDefaultApp => app.dispatch(ctx, Event::DefaultHandlerRequested),
            Action::SignOut => {
                app.ui.prefs_open = false;
                app.dispatch(ctx, Event::SignOutRequested);
            }
        }
    }
    if let Some(folder) = folder_chosen {
        app.ui.folders.open = false;
        app.dispatch(
            ctx,
            Event::SettingsChanged(SettingsChange::DefaultFolder(Some(folder))),
        );
    }
    if close {
        // Escape or a click outside goes back a page first, and closes from the first page.
        if app.ui.folders.open {
            app.ui.folders.open = false;
        } else {
            app.ui.prefs_open = false;
        }
    }
}

/// Title row: optional back arrow, the title, and a close button. Returns (back, close) clicks.
pub(crate) fn header(ui: &mut egui::Ui, title: &str, with_back: bool) -> (bool, bool) {
    let (mut back, mut close) = (false, false);
    ui.horizontal(|ui| {
        ui.set_height(32.0);
        if with_back {
            back = widgets::back_button(ui);
        }
        ui.label(RichText::new(title).font(theme::bold(17.0)));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            close = widgets::close_button(ui);
        });
    });
    ui.add_space(6.0);
    (back, close)
}

/// A vertical scroll area of at most `height`, given an explicit size. (A modal's own idea of the free
/// height can be tiny if it was first laid out while the window was still small, and a scroll area would
/// obey that: sizing it explicitly keeps the dialog correct whatever the window did before.)
pub(crate) fn scroll_region(ui: &mut egui::Ui, height: f32, content: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui(Vec2::new(ui.available_width(), height), |ui| {
        egui::ScrollArea::vertical()
            .max_height(height)
            .auto_shrink([false, true])
            .show(ui, content);
    });
}

/// The scrolling area under the header: tall enough for the dialog, never taller than the window.
pub(crate) fn body_height(ui: &egui::Ui) -> f32 {
    (ui.ctx().content_rect().height() - 150.0).max(160.0)
}

/// Returns `true` when the dialog should close.
fn settings_page(
    ui: &mut egui::Ui,
    app: &mut App,
    workers: &Workers,
    locale: Locale,
    actions: &mut Vec<Action>,
) -> bool {
    let (_, close) = header(ui, &Msg::PrefsTitle.text(locale), false);
    let settings = app.state.settings.clone();

    scroll_region(ui, body_height(ui), |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;

        widgets::section_title(ui, &Msg::PrefsGeneral.text(locale));
        widgets::group(ui, |ui| {
            let mut magnet = settings.close_after_magnet;
            widgets::row(
                ui,
                &Msg::PrefsQuitMagnet.text(locale),
                Some(&Msg::PrefsQuitMagnetHint.text(locale)),
                |ui| {
                    if widgets::switch(ui, &mut magnet).changed() {
                        actions.push(Action::Settings(SettingsChange::CloseAfterMagnet(magnet)));
                    }
                },
            );
            widgets::divider(ui);
            let mut torrent = settings.close_after_torrent;
            widgets::row(
                ui,
                &Msg::PrefsQuitTorrent.text(locale),
                Some(&Msg::PrefsQuitTorrentHint.text(locale)),
                |ui| {
                    if widgets::switch(ui, &mut torrent).changed() {
                        actions.push(Action::Settings(SettingsChange::CloseAfterTorrent(torrent)));
                    }
                },
            );
            widgets::divider(ui);
            default_app_row(ui, app, locale, actions);
            widgets::divider(ui);
            let mut updates = settings.check_for_updates;
            widgets::row(
                ui,
                &Msg::PrefsCheckUpdates.text(locale),
                Some(&Msg::PrefsCheckUpdatesHint.text(locale)),
                |ui| {
                    if widgets::switch(ui, &mut updates).changed() {
                        actions.push(Action::Settings(SettingsChange::CheckForUpdates(updates)));
                    }
                },
            );
        });

        widgets::section_title(ui, &Msg::PrefsAppearance.text(locale));
        widgets::group(ui, |ui| {
            ui.add_space(9.0);
            let options = [
                (ThemeChoice::System, Msg::ThemeSystem.text(locale)),
                (ThemeChoice::Light, Msg::ThemeLight.text(locale)),
                (ThemeChoice::Dark, Msg::ThemeDark.text(locale)),
            ];
            if let Some(choice) = widgets::segmented(ui, &options, settings.theme) {
                actions.push(Action::Settings(SettingsChange::Theme(choice)));
            }
            ui.add_space(9.0);
        });

        widgets::section_title(ui, &Msg::PrefsDownloads.text(locale));
        widgets::group(ui, |ui| folder_row(ui, app, workers, locale, actions));

        if let Some(account) = &app.state.account {
            widgets::section_title(ui, &Msg::PrefsAccount.text(locale));
            widgets::group(ui, |ui| {
                widgets::row(
                    ui,
                    &Msg::PrefsSignedInAs {
                        name: account.username.clone(),
                    }
                    .text(locale),
                    None,
                    |ui| {
                        if theme::secondary_button(ui, Msg::MenuSignOut.text(locale)).clicked() {
                            actions.push(Action::SignOut);
                        }
                    },
                );
            });
        }

        about(ui, app, locale, actions);
    });
    close
}

/// The "Save new transfers in" row: a title, a hint, and below them the current folder with its buttons.
fn folder_row(
    ui: &mut egui::Ui,
    app: &mut App,
    workers: &Workers,
    locale: Locale,
    actions: &mut Vec<Action>,
) {
    let p = theme::pal();
    ui.add_space(10.0);
    ui.label(RichText::new(Msg::PrefsSaveIn.text(locale)).font(theme::bold(12.5)));
    ui.add(
        egui::Label::new(
            RichText::new(Msg::PrefsSaveInHint.text(locale))
                .font(theme::text(10.5))
                .color(p.muted),
        )
        .wrap(),
    );
    ui.add_space(6.0);
    let current = app.state.settings.default_folder.clone();
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // Buttons first (they sit at the right edge), then the pill fills what is left.
            if app.state.is_signed_in()
                && theme::secondary_button(ui, Msg::PrefsChooseFolder.text(locale)).clicked()
            {
                app.ui.folders.start(workers);
            }
            if current.is_some()
                && theme::secondary_button(ui, Msg::PrefsClearFolder.text(locale)).clicked()
            {
                actions.push(Action::Settings(SettingsChange::DefaultFolder(None)));
            }
            let (rect, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width().max(120.0), 32.0),
                Sense::hover(),
            );
            ui.painter()
                .rect_filled(rect, CornerRadius::same(9), p.track.gamma_multiply(0.55));
            widgets::folder_icon(ui.painter(), Pos2::new(rect.left() + 20.0, rect.center().y));
            let name = current.as_ref().map_or_else(
                || Msg::PrefsDefaultFolderNone.text(locale),
                |folder| folder.name.clone(),
            );
            let clip = Rect::from_min_max(
                Pos2::new(rect.left() + 38.0, rect.top()),
                Pos2::new(rect.right() - 8.0, rect.bottom()),
            );
            ui.painter().with_clip_rect(clip).text(
                Pos2::new(clip.left(), rect.center().y),
                egui::Align2::LEFT_CENTER,
                name,
                theme::text(12.0),
                p.text,
            );
        });
    });
    ui.add_space(12.0);
}

/// "Default app": whether put.io adder opens magnet links and `.torrent` files, and a button to make it so.
fn default_app_row(ui: &mut egui::Ui, app: &App, locale: Locale, actions: &mut Vec<Action>) {
    use putio_core::handler::Check;
    let (hint, can_set) = match &app.state.handler_check {
        None => (Msg::PrefsDefaultAppChecking.text(locale), false),
        Some(Check::Unavailable(reason)) => (reason.clone(), false),
        Some(Check::Known(status)) if status.is_default() => {
            (Msg::PrefsDefaultAppIs.text(locale), false)
        }
        Some(Check::Known(_)) => (Msg::PrefsDefaultAppIsNot.text(locale), true),
    };
    widgets::row(ui, &Msg::PrefsDefaultApp.text(locale), Some(&hint), |ui| {
        if can_set && !app.state.handler_busy {
            let label = Msg::HandlerMakeDefault.text(locale);
            if widgets::text_button(ui, &label, true).clicked() {
                actions.push(Action::MakeDefaultApp);
            }
        }
    });
}

/// Version, links, update status and the "unofficial" note.
fn about(ui: &mut egui::Ui, app: &App, locale: Locale, actions: &mut Vec<Action>) {
    let p = theme::pal();
    widgets::section_title(ui, &Msg::PrefsAbout.text(locale));
    ui.vertical_centered(|ui| {
        ui.add_space(2.0);
        ui.label(
            RichText::new(
                Msg::PrefsVersion {
                    version: app.state.current_version.clone(),
                }
                .text(locale),
            )
            .font(theme::bold(11.5)),
        );
        ui.horizontal(|ui| {
            let links_width = 250.0;
            ui.add_space(((ui.available_width() - links_width) / 2.0).max(0.0));
            if ui
                .link(RichText::new(Msg::MenuProjectPage.text(locale)).font(theme::text(11.0)))
                .clicked()
            {
                actions.push(Action::OpenProjectPage);
            }
            ui.label(RichText::new("·").color(p.muted));
            if ui
                .link(RichText::new(Msg::MenuCheckUpdates.text(locale)).font(theme::text(11.0)))
                .clicked()
            {
                actions.push(Action::CheckForUpdates);
            }
        });
        match &app.state.update {
            UpdateStatus::Checking => {
                ui.add(egui::Spinner::new().size(12.0).color(p.accent_text));
            }
            UpdateStatus::Available { version, .. } => {
                ui.label(
                    RichText::new(
                        Msg::UpdateAvailable {
                            version: version.clone(),
                        }
                        .text(locale),
                    )
                    .font(theme::bold(11.0))
                    .color(p.accent_text),
                );
                if let Some(look) = update::look(&app.state) {
                    ui.add_space(4.0);
                    if update::pill(ui, &look).clicked()
                        && let Some(click) = look.click
                    {
                        actions.push(Action::Update(click));
                    }
                }
                if let putio_core::state::Install::Failed { message, .. } = &app.state.install {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(message)
                            .font(theme::text(10.5))
                            .color(p.error),
                    );
                }
            }
            UpdateStatus::UpToDate => {
                ui.label(
                    RichText::new(Msg::UpToDate.text(locale))
                        .font(theme::text(11.0))
                        .color(p.muted),
                );
            }
            UpdateStatus::Unknown => {}
        }
        ui.add_space(4.0);
        ui.label(
            RichText::new(Msg::PrefsUnofficial.text(locale))
                .font(theme::text(10.5))
                .color(p.muted),
        );
        ui.add_space(2.0);
    });
}

/// The second page: pick a folder on put.io. Sets `chosen` (to the folder) when the user confirms.
/// Returns `true` when the dialog should close.
/// The path to the folder being shown: every part but the last takes you back up to it.
fn breadcrumb(ui: &mut egui::Ui, app: &mut App, workers: &Workers) {
    let p = theme::pal();
    let mut jump = None;
    let trail_len = app.ui.folders.trail.len();
    ui.horizontal_wrapped(|ui| {
        for (index, (_, name)) in app.ui.folders.trail.iter().enumerate() {
            let last = index + 1 == trail_len;
            let text = RichText::new(name)
                .font(if last {
                    theme::bold(12.0)
                } else {
                    theme::text(12.0)
                })
                .color(if last { p.text } else { p.muted });
            if last {
                ui.label(text);
            } else {
                if ui
                    .add(egui::Label::new(text).sense(Sense::click()))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    jump = Some(index);
                }
                ui.label(RichText::new("›").color(p.muted));
            }
        }
    });
    if let Some(index) = jump {
        app.ui.folders.jump_to(workers, index);
    }
    ui.add_space(4.0);
}

/// The subfolders of the folder being shown. Returns the one the user opened, if any.
fn folder_list(ui: &mut egui::Ui, app: &App, locale: Locale) -> Option<(i64, String)> {
    let p = theme::pal();
    let mut enter = None;
    widgets::group(ui, |ui| {
        let picker = &app.ui.folders;
        scroll_region(ui, (body_height(ui) - 140.0).max(120.0), |ui| {
            if picker.loading {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| {
                    ui.add(egui::Spinner::new().size(18.0).color(p.accent_text))
                });
                ui.add_space(30.0);
            } else if picker.failed {
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 80.0), Sense::hover());
                widgets::centered_text(ui, rect, &Msg::TransfersFailed.text(locale), 12.0, p.error);
            } else if picker.folders.is_empty() {
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 80.0), Sense::hover());
                widgets::centered_text(
                    ui,
                    rect,
                    &Msg::PrefsNoSubfolders.text(locale),
                    12.0,
                    p.muted,
                );
            }
            for (index, folder) in picker.folders.iter().enumerate() {
                if index > 0 {
                    widgets::divider(ui);
                }
                let (rect, response) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), FOLDER_ROW_HEIGHT),
                    Sense::click(),
                );
                let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                if response.hovered() {
                    ui.painter().rect_filled(
                        rect.shrink2(Vec2::new(-8.0, 1.0)),
                        CornerRadius::same(8),
                        p.card_hover,
                    );
                }
                widgets::folder_icon(ui.painter(), Pos2::new(rect.left() + 12.0, rect.center().y));
                let clip = Rect::from_min_max(
                    Pos2::new(rect.left() + 34.0, rect.top()),
                    Pos2::new(rect.right() - 28.0, rect.bottom()),
                );
                ui.painter().with_clip_rect(clip).text(
                    Pos2::new(clip.left(), rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    &folder.name,
                    theme::text(12.5),
                    p.text,
                );
                widgets::row_chevron(ui.painter(), rect);
                if response.clicked() {
                    enter = Some((folder.id, folder.name.clone()));
                }
            }
        });
    });
    enter
}

fn folder_page(
    ui: &mut egui::Ui,
    app: &mut App,
    workers: &Workers,
    locale: Locale,
    chosen: &mut Option<Folder>,
) -> bool {
    let (back, close) = header(ui, &Msg::PrefsChooseFolderTitle.text(locale), true);
    if back {
        app.ui.folders.open = false;
        return false;
    }

    breadcrumb(ui, app, workers);
    ui.add_space(4.0);
    if let Some((id, name)) = folder_list(ui, app, locale) {
        app.ui.folders.enter(workers, id, name);
    }

    // Confirm.
    ui.add_space(8.0);
    let here = app.ui.folders.trail.last().cloned();
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some((id, name)) = here
                && theme::primary_button(
                    ui,
                    Msg::PrefsUseFolder { name: name.clone() }.text(locale),
                )
                .clicked()
            {
                *chosen = Some(Folder { id, name });
            }
            if theme::secondary_button(ui, Msg::PrefsCancel.text(locale)).clicked() {
                app.ui.folders.open = false;
            }
        });
    });
    close
}
