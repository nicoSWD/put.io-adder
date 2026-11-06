//! The cards of the Files tab: naming a file or a new folder, choosing where to move something, and the
//! question before deleting.
//!
//! They follow the feed form: modal cards like Preferences, a click on the dimmed backdrop does not close a
//! card that holds something typed, Escape first leaves the text field it is in, and a card whose request is
//! on its way cannot be closed. Moving reuses the folder page of Preferences.

use eframe::egui::{self, Align, Id, Key, Layout, RichText, Ui};
use putio_api::File;
use putio_core::files::{MAX_NAME_CHARS, display_name, is_folder};
use putio_core::i18n::Msg;
use putio_core::secret::Secret;
use putio_core::state::{Event, FileAction};

use super::prefs::{self, FolderPage};
use super::{theme, widgets};
use crate::app::App;

/// Width of the content inside the naming card (the window is 560 wide).
const NAME_WIDTH: f32 = 420.0;
/// Width of the move card (the folder page of Preferences).
const MOVE_WIDTH: f32 = 456.0;
/// Width of the delete question.
const ASK_WIDTH: f32 = 400.0;

#[derive(Debug, Default)]
pub(crate) struct FileDialog {
    naming: Option<Naming>,
    moving: Option<Moving>,
    deleting: Option<Deleting>,
    /// What is being typed in the password card.
    password: Typed,
    /// The archive the password card has taken the keyboard for.
    password_focused_for: Option<i64>,
}

/// A password being typed. It never shows in debug output.
#[derive(Default)]
struct Typed(String);

impl std::fmt::Debug for Typed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Typed(…)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Purpose {
    Rename(i64),
    NewFolder,
}

#[derive(Debug)]
struct Naming {
    purpose: Purpose,
    text: String,
    /// The name field takes the keyboard when the card opens.
    focus: bool,
    /// The field that has the keyboard, so Escape can leave it before it closes the card.
    typing: Option<Id>,
}

#[derive(Debug)]
struct Moving {
    id: i64,
    /// What went wrong with the choice, when the check here found it.
    problem: Option<Msg>,
}

#[derive(Debug)]
struct Deleting {
    id: i64,
    name: String,
    folder: bool,
}

impl FileDialog {
    /// One of the cards is showing.
    pub(crate) fn is_open(&self) -> bool {
        self.naming.is_some() || self.moving.is_some() || self.deleting.is_some()
    }
}

fn open_card(app: &mut App, ctx: &egui::Context) {
    // What the last card ended with is of no interest to this one.
    app.dispatch(ctx, Event::FileActionDismissed);
    app.ui.folders.close();
}

pub(crate) fn open_rename(app: &mut App, ctx: &egui::Context, file: &File) {
    open_card(app, ctx);
    app.ui.file_dialog.naming = Some(Naming {
        purpose: Purpose::Rename(file.id),
        text: display_name(file),
        focus: true,
        typing: None,
    });
}

pub(crate) fn open_new_folder(app: &mut App, ctx: &egui::Context) {
    open_card(app, ctx);
    app.ui.file_dialog.naming = Some(Naming {
        purpose: Purpose::NewFolder,
        text: String::new(),
        focus: true,
        typing: None,
    });
}

pub(crate) fn open_move(app: &mut App, ctx: &egui::Context, file: &File) {
    open_card(app, ctx);
    let workers = app.workers();
    app.ui.folders.start(&workers);
    app.ui.file_dialog.moving = Some(Moving {
        id: file.id,
        problem: None,
    });
}

pub(crate) fn ask_to_delete(app: &mut App, file: &File) {
    app.ui.file_dialog.deleting = Some(Deleting {
        id: file.id,
        name: display_name(file),
        folder: is_folder(file),
    });
}

/// Debug builds only: show a card for a preview.
#[cfg(debug_assertions)]
pub(crate) fn demo(app: &mut App, ctx: &egui::Context, view: &str) {
    let first = app
        .state
        .files
        .items
        .iter()
        .find(|f| !is_folder(f))
        .cloned();
    let folder = app.state.files.items.iter().find(|f| is_folder(f)).cloned();
    match view {
        "file-rename" => {
            if let Some(file) = first {
                open_rename(app, ctx, &file);
            }
        }
        "file-new-folder" => open_new_folder(app, ctx),
        "file-move-long" => {
            if let Some(file) = first {
                open_move(app, ctx, &file);
                app.ui.folders = super::prefs::FolderPicker::demo_long();
            }
        }
        "file-move" => {
            if let Some(file) = first {
                open_move(app, ctx, &file);
                app.ui.folders = super::prefs::FolderPicker::demo();
            }
        }
        "file-delete" => {
            if let Some(file) = first {
                ask_to_delete(app, &file);
            }
        }
        "file-delete-folder" => {
            if let Some(file) = folder {
                ask_to_delete(app, &file);
            }
        }
        _ => {}
    }
}

