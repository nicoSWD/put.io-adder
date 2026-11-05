//! The sign-in panel (device code flow).

use eframe::egui::{self, RichText};
use putio_core::i18n::Msg;
use putio_core::state::{Auth, Event, SignedOutReason};

use super::theme;
use crate::app::App;

pub(crate) fn show(ui: &mut egui::Ui, app: &mut App) {
    let ctx = ui.ctx().clone();
    let locale = app.state.locale;
    ui.add_space(6.0);
    ui.vertical_centered(|ui| match app.state.auth.clone() {
        Auth::Starting | Auth::RequestingCode => {
            ui.add_space(30.0);
            ui.spinner();
            ui.label(Msg::FetchingInfo.text(locale));
        }
        Auth::Linking(link) => {
            ui.label(Msg::LinkInstructions.text(locale));
            ui.add_space(8.0);
            ui.label(RichText::new(&link.code).monospace().size(34.0).strong());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(ui.available_width() / 2.0 - 100.0);
                if ui.button(Msg::LinkOpenButton.text(locale)).clicked() {
                    app.dispatch(&ctx, Event::OpenLinkPage);
                }
                if ui.button("Copy code").clicked() {
                    ctx.copy_text(link.code.clone());
                }
            });
            ui.add_space(10.0);
            ui.label(
                RichText::new(Msg::SigningIn.text(locale))
                    .color(theme::MUTED)
                    .small(),
            );
        }
        Auth::SignedOut(reason) => {
            ui.add_space(20.0);
            let note = match reason {
                Some(SignedOutReason::SessionExpired) => Some(Msg::AuthExpired),
                Some(SignedOutReason::CodeExpired) => Some(Msg::LinkExpired),
                Some(SignedOutReason::CodeRequestFailed) | None => None,
            };
            ui.label(note.unwrap_or(Msg::AuthRequired).text(locale));
            ui.add_space(10.0);
            if ui.button(Msg::LinkRetryButton.text(locale)).clicked() {
                app.dispatch(&ctx, Event::SignInRequested);
            }
        }
        Auth::SignedIn => {}
    });
}
