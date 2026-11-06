//! Preferences: a modal card with grouped settings, and the folder chooser as its second page.

use eframe::egui::{self, Align, CornerRadius, Layout, Pos2, Rect, RichText, Sense, Vec2};
use putio_api::client::FileListing;
use putio_api::{AccountSettingChange, ApiError, File};
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
pub(crate) const ROOT_NAME: &str = "Your Files";

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
    /// What the choice is for.
    target: Target,
}

/// What a folder is being chosen for.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// This app's own save folder.
    #[default]
    App,
    /// The account's default folder on put.io.
    Account,
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

    /// Whether the folder page is showing (instead of the page it was opened from).
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    /// Go back from the folder page.
    pub(crate) fn close(&mut self) {
        self.open = false;
    }

    /// Open the folder page at the top of the account's files.
    pub(crate) fn start(&mut self, workers: &Workers) {
        self.target = Target::App;
        self.open = true;
        self.trail.clear();
        self.enter(workers, 0, ROOT_NAME.into());
    }

    /// [`start`](Self::start), to choose the account's default folder.
    fn start_for_account(&mut self, workers: &Workers) {
        self.start(workers);
        self.target = Target::Account;
    }

    fn enter(&mut self, workers: &Workers, id: i64, name: String) {
        self.trail.push((id, name));
        self.begin_loading(workers, id);
    }

    /// Go back up to the `index`-th folder of the trail.
    fn jump_to(&mut self, workers: &Workers, index: usize) {
        self.trail.truncate(index + 1);
        if let Some(&(id, _)) = self.trail.last() {
            self.begin_loading(workers, id);
        }
    }

    /// Ask for the folders inside `parent`. The rows of the folder being left go at once: until the
    /// answer arrives they would be clickable, and a click would put the wrong folder in the trail.
    fn begin_loading(&mut self, workers: &Workers, parent: i64) {
        self.folders.clear();
        self.failed = false;
        self.loading = true;
        workers.send(Job::ListFolders { parent });
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
            created_at: None,
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
            target: Target::App,
        }
    }
}

/// Everything the dialog can ask the app to do, collected while drawing and applied afterwards.
enum Action {
    Settings(SettingsChange),
    AccountSetting(AccountSettingChange),
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
                let page = if app.ui.folders.target == Target::Account {
                    FolderPage::account()
                } else {
                    FolderPage::choose()
                };
                folder_page(ui, app, &workers, locale, &page, &mut folder_chosen)
            } else {
                settings_page(ui, app, &workers, locale, &mut actions)
            }
        });

    let close = response.inner || response.should_close();
    for action in actions {
        match action {
            Action::Settings(change) => app.dispatch(ctx, Event::SettingsChanged(change)),
            Action::AccountSetting(change) => {
                app.dispatch(ctx, Event::AccountSettingRequested(change));
            }
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
        let event = if app.ui.folders.target == Target::Account {
            Event::AccountSettingRequested(AccountSettingChange::DefaultDownloadFolder(folder.id))
        } else {
            Event::SettingsChanged(SettingsChange::DefaultFolder(Some(folder)))
        };
        app.dispatch(ctx, event);
    }
    if close {
        // A message about a refused change is not kept for the next visit.
        app.dispatch(ctx, Event::AccountSettingDismissed);
        // Escape or a click outside goes back a page first, and closes from the first page.
        if app.ui.folders.open {
            app.ui.folders.open = false;
        } else {
            app.ui.prefs_open = false;
        }
    }
}

