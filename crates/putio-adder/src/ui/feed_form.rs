//! The add / edit feed form, and the question before a feed is deleted.
//!
//! Both are modal cards like Preferences. The form is a scrolling body with a fixed footer, so it works
//! at the shortest window height as well as the tallest; its folder choice is the folder page of
//! Preferences, shown in place of the form while the user picks.
//!
//! Nothing is lost by accident: a click on the dimmed backdrop does not close the form (a half-typed
//! address is not worth throwing away for a stray click), Escape first leaves the text field it is in,
//! and a form that is being saved cannot be closed.

use eframe::egui::{
    self, Align, CornerRadius, Id, Key, Layout, Pos2, Rect, RichText, Sense, Ui, Vec2,
};
use putio_api::RssFeed;
use putio_core::feeds::{
    FeedForm, FeedProblem, MAX_KEYWORDS_CHARS, MAX_TITLE_CHARS, MAX_URL_CHARS, host_of,
};
use putio_core::i18n::{Locale, Msg};
use putio_core::settings::Folder;
use putio_core::state::{Event, FeedSave};

use super::prefs::{self, ROOT_NAME};
use super::{theme, widgets};
use crate::app::App;
use crate::worker::Job;

/// Width of the content inside the card (the window is 560 wide).
const CONTENT_WIDTH: f32 = 456.0;
/// Width of the delete question (the shorter card).
const ASK_WIDTH: f32 = 400.0;
/// Space between the blocks of the form.
const BLOCK_GAP: f32 = 14.0;
/// Room under the scrolling body for the buttons.
const FOOTER_HEIGHT: f32 = 56.0;
/// Room for the line that says what is wrong, when there is one.
const PROBLEM_HEIGHT: f32 = 28.0;
/// The line under the address field gives the user a feed to try.
const URL_HINT: &str = "https://example.com/feed.xml";

/// The two cards of this module: which one is showing, and what the form holds.
#[derive(Debug, Default)]
pub(crate) struct FeedDialog {
    editing: Option<Editing>,
    /// The feed whose deletion is being asked about.
    delete: Option<i64>,
}

#[derive(Debug)]
struct Editing {
    /// `None` when adding.
    id: Option<i64>,
    form: FeedForm,
    /// What is wrong with the form, found when the user tried to save it.
    problem: Option<FeedProblem>,
    /// The address field takes the keyboard when the form opens.
    focus_url: bool,
    /// The text field that has the keyboard, so Escape can leave it before it closes the card.
    typing: Option<Id>,
}

impl FeedDialog {
    /// A card of this module is showing.
    pub(crate) fn is_open(&self) -> bool {
        self.editing.is_some() || self.delete.is_some()
    }

    pub(crate) fn ask_to_delete(&mut self, id: i64) {
        self.delete = Some(id);
    }
}

/// Open the form for a new feed. It saves into the default folder of the preferences, else the top level.
pub(crate) fn open_new(app: &mut App, ctx: &egui::Context) {
    let folder = app.state.settings.default_folder.clone();
    let folder_id = folder.as_ref().map_or(0, |folder| folder.id);
    if let Some(folder) = folder {
        app.ui.folder_names.insert(folder.id, Some(folder.name));
    }
    open(app, ctx, None, FeedForm::new(folder_id));
}

/// Open the form for changing `feed`.
pub(crate) fn open_edit(app: &mut App, ctx: &egui::Context, feed: &RssFeed) {
    open(app, ctx, Some(feed.id), FeedForm::from_feed(feed));
}

fn open(app: &mut App, ctx: &egui::Context, id: Option<i64>, form: FeedForm) {
    // What the last form ended with is of no interest to this one.
    app.dispatch(ctx, Event::FeedSaveDismissed);
    app.ui.folders.close();
    // The feed knows its folder by number only: ask put.io for the name (once).
    if form.folder_id != 0 && !app.ui.folder_names.contains_key(&form.folder_id) {
        app.send(Job::FolderName { id: form.folder_id });
    }
    app.ui.feed_dialog.editing = Some(Editing {
        id,
        form,
        problem: None,
        focus_url: id.is_none(),
        typing: None,
    });
}

