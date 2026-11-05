//! The details panel: what put.io knows about one transfer (peers, ratio, tracker message, …).
//!
//! It is a dialog like Preferences. The rows come from [`putio_core::format::details`], which leaves out
//! anything the server did not send, so the panel never shows empty or made-up values. It reads the
//! live transfer list, so the numbers keep moving while the panel is open, and it closes itself when
//! the transfer disappears.

use eframe::egui::{self, Align2, Pos2, RichText, Sense, Vec2};
use putio_api::Transfer;
use putio_core::format::{self, DetailRow};
use putio_core::i18n::{Locale, Msg};

use super::{prefs, theme, widgets};
use crate::app::App;

/// Width of the content inside the card (the window is 560 wide).
const CONTENT_WIDTH: f32 = 456.0;
const ROW_HEIGHT: f32 = 31.0;

pub(crate) fn show(ctx: &egui::Context, app: &mut App) {
    let Some(id) = app.ui.details else {
        return;
    };
    let Some(transfer) = app.state.transfers.iter().find(|t| t.id == id).cloned() else {
        app.ui.details = None;
        return;
    };
    let locale = app.state.locale;
    let (frame, backdrop) = widgets::dialog_frame();
    let response = egui::Modal::new(egui::Id::new("transfer-details"))
        .backdrop_color(backdrop)
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(CONTENT_WIDTH);
            let (_, close) = prefs::header(ui, &Msg::DetailsTitle.text(locale), false);
            // Leaves room for the card's padding and the name above the rows.
            let height = (ui.ctx().content_rect().height() - 130.0).max(160.0);
            prefs::scroll_region(ui, height, |ui| {
                body(ui, &transfer, locale);
            });
            close
        });
    if response.inner || response.should_close() {
        app.ui.details = None;
    }
}

/// The name, then the facts in a rounded group.
fn body(ui: &mut egui::Ui, transfer: &Transfer, locale: Locale) {
    ui.spacing_mut().item_spacing.y = 8.0;
    let name = if transfer.name.is_empty() {
        "…"
    } else {
        transfer.name.as_str()
    };
    ui.add(
        egui::Label::new(
            RichText::new(name)
                .font(theme::bold(14.0))
                .color(theme::pal().text),
        )
        .wrap(),
    );
    let rows = format::details(transfer, locale);
    widgets::group(ui, |ui| {
        for (index, row) in rows.iter().enumerate() {
            if index > 0 {
                widgets::divider(ui);
            }
            if row.long {
                long_row(ui, row, locale);
            } else {
                short_row(ui, row, locale);
            }
        }
    });
    ui.add_space(4.0);
}

/// A label on the left and its value on the right, both painted at fixed positions.
fn short_row(ui: &mut egui::Ui, row: &DetailRow, locale: Locale) {
    let p = theme::pal();
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_HEIGHT), Sense::hover());
    ui.painter().text(
        Pos2::new(rect.left(), rect.center().y),
        Align2::LEFT_CENTER,
        row.label.text(locale),
        theme::text(12.0),
        p.muted,
    );
    ui.painter().text(
        Pos2::new(rect.right(), rect.center().y),
        Align2::RIGHT_CENTER,
        &row.value,
        theme::bold(12.5),
        p.text,
    );
}

/// Free text (an error, the tracker's message): the label, then the text wrapped underneath.
fn long_row(ui: &mut egui::Ui, row: &DetailRow, locale: Locale) {
    let p = theme::pal();
    ui.add_space(11.0);
    ui.label(
        RichText::new(row.label.text(locale))
            .font(theme::text(12.0))
            .color(p.muted),
    );
    ui.add_space(2.0);
    ui.add(
        egui::Label::new(
            RichText::new(&row.value)
                .font(theme::text(12.5))
                .color(p.text),
        )
        .wrap(),
    );
    ui.add_space(12.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::test_support::rendered_text;
    use putio_api::{TransferKind, TransferStatus};

    fn seeding() -> Transfer {
        Transfer {
            id: 1,
            name: "Spring.2019.Blender.Open.Movie".into(),
            status: TransferStatus::Seeding,
            kind: TransferKind::Torrent,
            size: 4 * 1024 * 1024 * 1024,
            peers: Some(14),
            availability: Some(100),
            current_ratio: Some(1.5),
            uploaded: Some(6 * 1024 * 1024 * 1024),
            tracker_message: Some("Announce OK".into()),
            ..Transfer::default()
        }
    }

    #[test]
    fn the_panel_shows_peers_ratio_and_the_tracker_message() {
        let t = seeding();
        let text = rendered_text(|ui| body(ui, &t, Locale::En));
        for expected in [
            "Spring.2019.Blender.Open.Movie",
            "Peers",
            "14",
            "Share ratio",
            "1.50",
            "Tracker message",
            "Announce OK",
        ] {
            assert!(
                text.iter().any(|s| s == expected),
                "missing {expected:?} in {text:?}"
            );
        }
    }

    #[test]
    fn what_the_server_did_not_send_is_not_shown() {
        let t = Transfer {
            id: 2,
            name: "Plain".into(),
            status: TransferStatus::Waiting,
            ..Transfer::default()
        };
        let text = rendered_text(|ui| body(ui, &t, Locale::En));
        for absent in ["Peers", "Share ratio", "Tracker message", "Availability"] {
            assert!(
                !text.iter().any(|s| s == absent),
                "{absent:?} should be absent"
            );
        }
    }

    #[test]
    fn the_panel_is_translated() {
        let t = seeding();
        let text = rendered_text(|ui| body(ui, &t, Locale::De));
        assert!(text.iter().any(|s| s == "Share-Ratio"), "{text:?}");
        assert!(text.iter().any(|s| s == "Tracker-Meldung"), "{text:?}");
    }
}