pub(crate) fn show(ctx: &egui::Context, app: &mut App) {
    if !app.state.is_signed_in() {
        // The account is gone (signed out, session expired): so is anything half done for it.
        app.ui.file_dialog = FileDialog::default();
        app.ui.selected_file = None;
        app.ui.search_text.clear();
        return;
    }
    naming(ctx, app);
    moving(ctx, app);
    deleting(ctx, app);
    password(ctx, app);
}

// ---- naming -----------------------------------------------------------------------------------

fn naming(ctx: &egui::Context, app: &mut App) {
    let Some(mut card) = app.ui.file_dialog.naming.take() else {
        return;
    };
    if *app.state.file_action() == FileAction::Done {
        app.dispatch(ctx, Event::FileActionDismissed);
        return;
    }
    let working = *app.state.file_action() == FileAction::Working;
    let failure = match app.state.file_action() {
        FileAction::Failed(text) => Some(text.clone()),
        _ => None,
    };
    let locale = app.state.locale;
    let (frame, backdrop) = widgets::dialog_frame();
    let mut submit = false;
    let mut close = false;
    let response = egui::Modal::new(Id::unique("file-name"))
        .backdrop_color(backdrop)
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(NAME_WIDTH);
            let title = match card.purpose {
                Purpose::Rename(_) => Msg::FileRenameTitle,
                Purpose::NewFolder => Msg::FilesNewFolder,
            }
            .text(locale);
            close = prefs::header(ui, &title, false, locale).1;
            let label = ui
                .add(egui::Label::new(
                    RichText::new(Msg::ColName.text(locale)).font(theme::bold(12.5)),
                ))
                .id;
            ui.add_space(4.0);
            let field = ui.add_enabled_ui(!working, |ui| {
                widgets::text_field(ui, &mut card.text, "", Some(label), MAX_NAME_CHARS)
            });
            let field = field.inner;
            if card.focus {
                field.request_focus();
                card.focus = false;
            }
            card.typing = field.has_focus().then_some(field.id);
            submit |= field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
            ui.add_space(8.0);
            if let Some(failure) = &failure {
                ui.add(
                    egui::Label::new(
                        RichText::new(failure)
                            .font(theme::text(11.0))
                            .color(theme::pal().error),
                    )
                    .wrap(),
                );
                ui.add_space(8.0);
            }
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let confirm = match card.purpose {
                        Purpose::Rename(_) => Msg::FileRenameConfirm,
                        Purpose::NewFolder => Msg::FileCreateConfirm,
                    }
                    .text(locale);
                    if widgets::text_button(ui, &confirm, true).clicked() {
                        submit = true;
                    }
                    if widgets::text_button(ui, &Msg::PrefsCancel.text(locale), false).clicked() {
                        close = true;
                    }
                });
            });
        });

    // A click on the backdrop does not count: see the module's note.
    let escape = response.should_close() && !response.backdrop_response.clicked();
    if (close || escape) && !working {
        if escape && let Some(field) = card.typing.take() {
            // The first Escape leaves the field it was pressed in.
            ctx.memory_mut(|memory| memory.surrender_focus(field));
        } else {
            return; // closed: the card is not put back
        }
    }
    if submit && !working {
        let name = card.text.clone();
        let event = match card.purpose {
            Purpose::Rename(id) => Event::FileRenameRequested { id, name },
            Purpose::NewFolder => Event::FolderCreateRequested(name),
        };
        app.dispatch(ctx, event);
    }
    app.ui.file_dialog.naming = Some(card);
}

// ---- moving -----------------------------------------------------------------------------------