/// Debug builds only: show the form or the question for a preview.
#[cfg(debug_assertions)]
pub(crate) fn demo(app: &mut App, ctx: &egui::Context, view: &str) {
    match view {
        "feed-add" => open_new(app, ctx),
        "feed-edit" | "feed-edit-long" => {
            if let Some(feed) = app.state.feeds.items.first().cloned() {
                open_edit(app, ctx, &feed);
            }
        }
        "feed-delete" => {
            if let Some(feed) = app.state.feeds.items.first() {
                app.ui.feed_dialog.delete = Some(feed.id);
            }
        }
        _ => {}
    }
}

pub(crate) fn show(ctx: &egui::Context, app: &mut App) {
    if !app.state.is_signed_in() {
        // The account is gone (signed out, session expired): so is anything half done for it.
        app.ui.feed_dialog = FeedDialog::default();
        app.ui.folder_names.clear();
        app.ui.selected_feed = None;
        return;
    }
    form(ctx, app);
    ask_to_delete(ctx, app);
}

// ---- the form -------------------------------------------------------------------------------

/// What the form page asks for.
#[derive(Debug, Default)]
struct Outcome {
    /// The X was pressed.
    close: bool,
    /// Save was pressed (or Enter in a field).
    submit: bool,
    /// "Choose…" was pressed: show the folder page.
    choose_folder: bool,
}

fn form(ctx: &egui::Context, app: &mut App) {
    let Some(mut editing) = app.ui.feed_dialog.editing.take() else {
        return;
    };
    if *app.state.feed_save() == FeedSave::Saved {
        // put.io took the feed: the form has done its job.
        app.dispatch(ctx, Event::FeedSaveDismissed);
        app.ui.folders.close();
        return;
    }
    let saving = *app.state.feed_save() == FeedSave::Saving;
    let failure = match app.state.feed_save() {
        FeedSave::Failed(text) => Some(text.clone()),
        _ => None,
    };
    let locale = app.state.locale;
    let workers = app.workers();
    let (frame, backdrop) = widgets::dialog_frame();
    let mut chosen: Option<Folder> = None;
    let response = egui::Modal::new(Id::unique("feed-form"))
        .backdrop_color(backdrop)
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(CONTENT_WIDTH);
            if app.ui.folders.is_open() {
                let close = prefs::folder_page(
                    ui,
                    app,
                    &workers,
                    locale,
                    &prefs::FolderPage::choose(),
                    &mut chosen,
                );
                Outcome {
                    close,
                    ..Outcome::default()
                }
            } else {
                page(ui, app, &mut editing, saving, failure.as_deref())
            }
        });

    if let Some(folder) = chosen {
        editing.form.folder_id = folder.id;
        app.ui.folder_names.insert(folder.id, Some(folder.name));
        app.ui.folders.close();
    }
    // A click on the backdrop does not count: see the module's note.
    let escape = response.should_close() && !response.backdrop_response.clicked();
    if (response.inner.close || escape) && !saving {
        if app.ui.folders.is_open() {
            app.ui.folders.close();
        } else if escape && let Some(field) = editing.typing.take() {
            // The first Escape leaves the field it was pressed in.
            ctx.memory_mut(|memory| memory.surrender_focus(field));
        } else {
            return; // closed: the form is not put back
        }
    }
    if response.inner.submit && !saving {
        match editing.form.settings() {
            Err(problem) => {
                editing.problem = Some(problem);
                editing.focus_url = true;
            }
            Ok(settings) => {
                editing.problem = None;
                app.dispatch(
                    ctx,
                    Event::FeedSaveRequested {
                        id: editing.id,
                        settings,
                    },
                );
            }
        }
    }
    app.ui.feed_dialog.editing = Some(editing);
}

/// What put.io (or the form's own check) has to say about the feed.
fn problem_text(problem: FeedProblem, locale: Locale) -> String {
    match problem {
        FeedProblem::UrlMissing => Msg::FeedErrUrlMissing,
        FeedProblem::UrlInvalid => Msg::FeedErrUrlInvalid,
    }
    .text(locale)
}

