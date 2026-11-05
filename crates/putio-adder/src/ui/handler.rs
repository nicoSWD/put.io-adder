//! The question "make put.io adder your default app for magnet links and .torrent files?".
//!
//! A dialog like the others. "Yes" asks the system to change the default (the platform crate does the
//! work, per system, off the UI thread). "Not now" only closes it for this run; "Don't ask again" is
//! remembered. Closing it any other way (Escape, a click outside) counts as "not now": the user never
//! agrees to anything by accident.

use eframe::egui::{self, RichText};
use putio_core::i18n::{Locale, Msg};
use putio_core::state::Event;

use super::icons::MenuIcon;
use super::{theme, widgets};
use crate::app::App;

/// Width of the content inside the card (the window is at least 560 wide).
const CONTENT_WIDTH: f32 = 400.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Yes,
    NotNow,
    Never,
}

pub(crate) fn show(ctx: &egui::Context, app: &mut App) {
    if !app.state.handler_prompt {
        return;
    }
    let locale = app.state.locale;
    let (frame, backdrop) = widgets::dialog_frame();
    let mut choice = None;
    let response = egui::Modal::new(egui::Id::new("default-app"))
        .backdrop_color(backdrop)
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(CONTENT_WIDTH);
            choice = body(ui, locale);
        });
    let choice = choice.or_else(|| response.should_close().then_some(Choice::NotNow));
    let event = choice.map(|choice| match choice {
        Choice::Yes => Event::DefaultHandlerRequested,
        Choice::NotNow => Event::DefaultHandlerPromptDismissed { forever: false },
        Choice::Never => Event::DefaultHandlerPromptDismissed { forever: true },
    });
    if let Some(event) = event {
        app.dispatch(ctx, event);
    }
}

fn body(ui: &mut egui::Ui, locale: Locale) -> Option<Choice> {
    let p = theme::pal();
    let mut choice = None;
    ui.spacing_mut().item_spacing.y = 10.0;
    ui.add(
        egui::Label::new(
            RichText::new(Msg::HandlerPromptTitle.text(locale))
                .font(theme::bold(17.0))
                .color(p.text),
        )
        .wrap(),
    );
    ui.add(
        egui::Label::new(
            RichText::new(Msg::HandlerPromptBody.text(locale))
                .font(theme::text(12.5))
                .color(p.muted),
        )
        .wrap(),
    );
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if widgets::action_button(ui, MenuIcon::Check, &Msg::HandlerYes.text(locale), true)
            .clicked()
        {
            choice = Some(Choice::Yes);
        }
        if widgets::text_button(ui, &Msg::HandlerNotNow.text(locale), false).clicked() {
            choice = Some(Choice::NotNow);
        }
    });
    ui.add_space(2.0);
    if ui
        .link(RichText::new(Msg::HandlerNever.text(locale)).font(theme::text(11.0)))
        .clicked()
    {
        choice = Some(Choice::Never);
    }
    choice
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::test_support::rendered_text;

    fn has(text: &[String], wanted: &str) -> bool {
        text.iter().any(|s| s == wanted)
    }

    #[test]
    fn the_question_names_magnet_links_and_torrent_files_and_offers_three_answers() {
        let text = rendered_text(|ui| {
            body(ui, Locale::En);
        });
        assert!(
            has(&text, "Open magnet links with put.io adder?"),
            "{text:?}"
        );
        assert!(
            text.iter()
                .any(|s| s.contains("magnet links and .torrent files")),
            "{text:?}"
        );
        for answer in ["Yes, make it the default", "Not now", "Don't ask again"] {
            assert!(has(&text, answer), "{answer}");
        }
    }

    #[test]
    fn the_question_is_translated() {
        for (locale, yes) in [
            (Locale::De, "Ja, als Standard festlegen"),
            (Locale::Es, "Sí, usarla por defecto"),
        ] {
            let text = rendered_text(|ui| {
                body(ui, locale);
            });
            assert!(has(&text, yes), "{locale:?}: {text:?}");
        }
    }

    #[test]
    fn nothing_is_chosen_until_the_user_clicks() {
        let mut chosen = Some(Choice::Yes);
        let _ = rendered_text(|ui| chosen = body(ui, Locale::En));
        assert_eq!(chosen, None);
    }
}