fn moving(ctx: &egui::Context, app: &mut App) {
    let Some(mut card) = app.ui.file_dialog.moving.take() else {
        return;
    };
    // The folder page was left (back, Cancel or Escape): the move is off.
    if !app.ui.folders.is_open() {
        app.dispatch(ctx, Event::FileActionDismissed);
        return;
    }
    if *app.state.file_action() == FileAction::Done {
        app.dispatch(ctx, Event::FileActionDismissed);
        app.ui.folders.close();
        return;
    }
    let working = *app.state.file_action() == FileAction::Working;
    let locale = app.state.locale;
    let said = card
        .problem
        .as_ref()
        .map(|msg| msg.text(locale))
        .or_else(|| match app.state.file_action() {
            FileAction::Failed(text) => Some(text.clone()),
            _ => None,
        });
    let page = FolderPage {
        title: Msg::FileMoveTitle,
        confirm: |name| Msg::FileMoveHere { name },
        error: said.as_deref(),
        busy: working,
    };
    let workers = app.workers();
    let (frame, backdrop) = widgets::dialog_frame();
    let mut chosen = None;
    let response = egui::Modal::new(Id::unique("file-move"))
        .backdrop_color(backdrop)
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(MOVE_WIDTH);
            prefs::folder_page(ui, app, &workers, locale, &page, &mut chosen)
        });
    let escape = response.should_close() && !response.backdrop_response.clicked();
    if (response.inner || escape) && !working {
        app.ui.folders.close();
        app.dispatch(ctx, Event::FileActionDismissed);
        return;
    }
    if let Some(folder) = chosen
        && !working
    {
        if folder.id == card.id {
            card.problem = Some(Msg::FileErrMoveIntoSelf);
        } else if folder.id == app.state.files.folder_id() && app.state.files.search.is_none() {
            // It is there already.
            app.ui.folders.close();
            return;
        } else {
            card.problem = None;
            app.dispatch(
                ctx,
                Event::FileMoveRequested {
                    id: card.id,
                    to: folder.id,
                },
            );
        }
    }
    app.ui.file_dialog.moving = Some(card);
}

// ---- a password for an archive --------------------------------------------------------------------

fn password(ctx: &egui::Context, app: &mut App) {
    let Some(ask) = app.state.extract_ask().cloned() else {
        app.ui.file_dialog.password = Typed::default();
        app.ui.file_dialog.password_focused_for = None;
        return;
    };
    let locale = app.state.locale;
    let take_focus = app.ui.file_dialog.password_focused_for != Some(ask.file_id);
    app.ui.file_dialog.password_focused_for = Some(ask.file_id);
    let (frame, backdrop) = widgets::dialog_frame();
    let mut submit = false;
    let mut close = false;
    let typed = &mut app.ui.file_dialog.password;
    let response = egui::Modal::new(Id::unique("file-password"))
        .backdrop_color(backdrop)
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(NAME_WIDTH);
            close = prefs::header(ui, &Msg::ExtractPasswordTitle.text(locale), false, locale).1;
            ui.add(
                egui::Label::new(
                    RichText::new(
                        Msg::ExtractPasswordBody {
                            name: ask.name.clone(),
                        }
                        .text(locale),
                    )
                    .font(theme::text(12.0))
                    .color(theme::pal().muted),
                )
                .wrap(),
            );
            ui.add_space(10.0);
            let label = ui
                .add(egui::Label::new(
                    RichText::new(Msg::FilesPasswordLabel.text(locale)).font(theme::bold(12.5)),
                ))
                .id;
            ui.add_space(4.0);
            let field = widgets::password_field(ui, &mut typed.0, Some(label), 512);
            if take_focus {
                field.request_focus();
            }
            submit |= field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let confirm = Msg::FileExtractConfirm.text(locale);
                    if widgets::text_button(ui, &confirm, true).clicked() {
                        submit = true;
                    }
                    if widgets::text_button(ui, &Msg::PrefsCancel.text(locale), false).clicked() {
                        close = true;
                    }
                });
            });
        });
    // A click on the backdrop does not count: a half-typed password is not worth losing to a stray click.
    let escape = response.should_close() && !response.backdrop_response.clicked();
    if close || escape {
        app.ui.file_dialog.password = Typed::default();
        app.dispatch(ctx, Event::FileExtractPasswordDismissed);
    } else if submit && !app.ui.file_dialog.password.0.is_empty() {
        let secret = Secret::new(std::mem::take(&mut app.ui.file_dialog.password.0));
        app.dispatch(
            ctx,
            Event::FileExtractRequested {
                id: ask.file_id,
                password: Some(secret),
            },
        );
    }
}

// ---- the question before deleting ---------------------------------------------------------------

fn deleting(ctx: &egui::Context, app: &mut App) {
    let Some(card) = app.ui.file_dialog.deleting.take() else {
        return;
    };
    let locale = app.state.locale;
    let (frame, backdrop) = widgets::dialog_frame();
    let mut confirmed = false;
    let mut cancelled = false;
    let response = egui::Modal::new(Id::unique("file-delete"))
        .backdrop_color(backdrop)
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(ASK_WIDTH);
            let (title, body) = if card.folder {
                (
                    Msg::FileDeleteFolderTitle,
                    Msg::FileDeleteFolderBody {
                        name: card.name.clone(),
                    },
                )
            } else {
                (
                    Msg::FileDeleteFileTitle,
                    Msg::FileDeleteFileBody {
                        name: card.name.clone(),
                    },
                )
            };
            ui.add(egui::Label::new(
                RichText::new(title.text(locale)).font(theme::bold(15.0)),
            ));
            ui.add_space(6.0);
            ui.add(
                egui::Label::new(
                    RichText::new(body.text(locale))
                        .font(theme::text(12.0))
                        .color(theme::pal().muted),
                )
                .wrap(),
            );
            ui.add_space(16.0);
            confirm_row(ui, &mut confirmed, &mut cancelled, locale);
        });
    // Escape and a click outside are the safe answer: nothing is deleted by accident.
    if confirmed {
        app.dispatch(ctx, Event::FileDeleteRequested(card.id));
    } else if !(cancelled || response.should_close()) {
        app.ui.file_dialog.deleting = Some(card);
    }
}