fn page(
    ui: &mut Ui,
    app: &mut App,
    editing: &mut Editing,
    saving: bool,
    failure: Option<&str>,
) -> Outcome {
    let locale = app.state.locale;
    let title = if editing.id.is_some() {
        Msg::FeedEditTitle
    } else {
        Msg::FeedsAdd
    }
    .text(locale);
    let (_, close) = prefs::header(ui, &title, false, locale);
    let mut outcome = Outcome {
        close,
        ..Outcome::default()
    };

    let said = editing
        .problem
        .map(|problem| problem_text(problem, locale))
        .or_else(|| failure.map(str::to_owned));
    let reserved = FOOTER_HEIGHT + if said.is_some() { PROBLEM_HEIGHT } else { 0.0 };
    let body = (prefs::body_height(ui) - reserved).max(120.0);
    prefs::scroll_region(ui, body, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        let folder = folder_name(app, editing.form.folder_id, locale);
        fields(ui, locale, &folder, editing, saving, &mut outcome);
    });
    if outcome.choose_folder {
        let workers = app.workers();
        app.ui.folders.start(&workers);
    }

    ui.add_space(8.0);
    if let Some(said) = said {
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), PROBLEM_HEIGHT - 8.0),
            Sense::hover(),
        );
        let galley = ui
            .painter()
            .layout(said, theme::text(11.0), theme::pal().error, rect.width());
        ui.painter().galley(rect.min, galley, theme::pal().error);
        ui.add_space(8.0);
    }
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let label = if saving {
                Msg::FeedSaving
            } else if editing.id.is_some() {
                Msg::FeedSave
            } else {
                Msg::FeedsAdd
            }
            .text(locale);
            if widgets::text_button(ui, &label, true).clicked() && !saving {
                outcome.submit = true;
            }
            if widgets::text_button(ui, &Msg::PrefsCancel.text(locale), false).clicked() && !saving
            {
                outcome.close = true;
            }
        });
    });
    outcome
}

/// A block's label (bold), with an optional quieter line under it. Returns the label's id, which the field
/// below it is `labelled_by` for screen readers.
fn label(ui: &mut Ui, text: &str, hint: Option<&str>) -> Id {
    let id = ui
        .add(egui::Label::new(
            RichText::new(text).font(theme::bold(12.5)),
        ))
        .id;
    if let Some(hint) = hint {
        ui.add(
            egui::Label::new(
                RichText::new(hint)
                    .font(theme::text(10.5))
                    .color(theme::pal().muted),
            )
            .wrap(),
        );
    }
    id
}

/// Enter in a text field saves, like the button.
fn enter_pressed(ui: &Ui, field: &egui::Response) -> bool {
    field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter))
}

#[allow(clippy::too_many_lines)] // the form is its fields, top to bottom
fn fields(
    ui: &mut Ui,
    locale: Locale,
    folder: &str,
    editing: &mut Editing,
    saving: bool,
    outcome: &mut Outcome,
) {
    let mut typing = None;
    let mut track = |field: &egui::Response| {
        if field.has_focus() {
            typing = Some(field.id);
        }
    };

    // Address.
    let url_label = label(ui, &Msg::FeedFieldUrl.text(locale), None);
    let url = ui.add_enabled_ui(!saving, |ui| {
        widgets::text_field(
            ui,
            &mut editing.form.url,
            URL_HINT,
            Some(url_label),
            MAX_URL_CHARS,
        )
    });
    let url = url.inner;
    if editing.focus_url {
        url.request_focus();
        editing.focus_url = false;
    }
    track(&url);
    outcome.submit |= enter_pressed(ui, &url);
    ui.add_space(BLOCK_GAP);

    // Name.
    let name_label = label(ui, &Msg::ColName.text(locale), None);
    // The field shows what the name will be when it is left empty.
    let placeholder = host_of(&editing.form.url).unwrap_or_else(|| Msg::FeedTitleHint.text(locale));
    let name = ui.add_enabled_ui(!saving, |ui| {
        widgets::text_field(
            ui,
            &mut editing.form.title,
            &placeholder,
            Some(name_label),
            MAX_TITLE_CHARS,
        )
    });
    let name = name.inner;
    track(&name);
    outcome.submit |= enter_pressed(ui, &name);
    ui.add_space(BLOCK_GAP);

    // Folder.
    label(ui, &Msg::FeedFieldFolder.text(locale), None);
    folder_row(ui, locale, folder, saving, outcome);
    ui.add_space(BLOCK_GAP);

    // Filters.
    let include_label = label(ui, &Msg::FeedFieldInclude.text(locale), None);
    let include = ui.add_enabled_ui(!saving, |ui| {
        widgets::text_field(
            ui,
            &mut editing.form.keyword,
            "1080p, 2160p",
            Some(include_label),
            MAX_KEYWORDS_CHARS,
        )
    });
    let include = include.inner;
    track(&include);
    outcome.submit |= enter_pressed(ui, &include);
    ui.add_space(8.0);
    let exclude_label = label(ui, &Msg::FeedFieldExclude.text(locale), None);
    let exclude = ui.add_enabled_ui(!saving, |ui| {
        widgets::text_field(
            ui,
            &mut editing.form.unwanted_keywords,
            "CAM, HDTS",
            Some(exclude_label),
            MAX_KEYWORDS_CHARS,
        )
    });
    let exclude = exclude.inner;
    track(&exclude);
    outcome.submit |= enter_pressed(ui, &exclude);
    ui.add(
        egui::Label::new(
            RichText::new(Msg::FeedKeywordsHint.text(locale))
                .font(theme::text(10.5))
                .color(theme::pal().muted),
        )
        .wrap(),
    );
    ui.add_space(BLOCK_GAP);

    // Housekeeping.
    ui.add_enabled_ui(!saving, |ui| {
        widgets::group(ui, |ui| {
            if editing.id.is_none() {
                switch(
                    ui,
                    (
                        &Msg::FeedSkipExisting.text(locale),
                        &Msg::FeedSkipExistingHint.text(locale),
                    ),
                    &mut editing.form.skip_existing,
                );
                widgets::divider(ui);
            }
            switch(
                ui,
                (
                    &Msg::FeedDeleteOld.text(locale),
                    &Msg::FeedDeleteOldHint.text(locale),
                ),
                &mut editing.form.delete_old_files,
            );
        });
    });
    ui.add_space(4.0);
    editing.typing = typing;
}

