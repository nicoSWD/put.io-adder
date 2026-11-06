//! The *Add a link* card: paste a magnet link or the address of a file on the web, and put.io fetches it.
//!
//! A modal like the other cards. It stays open while put.io is asked and shows what it answers (an error is
//! not lost with a notice that fades), closes when the transfer has been added, and cannot be closed while
//! the request is on its way. Pasting a link anywhere in the window (⌘V / Ctrl+V with no field in use) opens
//! it with the link filled in, and nothing is added until the user presses *Add*.

use eframe::egui::{self, Align, Id, Key, Layout, RichText};
use putio_core::i18n::{Locale, Msg};
use putio_core::link::{self, MAX_LINK_CHARS};
use putio_core::state::{Event, LinkAdd};

use super::{icons, prefs, theme, widgets};
use crate::app::App;

/// Width of the content inside the card (the window is 560 wide).
const WIDTH: f32 = 420.0;

/// What the card has in it.
#[derive(Debug, Default)]
pub(crate) struct LinkUi {
    open: bool,
    text: String,
    /// The field takes the keyboard when the card opens.
    focus: bool,
    /// The field that has the keyboard, so Escape can leave it before it closes the card.
    typing: Option<Id>,
}

/// Open the card, with `text` in the field (empty for none).
pub(crate) fn open(app: &mut App, ctx: &egui::Context, text: &str) {
    app.ui.link_ui = LinkUi {
        open: true,
        text: text.trim().chars().take(MAX_LINK_CHARS).collect(),
        focus: true,
        typing: None,
    };
    app.dispatch(ctx, Event::AddLinkDismissed);
}

/// Debug builds only: the card (`add-link`, `add-link-failed`) for a preview.
#[cfg(debug_assertions)]
pub(crate) fn demo(app: &mut App, ctx: &egui::Context, view: &str) {
    match view {
        "add-link" => open(app, ctx, ""),
        "add-link-failed" => {
            open(
                app,
                ctx,
                "https://files.example.invalid/releases/some.very.long.release.name.2160p.mkv?key=SECRET",
            );
            app.state.link_add = LinkAdd::Failed(
                "put.io could not download that address: the server answered 403.".into(),
            );
        }
        _ => {}
    }
}

/// The button at the right of the tab bar while the transfers are showing.
pub(crate) fn button(ui: &mut egui::Ui, app: &mut App) {
    let label = Msg::TabAddLink.text(app.state.locale);
    if widgets::pill(ui, &label, icons::Glyph::Plus, true).clicked() {
        open(app, &ui.ctx().clone(), "");
    }
}

/// What a paste should open the card with: the pasted text when it is a link, nothing otherwise (a
/// paste of anything else is none of this card's business).
fn paste_to_open(text: &str) -> Option<String> {
    link::parse(text).is_ok().then(|| text.trim().to_owned())
}

/// A paste with no field in use and nothing else open is a link being handed over: show the card.
pub(crate) fn handle_paste(ctx: &egui::Context, app: &mut App) {
    if !app.state.is_signed_in()
        || anything_open(app)
        || ctx.memory(|memory| memory.focused().is_some())
    {
        return;
    }
    let pasted = ctx.input(|input| {
        input.events.iter().find_map(|event| match event {
            egui::Event::Paste(text) => paste_to_open(text),
            _ => None,
        })
    });
    if let Some(text) = pasted {
        open(app, ctx, &text);
    }
}

/// Whether a card or the preferences are in the way.
fn anything_open(app: &App) -> bool {
    app.ui.link_ui.open
        || app.ui.prefs_open
        || app.ui.file_dialog.is_open()
        || app.ui.feed_dialog.is_open()
        || app.state.sharing.is_some()
        || app.state.friends_card.is_some()
        || app.state.detail.is_some()
}

pub(crate) fn show(ctx: &egui::Context, app: &mut App) {
    if !app.ui.link_ui.open {
        return;
    }
    if !app.state.is_signed_in() {
        app.ui.link_ui = LinkUi::default();
        return;
    }
    let status = app.state.link_add.clone();
    if status == LinkAdd::Done {
        // Added: the card has done its job.
        app.ui.link_ui = LinkUi::default();
        app.dispatch(ctx, Event::AddLinkDismissed);
        return;
    }
    let locale = app.state.locale;
    let working = status == LinkAdd::Working;
    let mut card = std::mem::take(&mut app.ui.link_ui);
    let (frame, backdrop) = widgets::dialog_frame();
    let mut submit = false;
    let mut close = false;
    let response = egui::Modal::new(Id::unique("add-link"))
        .backdrop_color(backdrop)
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(WIDTH);
            close = prefs::header(ui, &Msg::LinkAddTitle.text(locale), false, locale).1;
            submit = body(ui, &mut card, &status, locale);
        });
    // A click on the backdrop does not count: something may have been pasted.
    let escape = response.should_close() && !response.backdrop_response.clicked();
    if (close || escape) && !working {
        if escape && let Some(field) = card.typing.take() {
            // The first Escape leaves the field it was pressed in.
            ctx.memory_mut(|memory| memory.surrender_focus(field));
        } else {
            app.dispatch(ctx, Event::AddLinkDismissed);
            return; // closed: the card is not put back
        }
    }
    if submit && !working {
        app.dispatch(ctx, Event::AddLinkRequested(card.text.clone()));
    }
    app.ui.link_ui = card;
}