/// Title row: optional back arrow, the title, and a close button. Returns (back, close) clicks.
pub(crate) fn header(
    ui: &mut egui::Ui,
    title: &str,
    with_back: bool,
    locale: Locale,
) -> (bool, bool) {
    let (mut back, mut close) = (false, false);
    ui.horizontal(|ui| {
        ui.set_height(32.0);
        if with_back {
            back = widgets::back_button(ui, &Msg::A11yBack.text(locale));
        }
        ui.label(RichText::new(title).font(theme::bold(17.0)));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            close = widgets::close_button(ui, &Msg::A11yClose.text(locale));
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
    let (_, close) = header(ui, &Msg::PrefsTitle.text(locale), false, locale);
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
                    if widgets::switch(ui, &mut magnet, &Msg::PrefsQuitMagnet.text(locale))
                        .changed()
                    {
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
                    if widgets::switch(ui, &mut torrent, &Msg::PrefsQuitTorrent.text(locale))
                        .changed()
                    {
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
                    if widgets::switch(ui, &mut updates, &Msg::PrefsCheckUpdates.text(locale))
                        .changed()
                    {
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
        widgets::group(ui, |ui| {
            let name = settings.default_folder.as_ref().map(|f| f.name.as_str());
            let signed_in = app.state.is_signed_in();
            match folder_chooser(
                ui,
                locale,
                (&Msg::PrefsSaveIn, &Msg::PrefsSaveInHint),
                name,
                (signed_in, name.is_some()),
            ) {
                Some(FolderClick::Choose) => app.ui.folders.start(workers),
                Some(FolderClick::Clear) => {
                    actions.push(Action::Settings(SettingsChange::DefaultFolder(None)));
                }
                None => {}
            }
        });

        account_section(ui, app, workers, locale, actions);

        about(ui, app, locale, actions);
    });
    close
}

/// What a click in a folder row asked for.
enum FolderClick {
    Choose,
    Clear,
}

/// A folder row: a title, a hint, and below them the current folder's name with the buttons that choose
/// another (when `can` allows) and take the choice away (when `can` allows).
fn folder_chooser(
    ui: &mut egui::Ui,
    locale: Locale,
    (title, hint): (&Msg, &Msg),
    name: Option<&str>,
    (can_choose, can_clear): (bool, bool),
) -> Option<FolderClick> {
    let p = theme::pal();
    let mut click = None;
    ui.add_space(10.0);
    ui.label(RichText::new(title.text(locale)).font(theme::bold(12.5)));
    ui.add(
        egui::Label::new(
            RichText::new(hint.text(locale))
                .font(theme::text(10.5))
                .color(p.muted),
        )
        .wrap(),
    );
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // Buttons first (they sit at the right edge), then the pill fills what is left.
            if can_choose
                && theme::secondary_button(ui, Msg::PrefsChooseFolder.text(locale)).clicked()
            {
                click = Some(FolderClick::Choose);
            }
            if can_clear
                && theme::secondary_button(ui, Msg::PrefsClearFolder.text(locale)).clicked()
            {
                click = Some(FolderClick::Clear);
            }
            let (rect, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width().max(120.0), 32.0),
                Sense::hover(),
            );
            ui.painter()
                .rect_filled(rect, CornerRadius::same(9), p.track.gamma_multiply(0.55));
            widgets::folder_icon(ui.painter(), Pos2::new(rect.left() + 20.0, rect.center().y));
            let name = name.map_or_else(|| Msg::PrefsDefaultFolderNone.text(locale), str::to_owned);
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
    click
}

/// The Account group: who is signed in, and the settings of the account itself that put.io told us. A
/// setting put.io did not report is left out rather than shown in a position it may not be in.
fn account_section(
    ui: &mut egui::Ui,
    app: &mut App,
    workers: &Workers,
    locale: Locale,
    actions: &mut Vec<Action>,
) {
    let Some(username) = app.state.account.as_ref().map(|a| a.username.clone()) else {
        return;
    };
    let trash = app.state.trash_enabled();
    let folder = app.state.account_default_folder();
    // "…" until put.io has said what the folder is called.
    let folder_name =
        folder.map(|id| super::folder_name(app, id, locale).unwrap_or_else(|| "…".into()));
    widgets::section_title(ui, &Msg::PrefsAccount.text(locale));
    widgets::group(ui, |ui| {
        widgets::row(
            ui,
            &Msg::PrefsSignedInAs { name: username }.text(locale),
            None,
            |ui| {
                if theme::secondary_button(ui, Msg::MenuSignOut.text(locale)).clicked() {
                    actions.push(Action::SignOut);
                }
            },
        );
        if let Some(mut on) = trash {
            widgets::divider(ui);
            widgets::row(
                ui,
                &Msg::PrefsTrash.text(locale),
                Some(&Msg::PrefsTrashHint.text(locale)),
                |ui| {
                    if widgets::switch(ui, &mut on, &Msg::PrefsTrash.text(locale)).changed() {
                        actions.push(Action::AccountSetting(AccountSettingChange::TrashEnabled(
                            on,
                        )));
                    }
                },
            );
        }
        if let Some(name) = &folder_name {
            widgets::divider(ui);
            let click = folder_chooser(
                ui,
                locale,
                (&Msg::PrefsAccountFolder, &Msg::PrefsAccountFolderHint),
                Some(name),
                (true, false),
            );
            if matches!(click, Some(FolderClick::Choose)) {
                app.ui.folders.start_for_account(workers);
            }
        }
    });
    if let Some(reason) = &app.state.account_setting.error {
        ui.add(
            egui::Label::new(
                RichText::new(
                    Msg::AccountSettingFailed {
                        reason: reason.clone(),
                    }
                    .text(locale),
                )
                .font(theme::text(11.0))
                .color(theme::pal().error),
            )
            .wrap(),
        );
    }
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

/// What the folder page says: its title, what the button that chooses a folder is called, and, while
/// something is being done with the choice, a line about how it went.
pub(crate) struct FolderPage<'a> {
    pub(crate) title: Msg,
    /// The button's label, given the name of the folder being shown.
    pub(crate) confirm: fn(String) -> Msg,
    /// What went wrong with the last choice, if anything.
    pub(crate) error: Option<&'a str>,
    /// The choice is being acted on: the button waits.
    pub(crate) busy: bool,
}

impl FolderPage<'_> {
    /// Choosing the account's default folder on put.io.
    fn account() -> Self {
        Self {
            title: Msg::PrefsAccountFolder,
            ..Self::choose()
        }
    }

    /// Choosing the folder new transfers are saved in.
    pub(crate) fn choose() -> Self {
        Self {
            title: Msg::PrefsChooseFolderTitle,
            confirm: |name| Msg::PrefsUseFolder { name },
            error: None,
            busy: false,
        }
    }
}

/// The folder page: where to go, and a button that chooses the folder being shown.
pub(crate) fn folder_page(
    ui: &mut egui::Ui,
    app: &mut App,
    workers: &Workers,
    locale: Locale,
    page: &FolderPage,
    chosen: &mut Option<Folder>,
) -> bool {
    let (back, close) = header(ui, &page.title.text(locale), true, locale);
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
    if let Some(error) = page.error {
        ui.add(
            egui::Label::new(
                RichText::new(error)
                    .font(theme::text(11.0))
                    .color(theme::pal().error),
            )
            .wrap(),
        );
        ui.add_space(6.0);
    }
    let here = app.ui.folders.trail.last().cloned();
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some((id, name)) = here
                && theme::primary_button(ui, (page.confirm)(name.clone()).text(locale)).clicked()
                && !page.busy
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::test_support::text_boxes;

    #[test]
    fn a_folder_row_fits_the_card_in_every_language_whatever_the_folders_name() {
        let long = "An.Extremely.Long.Folder.Name.".repeat(8);
        for locale in [Locale::En, Locale::De, Locale::Es] {
            for (name, can_clear) in [
                (None, false),
                (Some(long.as_str()), true),
                (Some("…"), false),
            ] {
                let mut used = Rect::NOTHING;
                let boxes = text_boxes(|ui| {
                    ui.allocate_ui(Vec2::new(CONTENT_WIDTH, 200.0), |ui| {
                        folder_chooser(
                            ui,
                            locale,
                            (&Msg::PrefsAccountFolder, &Msg::PrefsAccountFolderHint),
                            name,
                            (true, can_clear),
                        );
                        used = ui.min_rect();
                    });
                });
                assert!(used.width() <= CONTENT_WIDTH + 0.5, "{locale:?}: {used:?}");
                for text_box in &boxes {
                    assert!(
                        text_box.visible.right() <= used.left() + CONTENT_WIDTH + 0.5,
                        "{locale:?}: {text_box:?}"
                    );
                }
                // The title, the hint, the name (cut) and the buttons are all there.
                assert!(boxes.len() >= 4, "{locale:?}: {boxes:?}");
            }
        }
    }

    #[test]
    fn the_trash_row_keeps_its_switch_clear_of_the_text_in_every_language() {
        for locale in [Locale::En, Locale::De, Locale::Es] {
            let mut on = true;
            let mut rows = Rect::NOTHING;
            let boxes = text_boxes(|ui| {
                ui.allocate_ui(Vec2::new(CONTENT_WIDTH, 200.0), |ui| {
                    widgets::row(
                        ui,
                        &Msg::PrefsTrash.text(locale),
                        Some(&Msg::PrefsTrashHint.text(locale)),
                        |ui| {
                            widgets::switch(ui, &mut on, &Msg::PrefsTrash.text(locale));
                        },
                    );
                    rows = ui.min_rect();
                });
            });
            assert!(rows.width() <= CONTENT_WIDTH + 0.5, "{locale:?}: {rows:?}");
            // The text stops before the switch begins (the switch is 44 wide at the right edge).
            for text_box in &boxes {
                assert!(
                    text_box.full.right() <= rows.left() + CONTENT_WIDTH - 44.0,
                    "{locale:?}: {text_box:?}"
                );
            }
        }
    }

    #[test]
    fn the_rows_of_the_folder_being_left_are_gone_while_the_next_one_loads() {
        let workers = Workers::detached();
        let mut picker = FolderPicker::demo();
        assert!(picker.folders.len() > 1);
        picker.enter(&workers, 11, "2023".into());
        assert!(picker.loading);
        assert!(
            picker.folders.is_empty(),
            "nothing to click on a wrong page"
        );
        assert_eq!(picker.trail.last().map(|(id, _)| *id), Some(11));
    }

    #[test]
    fn going_back_up_also_clears_the_rows_until_the_answer_comes() {
        let workers = Workers::detached();
        let mut picker = FolderPicker::demo();
        picker.jump_to(&workers, 0);
        assert!(picker.loading);
        assert_eq!(picker.folders.len(), 0);
        assert_eq!(picker.trail.len(), 1);
    }
}
