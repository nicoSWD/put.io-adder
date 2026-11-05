//! The sign-in panel (device code flow).

use eframe::egui::{self, Align2, CornerRadius, RichText, Sense, Vec2};
use putio_core::i18n::Msg;
use putio_core::state::{Auth, Event, SignedOutReason};

use super::theme;
use crate::app::App;

pub(crate) fn show(ui: &mut egui::Ui, app: &mut App) {
    let ctx = ui.ctx().clone();
    let locale = app.state.locale;
    ui.add_space(14.0);
    ui.vertical_centered(|ui| match app.state.auth.clone() {
        Auth::Starting | Auth::RequestingCode => {
            ui.add_space(60.0);
            ui.add(
                egui::Spinner::new()
                    .size(22.0)
                    .color(theme::pal().accent_text),
            );
            ui.label(RichText::new(Msg::FetchingInfo.text(locale)).color(theme::pal().muted));
        }
        Auth::Linking(link) => {
            ui.label(
                RichText::new(Msg::LinkInstructions.text(locale))
                    .font(theme::text(12.5))
                    .color(theme::pal().muted),
            );
            ui.add_space(10.0);
            code_card(ui, &link.code);
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                let width = 150.0 + 110.0 + ui.spacing().item_spacing.x;
                ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
                if theme::primary_button(ui, Msg::LinkOpenButton.text(locale)).clicked() {
                    app.dispatch(&ctx, Event::OpenLinkPage);
                }
                if theme::secondary_button(ui, Msg::CopyCode.text(locale)).clicked() {
                    ctx.copy_text(link.code.clone());
                }
            });
            ui.add_space(16.0);
            // The status row already says what we are waiting for; a spinner is enough here.
            ui.add(
                egui::Spinner::new()
                    .size(16.0)
                    .color(theme::pal().accent_text),
            );
        }
        Auth::SignedOut(reason) => {
            ui.add_space(48.0);
            let note = match reason {
                Some(SignedOutReason::SessionExpired) => Some(Msg::AuthExpired),
                Some(SignedOutReason::CodeExpired) => Some(Msg::LinkExpired),
                Some(SignedOutReason::CodeRequestFailed) | None => None,
            };
            ui.label(
                RichText::new(note.unwrap_or(Msg::AuthRequired).text(locale))
                    .font(theme::text(13.0)),
            );
            ui.add_space(14.0);
            if theme::primary_button(ui, Msg::LinkRetryButton.text(locale)).clicked() {
                app.dispatch(&ctx, Event::SignInRequested);
            }
        }
        Auth::SignedIn => {}
    });
}

/// The device code, big and yellow on a card, one letter at a time so it is easy to read out.
fn code_card(ui: &mut egui::Ui, code: &str) {
    let size = Vec2::new(300.0, 84.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(14), theme::pal().card);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(14),
        egui::Stroke::new(2.0, theme::YELLOW),
        egui::StrokeKind::Inside,
    );
    let letters: Vec<char> = code.chars().collect();
    let advance = 34.0;
    let count = f32::from(u8::try_from(letters.len()).unwrap_or(8));
    let start = rect.center().x - advance * (count - 1.0) / 2.0;
    for (index, letter) in letters.iter().enumerate() {
        let offset = f32::from(u8::try_from(index).unwrap_or(0));
        let center = egui::pos2(start + advance * offset, rect.center().y);
        ui.painter().text(
            center,
            Align2::CENTER_CENTER,
            letter.to_string(),
            theme::bold(36.0),
            theme::pal().accent_text,
        );
    }
}