fn switch(ui: &mut Ui, (title, hint): (&str, &str), value: &mut bool) {
    widgets::row(ui, title, Some(hint), |ui| {
        widgets::switch(ui, value, title);
    });
}

/// The folder the feed saves into: its name, and a button to choose another.
fn folder_row(ui: &mut Ui, locale: Locale, name: &str, saving: bool, outcome: &mut Outcome) {
    let p = theme::pal();
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // The button first (it sits at the right edge), then the pill fills what is left.
            if ui
                .add_enabled(
                    !saving,
                    egui::Button::new(
                        RichText::new(Msg::PrefsChooseFolder.text(locale)).font(theme::text(12.0)),
                    )
                    .min_size(Vec2::new(0.0, 30.0)),
                )
                .clicked()
            {
                outcome.choose_folder = true;
            }
            let (rect, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width().max(120.0), 32.0),
                Sense::hover(),
            );
            ui.painter()
                .rect_filled(rect, CornerRadius::same(9), p.track.gamma_multiply(0.55));
            widgets::folder_icon(ui.painter(), Pos2::new(rect.left() + 20.0, rect.center().y));
            let clip = Rect::from_min_max(
                Pos2::new(rect.left() + 38.0, rect.top()),
                Pos2::new(rect.right() - 8.0, rect.bottom()),
            );
            let shown = widgets::fit_to_width(ui.painter(), name, &theme::text(12.0), clip.width());
            ui.painter().with_clip_rect(clip).text(
                Pos2::new(clip.left(), rect.center().y),
                egui::Align2::LEFT_CENTER,
                shown,
                theme::text(12.0),
                p.text,
            );
        });
    });
}

/// What to call a folder: put.io's top level, a name already known, "…" while it is being asked for,
/// or just its number when put.io could not say.
fn folder_name(app: &App, id: i64, locale: Locale) -> String {
    if id == 0 {
        return ROOT_NAME.to_owned();
    }
    match app.ui.folder_names.get(&id) {
        Some(Some(name)) => name.clone(),
        Some(None) => format!("#{id}"),
        None => Msg::FeedFolderLoading.text(locale),
    }
}

// ---- the question before deleting -----------------------------------------------------------