fn confirm_row(
    ui: &mut Ui,
    confirmed: &mut bool,
    cancelled: &mut bool,
    locale: putio_core::i18n::Locale,
) {
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::danger_button(ui, &Msg::FileDeleteConfirm.text(locale)).clicked() {
                *confirmed = true;
            }
            if widgets::text_button(ui, &Msg::PrefsCancel.text(locale), false).clicked() {
                *cancelled = true;
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use putio_core::i18n::Locale;

    use super::*;
    use crate::ui::test_support::text_boxes;

    #[test]
    fn the_card_buttons_fit_side_by_side_in_every_language() {
        for locale in [Locale::En, Locale::De, Locale::Es] {
            for (confirm, width) in [
                (Msg::FileRenameConfirm, NAME_WIDTH),
                (Msg::FileCreateConfirm, NAME_WIDTH),
                (Msg::FileDeleteConfirm, ASK_WIDTH),
            ] {
                let needed = std::cell::Cell::new(0.0);
                text_boxes(|ui| {
                    ui.set_width(width);
                    let a = widgets::text_button(ui, &Msg::PrefsCancel.text(locale), false)
                        .rect
                        .width();
                    let b = widgets::text_button(ui, &confirm.text(locale), true)
                        .rect
                        .width();
                    needed.set(a + b + ui.spacing().item_spacing.x);
                });
                assert!(
                    needed.get() <= width,
                    "{locale:?} {confirm:?}: needs {}",
                    needed.get()
                );
            }
        }
    }

    #[test]
    fn the_delete_question_stays_inside_its_card_in_every_language() {
        let long = "A file with a very long name that keeps going and going and going.mkv";
        for locale in [Locale::En, Locale::De, Locale::Es] {
            for body in [
                Msg::FileDeleteFileBody { name: long.into() },
                Msg::FileDeleteFolderBody { name: long.into() },
            ] {
                let boxes = text_boxes(|ui| {
                    ui.set_width(ASK_WIDTH);
                    ui.add(
                        egui::Label::new(RichText::new(body.text(locale)).font(theme::text(12.0)))
                            .wrap(),
                    );
                });
                for text in &boxes {
                    assert!(
                        text.full.right() <= ASK_WIDTH + 0.5,
                        "{locale:?}: {:?}",
                        text.text
                    );
                }
            }
        }
    }

    #[test]
    fn the_password_card_fits_in_every_language_and_never_prints_the_password() {
        for locale in [Locale::En, Locale::De, Locale::Es] {
            let needed = std::cell::Cell::new(0.0);
            let boxes = text_boxes(|ui| {
                ui.set_width(NAME_WIDTH);
                ui.add(
                    egui::Label::new(
                        RichText::new(
                            Msg::ExtractPasswordBody {
                                name: "An archive with a very long name that keeps going and going.rar".into(),
                            }
                            .text(locale),
                        )
                        .font(theme::text(12.0)),
                    )
                    .wrap(),
                );
                let a = widgets::text_button(ui, &Msg::PrefsCancel.text(locale), false)
                    .rect
                    .width();
                let b = widgets::text_button(ui, &Msg::FileExtractConfirm.text(locale), true)
                    .rect
                    .width();
                needed.set(a + b + ui.spacing().item_spacing.x);
            });
            assert!(
                needed.get() <= NAME_WIDTH,
                "{locale:?}: needs {}",
                needed.get()
            );
            for text in &boxes {
                assert!(
                    text.full.right() <= NAME_WIDTH + 0.5,
                    "{locale:?}: {:?}",
                    text.text
                );
            }
        }
        let typed = Typed("hunter2".into());
        assert!(!format!("{typed:?}").contains("hunter2"));
        let dialog = FileDialog {
            password: Typed("hunter2".into()),
            ..FileDialog::default()
        };
        assert!(!format!("{dialog:?}").contains("hunter2"), "{dialog:?}");
    }

    #[test]
    fn nothing_is_open_until_a_card_is_asked_for() {
        assert!(!FileDialog::default().is_open());
        let open = FileDialog {
            deleting: Some(Deleting {
                id: 1,
                name: "a".into(),
                folder: false,
            }),
            ..FileDialog::default()
        };
        assert!(open.is_open());
    }
}