/// The hint, the field, what went wrong and the buttons. Returns whether *Add* was asked for.
fn body(ui: &mut egui::Ui, card: &mut LinkUi, status: &LinkAdd, locale: Locale) -> bool {
    let working = *status == LinkAdd::Working;
    let mut submit = false;
    ui.add(
        egui::Label::new(
            RichText::new(Msg::LinkAddHint.text(locale))
                .font(theme::text(12.0))
                .color(theme::pal().muted),
        )
        .wrap(),
    );
    ui.add_space(10.0);
    let field = ui
        .add_enabled_ui(!working, |ui| {
            widgets::text_field(
                ui,
                &mut card.text,
                &Msg::LinkFieldHint.text(locale),
                None,
                MAX_LINK_CHARS,
            )
        })
        .inner;
    if card.focus {
        field.request_focus();
        card.focus = false;
    }
    card.typing = field.has_focus().then_some(field.id);
    submit |= field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
    ui.add_space(8.0);
    if let LinkAdd::Failed(why) = status {
        ui.add(
            egui::Label::new(
                RichText::new(why)
                    .font(theme::text(11.0))
                    .color(theme::pal().error),
            )
            .wrap(),
        );
        ui.add_space(8.0);
    }
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::text_button(ui, &Msg::LinkAddButton.text(locale), true).clicked() {
                submit = true;
            }
            if widgets::text_button(ui, &Msg::PrefsCancel.text(locale), false).clicked() && !working
            {
                card.open = false;
            }
        });
    });
    submit
}

#[cfg(test)]
mod tests {
    use eframe::egui::{Rect, Vec2};

    use super::*;
    use crate::ui::test_support::{rendered_text, text_boxes};

    #[test]
    fn only_a_link_that_is_pasted_opens_the_card() {
        assert_eq!(
            paste_to_open("  https://example.com/a.mkv \n").as_deref(),
            Some("https://example.com/a.mkv")
        );
        for text in [
            "",
            "hello there",
            "ftp://example.com/a",
            "file:///etc/passwd",
        ] {
            assert_eq!(paste_to_open(text), None, "{text:?}");
        }
    }

    #[test]
    fn the_card_has_its_hint_field_and_buttons_in_every_language() {
        for locale in [Locale::En, Locale::De, Locale::Es] {
            let text = rendered_text(|ui| {
                body(ui, &mut LinkUi::default(), &LinkAdd::Idle, locale);
            });
            for wanted in [
                Msg::LinkAddHint.text(locale),
                Msg::LinkFieldHint.text(locale),
                Msg::LinkAddButton.text(locale),
                Msg::PrefsCancel.text(locale),
            ] {
                assert!(
                    text.contains(&wanted),
                    "{locale:?}: {wanted:?} missing in {text:?}"
                );
            }
        }
    }

    #[test]
    fn what_went_wrong_is_shown_and_a_long_pasted_address_stays_in_the_field() {
        let long = format!("https://example.com/{}", "a".repeat(400));
        for locale in [Locale::En, Locale::De, Locale::Es] {
            let mut used = Rect::NOTHING;
            let mut card = LinkUi {
                text: long.clone(),
                ..LinkUi::default()
            };
            let failed = LinkAdd::Failed(Msg::LinkErrNotALink.text(locale));
            let boxes = text_boxes(|ui| {
                ui.allocate_ui(Vec2::new(WIDTH, 400.0), |ui| {
                    body(ui, &mut card, &failed, locale);
                    used = ui.min_rect();
                });
            });
            assert!(used.width() <= WIDTH + 0.5, "{locale:?}: {used:?}");
            for text in &boxes {
                assert!(
                    text.visible.right() <= used.left() + WIDTH + 0.5,
                    "{locale:?}: {:?} sticks out ({:?})",
                    text.text,
                    text.visible
                );
            }
            assert!(
                boxes
                    .iter()
                    .any(|b| b.text == Msg::LinkErrNotALink.text(locale)),
                "{locale:?}: the error is shown"
            );
        }
    }
}