fn ask_to_delete(ctx: &egui::Context, app: &mut App) {
    let Some(id) = app.ui.feed_dialog.delete else {
        return;
    };
    let Some(feed) = app
        .state
        .feeds
        .items
        .iter()
        .find(|feed| feed.id == id)
        .cloned()
    else {
        // Gone already (deleted elsewhere, or the list was refreshed): nothing to ask about.
        app.ui.feed_dialog.delete = None;
        return;
    };
    let locale = app.state.locale;
    let title = putio_core::feeds::display_title(&feed);
    let (frame, backdrop) = widgets::dialog_frame();
    let mut confirmed = false;
    let mut cancelled = false;
    let response = egui::Modal::new(Id::unique("feed-delete"))
        .backdrop_color(backdrop)
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(ASK_WIDTH);
            ui.add(egui::Label::new(
                RichText::new(Msg::FeedDeleteTitle.text(locale)).font(theme::bold(15.0)),
            ));
            ui.add_space(6.0);
            ui.add(
                egui::Label::new(
                    RichText::new(Msg::FeedDeleteBody { title }.text(locale))
                        .font(theme::text(12.0))
                        .color(theme::pal().muted),
                )
                .wrap(),
            );
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::danger_button(ui, &Msg::FeedDeleteConfirm.text(locale)).clicked() {
                        confirmed = true;
                    }
                    if widgets::text_button(ui, &Msg::PrefsCancel.text(locale), false).clicked() {
                        cancelled = true;
                    }
                });
            });
        });
    // Escape and a click outside are the safe answer: nothing is deleted by accident.
    if confirmed {
        app.ui.feed_dialog.delete = None;
        app.dispatch(ctx, Event::FeedDeleteRequested(id));
    } else if cancelled || response.should_close() {
        app.ui.feed_dialog.delete = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::test_support::{accessible_names, rendered_text, text_boxes};

    const LOCALES: [Locale; 3] = [Locale::En, Locale::De, Locale::Es];

    fn editing(id: Option<i64>) -> Editing {
        let mut form = FeedForm::new(0);
        form.url = "https://releases.example.invalid/rss/weekly.xml".into();
        form.title = "Weekly releases".into();
        Editing {
            id,
            form,
            problem: None,
            focus_url: false,
            typing: None,
        }
    }

    /// The form's fields as the card draws them, in a column the width of the card.
    fn drawn(
        locale: Locale,
        id: Option<i64>,
        saving: bool,
    ) -> Vec<crate::ui::test_support::TextBox> {
        text_boxes(|ui| {
            ui.set_width(CONTENT_WIDTH);
            let mut state = editing(id);
            fields(
                ui,
                locale,
                "TV shows",
                &mut state,
                saving,
                &mut Outcome::default(),
            );
        })
    }

    #[test]
    fn no_text_of_the_form_sticks_out_of_the_card_in_any_language() {
        for locale in LOCALES {
            for id in [None, Some(3)] {
                let boxes = drawn(locale, id, false);
                assert!(
                    boxes.len() >= 8,
                    "{locale:?}: the form drew {} texts",
                    boxes.len()
                );
                for text in &boxes {
                    assert!(
                        text.visible.left() >= -0.5 && text.visible.right() <= CONTENT_WIDTH + 0.5,
                        "{locale:?} ({id:?}): {:?} leaves the card ({:?})",
                        text.text,
                        text.visible
                    );
                    // Nothing is cut off by a clip on its way: a label shows whole.
                    if text.text.chars().count() > 12 && !text.text.contains("://") {
                        assert!(
                            text.full.right() <= CONTENT_WIDTH + 0.5,
                            "{locale:?}: {:?} would be cut",
                            text.text
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_form_is_in_the_users_language() {
        for (locale, words) in [
            (
                Locale::En,
                ["Feed address", "Save to", "Only items containing"],
            ),
            (
                Locale::De,
                ["Feed-Adresse", "Speichern in", "Nur Einträge mit"],
            ),
            (
                Locale::Es,
                ["Dirección del feed", "Guardar en", "Solo elementos con"],
            ),
        ] {
            let text = rendered_text(|ui| {
                ui.set_width(CONTENT_WIDTH);
                fields(
                    ui,
                    locale,
                    "x",
                    &mut editing(None),
                    false,
                    &mut Outcome::default(),
                );
            });
            for word in words {
                assert!(
                    text.iter().any(|t| t == word),
                    "{locale:?}: {word:?} in {text:?}"
                );
            }
        }
    }

    #[test]
    fn skipping_what_the_feed_already_has_is_only_offered_when_adding() {
        for locale in LOCALES {
            let adding = rendered_text(|ui| {
                ui.set_width(CONTENT_WIDTH);
                fields(
                    ui,
                    locale,
                    "x",
                    &mut editing(None),
                    false,
                    &mut Outcome::default(),
                );
            });
            let changing = rendered_text(|ui| {
                ui.set_width(CONTENT_WIDTH);
                fields(
                    ui,
                    locale,
                    "x",
                    &mut editing(Some(3)),
                    false,
                    &mut Outcome::default(),
                );
            });
            let skip = Msg::FeedSkipExisting.text(locale);
            assert!(adding.contains(&skip), "{locale:?}");
            assert!(!changing.contains(&skip), "{locale:?}");
            // The other housekeeping choice is always there.
            assert!(
                changing.contains(&Msg::FeedDeleteOld.text(locale)),
                "{locale:?}"
            );
        }
    }

    #[test]
    fn the_name_field_shows_what_an_empty_name_becomes() {
        for locale in LOCALES {
            let mut state = editing(None);
            state.form.title.clear();
            let with_address = rendered_text(|ui| {
                ui.set_width(CONTENT_WIDTH);
                fields(ui, locale, "x", &mut state, false, &mut Outcome::default());
            });
            assert!(
                with_address.iter().any(|t| t == "releases.example.invalid"),
                "{locale:?}"
            );
            state.form.url.clear();
            let without = rendered_text(|ui| {
                ui.set_width(CONTENT_WIDTH);
                fields(ui, locale, "x", &mut state, false, &mut Outcome::default());
            });
            assert!(
                without.contains(&Msg::FeedTitleHint.text(locale)),
                "{locale:?}"
            );
        }
    }

    #[test]
    fn the_fields_are_named_for_a_screen_reader() {
        let names = accessible_names(|ui| {
            ui.set_width(CONTENT_WIDTH);
            fields(
                ui,
                Locale::En,
                "x",
                &mut editing(None),
                false,
                &mut Outcome::default(),
            );
        });
        // Every text field is announced with the label above it, not with its placeholder.
        for label in [
            "Feed address",
            "Name",
            "Only items containing",
            "Skip items containing",
        ] {
            assert!(
                names
                    .iter()
                    .any(|(role, name)| role == "TextInput" && name == label)
                    || names.iter().any(|(_, name)| name == label),
                "{label:?} in {names:?}"
            );
        }
    }

    #[test]
    fn the_two_footer_buttons_fit_side_by_side_in_every_language() {
        for locale in LOCALES {
            for primary in [Msg::FeedsAdd, Msg::FeedSave, Msg::FeedSaving] {
                let widths = std::cell::Cell::new(0.0);
                text_boxes(|ui| {
                    ui.set_width(CONTENT_WIDTH);
                    let a = widgets::text_button(ui, &Msg::PrefsCancel.text(locale), false)
                        .rect
                        .width();
                    let b = widgets::text_button(ui, &primary.text(locale), true)
                        .rect
                        .width();
                    widths.set(a + b + ui.spacing().item_spacing.x);
                });
                assert!(
                    widths.get() <= CONTENT_WIDTH,
                    "{locale:?} {primary:?}: the buttons need {} of {CONTENT_WIDTH}",
                    widths.get()
                );
            }
        }
    }

    #[test]
    fn the_delete_question_and_its_buttons_fit_in_every_language() {
        for locale in LOCALES {
            let width = std::cell::Cell::new(0.0);
            let boxes = text_boxes(|ui| {
                ui.set_width(ASK_WIDTH);
                ui.add(
                    egui::Label::new(
                        RichText::new(
                            Msg::FeedDeleteBody {
                                title: "A feed with a very long name that keeps going and going"
                                    .into(),
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
                let b = widgets::danger_button(ui, &Msg::FeedDeleteConfirm.text(locale))
                    .rect
                    .width();
                width.set(a + b + ui.spacing().item_spacing.x);
            });
            assert!(
                width.get() <= ASK_WIDTH,
                "{locale:?}: the buttons need {}",
                width.get()
            );
            for text in &boxes {
                assert!(
                    text.full.right() <= ASK_WIDTH + 0.5,
                    "{locale:?}: {:?}",
                    text.text
                );
            }
        }
    }

    #[test]
    fn a_problem_is_said_in_the_users_language() {
        for locale in LOCALES {
            assert_eq!(
                problem_text(FeedProblem::UrlMissing, locale),
                Msg::FeedErrUrlMissing.text(locale)
            );
            assert_eq!(
                problem_text(FeedProblem::UrlInvalid, locale),
                Msg::FeedErrUrlInvalid.text(locale)
            );
        }
    }
}
