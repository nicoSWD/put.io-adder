//! The transfers list: one card per transfer, with a slim progress bar.

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, Layout, Pos2, Rect, RichText, Sense, Vec2,
};
use putio_api::{Transfer, TransferStatus};
use putio_core::format;
use putio_core::i18n::Msg;
use putio_core::state::Event;
use putio_core::transfers::{self, SortKey};
use putio_core::update::UpdateStatus;

use super::{icons, theme};
use crate::app::App;

const ROW_HEIGHT: f32 = 56.0;
const ROW_GAP: f32 = 6.0;
const TOOLBAR_HEIGHT: f32 = 28.0;
/// Space kept free at the right of the cards for the scroll bar.
const SCROLLBAR_ROOM: f32 = 8.0;
/// Room kept free on the right of a row for the size and percentage.
const RIGHT_COLUMN: f32 = 84.0;

pub(crate) fn show(ui: &mut egui::Ui, app: &mut App) {
    let ctx = ui.ctx().clone();
    toolbar(ui, app, &ctx);

    // A sorted copy for display: the core keeps its own (default) order.
    let mut rows = app.state.transfers.clone();
    transfers::sort(&mut rows, app.ui.sort, app.ui.ascending);

    if rows.is_empty() {
        empty(ui, app);
        return;
    }

    let mut action: Option<Event> = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = ROW_GAP;
            for transfer in &rows {
                if let Some(event) = card(ui, app, transfer) {
                    action = Some(event);
                }
            }
            ui.add_space(4.0);
        });
    if let Some(event) = action {
        app.dispatch(&ctx, event);
    }
}

/// The row above the list: an update notice on the left, the sort menu on the right.
fn toolbar(ui: &mut egui::Ui, app: &mut App, ctx: &egui::Context) {
    let locale = app.state.locale;
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), TOOLBAR_HEIGHT),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.label(
                RichText::new(Msg::TransfersTitle.text(locale))
                    .font(theme::bold(13.0))
                    .color(theme::TEXT),
            );
            count_pill(ui, app.state.transfers.len());
            if let UpdateStatus::Available { version, .. } = app.state.update.clone() {
                ui.add_space(8.0);
                ui.label(
                    RichText::new(Msg::UpdateAvailable { version }.text(locale))
                        .font(theme::bold(11.5))
                        .color(theme::TEXT),
                );
                if ui
                    .link(RichText::new(Msg::UpdateOpenPage.text(locale)).font(theme::text(11.5)))
                    .clicked()
                {
                    app.dispatch(ctx, Event::OpenReleasePage);
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                sort_menu(ui, app);
            });
        },
    );
}

/// A small rounded number next to the title.
fn count_pill(ui: &mut egui::Ui, count: usize) {
    let galley = ui
        .painter()
        .layout_no_wrap(count.to_string(), theme::bold(10.5), theme::TEXT);
    let size = Vec2::new((galley.size().x + 14.0).max(22.0), 18.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(9), theme::YELLOW);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, theme::TEXT);
}

fn sort_label(key: SortKey) -> Msg {
    match key {
        SortKey::Default => Msg::SortStatus,
        SortKey::Name => Msg::ColName,
        SortKey::Progress => Msg::SortProgress,
        SortKey::Speed => Msg::ColSpeed,
        SortKey::Eta => Msg::ColEta,
        SortKey::Size => Msg::ColSize,
    }
}

fn sort_menu(ui: &mut egui::Ui, app: &mut App) {
    let locale = app.state.locale;
    let arrow = if app.ui.ascending { "▲" } else { "▼" };
    let current = format!(
        "{}: {}  {arrow}",
        Msg::SortBy.text(locale),
        sort_label(app.ui.sort).text(locale)
    );
    let button = ui.add(
        egui::Button::new(
            RichText::new(current)
                .font(theme::text(11.0))
                .color(theme::MUTED),
        )
        .fill(Color32::TRANSPARENT)
        .stroke(egui::Stroke::NONE),
    );
    let button = button.on_hover_cursor(egui::CursorIcon::PointingHand);
    egui::Popup::menu(&button)
        .align(egui::RectAlign::BOTTOM_END)
        .width(170.0)
        .show(|ui| {
            for key in [
                SortKey::Default,
                SortKey::Name,
                SortKey::Progress,
                SortKey::Speed,
                SortKey::Eta,
                SortKey::Size,
            ] {
                let selected = app.ui.sort == key;
                let marker = if selected {
                    if app.ui.ascending { "▲  " } else { "▼  " }
                } else {
                    "     "
                };
                if ui
                    .selectable_label(
                        selected,
                        format!("{marker}{}", sort_label(key).text(locale)),
                    )
                    .clicked()
                {
                    if selected {
                        app.ui.ascending = !app.ui.ascending;
                    } else {
                        app.ui.sort = key;
                        app.ui.ascending = true;
                    }
                }
            }
        });
}

fn empty(ui: &mut egui::Ui, app: &App) {
    let locale = app.state.locale;
    let area = ui.available_rect_before_wrap();
    let center = Pos2::new(
        area.center().x,
        area.top() + (area.height() * 0.38).min(120.0),
    );
    icons::empty_badge(ui.painter(), center);
    ui.painter().text(
        center + Vec2::new(0.0, 46.0),
        Align2::CENTER_CENTER,
        Msg::EmptyTitle.text(locale),
        theme::bold(15.0),
        theme::TEXT,
    );
    let hint = ui.painter().layout(
        Msg::EmptyHint.text(locale),
        theme::text(11.5),
        theme::MUTED,
        340.0,
    );
    let pos = Pos2::new(center.x - hint.size().x / 2.0, center.y + 64.0);
    ui.painter().galley(pos, hint, theme::MUTED);
}

/// One transfer. Returns the action the user asked for, if any. Everything is keyed by the transfer
/// *id*, never by row position: the list can change between opening a menu and clicking an item.
#[allow(clippy::too_many_lines)] // one card is drawn top to bottom, left to right
fn card(ui: &mut egui::Ui, app: &mut App, transfer: &Transfer) -> Option<Event> {
    let locale = app.state.locale;
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width() - SCROLLBAR_ROOM, ROW_HEIGHT),
        Sense::click(),
    );
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let selected = app.ui.selected == Some(transfer.id);
    let hover =
        ui.ctx()
            .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.12);
    let fill = if selected {
        theme::CARD_SELECTED
    } else {
        mix(theme::CARD, theme::CARD_HOVER, hover)
    };

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(10), fill);
    if selected {
        painter.rect_filled(
            Rect::from_min_size(
                rect.min + Vec2::new(0.0, 12.0),
                Vec2::new(3.0, rect.height() - 24.0),
            ),
            CornerRadius::same(2),
            theme::YELLOW,
        );
    }
    icons::status_badge(
        &painter,
        Pos2::new(rect.left() + 28.0, rect.center().y),
        transfer.status,
    );

    let text_left = rect.left() + 52.0;
    let text_right = rect.right() - RIGHT_COLUMN;
    let name_area = Rect::from_min_max(
        Pos2::new(text_left, rect.top()),
        Pos2::new(text_right, rect.bottom()),
    );
    let name = if transfer.name.is_empty() {
        "…"
    } else {
        transfer.name.as_str()
    };
    painter.with_clip_rect(name_area).text(
        Pos2::new(text_left, rect.top() + 11.0),
        Align2::LEFT_TOP,
        name,
        theme::bold(12.5),
        theme::TEXT,
    );

    let failed = transfer.status == TransferStatus::Error;
    let detail_color = if failed { theme::ERROR } else { theme::MUTED };
    painter.with_clip_rect(name_area).text(
        Pos2::new(text_left, rect.top() + 30.0),
        Align2::LEFT_TOP,
        format::detail(transfer, locale),
        theme::text(10.5),
        detail_color,
    );

    // Right column: size, and the percentage while the transfer is running.
    let right = rect.right() - 16.0;
    painter.text(
        Pos2::new(right, rect.top() + 11.0),
        Align2::RIGHT_TOP,
        format::bytes(transfer.size),
        theme::text(11.0),
        theme::MUTED,
    );
    let running = transfer.status.is_pending();
    if running {
        let percent = ui.ctx().animate_value_with_time(
            response.id.with("percent"),
            f32::from(transfer.percent()),
            0.4,
        );
        // A queued transfer has no progress to report yet: only its (empty) bar.
        let queued = matches!(
            transfer.status,
            TransferStatus::Waiting | TransferStatus::InQueue
        );
        if !queued || transfer.percent() > 0 {
            painter.text(
                Pos2::new(right, rect.top() + 28.0),
                Align2::RIGHT_TOP,
                format!("{percent:.0}%"),
                theme::bold(12.0),
                theme::TEXT,
            );
        }
        let bar = Rect::from_min_max(
            Pos2::new(text_left, rect.bottom() - 10.0),
            Pos2::new(text_right + 6.0, rect.bottom() - 6.0),
        );
        icons::progress_bar(&painter, bar, percent / 100.0, theme::YELLOW);
    }

    if response.clicked() {
        app.ui.selected = Some(transfer.id);
    }
    let mut action = response
        .double_clicked()
        .then_some(Event::OpenTransferOnPutio(transfer.id));
    response.context_menu(|ui| {
        if ui.button(Msg::MenuOpenOnPutio.text(locale)).clicked() {
            action = Some(Event::OpenTransferOnPutio(transfer.id));
            ui.close();
        }
        if transfer.file_id.is_some()
            && ui
                .button(
                    Msg::MenuStream {
                        name: shorten(&transfer.name),
                    }
                    .text(locale),
                )
                .clicked()
        {
            action = Some(Event::StreamRequested(transfer.id));
            ui.close();
        }
        if !transfer.status.is_finished()
            && ui
                .button(
                    Msg::MenuCancelTransfer {
                        name: shorten(&transfer.name),
                    }
                    .text(locale),
                )
                .clicked()
        {
            action = Some(Event::CancelRequested(transfer.id));
            ui.close();
        }
    });
    action
}

/// Keeps menu entries a sensible width.
fn shorten(name: &str) -> String {
    const MAX: usize = 32;
    if name.chars().count() <= MAX {
        return name.to_owned();
    }
    let head: String = name.chars().take(MAX - 1).collect();
    format!("{head}…")
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // each channel stays within 0..=255
fn mix(from: Color32, to: Color32, amount: f32) -> Color32 {
    let lerp = |a: u8, b: u8| {
        (f32::from(a) + (f32::from(b) - f32::from(a)) * amount.clamp(0.0, 1.0)).round() as u8
    };
    Color32::from_rgb(
        lerp(from.r(), to.r()),
        lerp(from.g(), to.g()),
        lerp(from.b(), to.b()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_names_are_shortened_for_menus() {
        assert_eq!(shorten("short"), "short");
        let long = shorten(&"a".repeat(80));
        assert_eq!(long.chars().count(), 32);
        assert!(long.ends_with('…'));
    }

    #[test]
    fn mixing_colours_stays_between_the_ends() {
        let a = Color32::from_rgb(0, 0, 0);
        let b = Color32::from_rgb(100, 200, 50);
        assert_eq!(mix(a, b, 0.0), a);
        assert_eq!(mix(a, b, 1.0), b);
        assert_eq!(mix(a, b, 2.0), b);
        assert_eq!(mix(a, b, 0.5), Color32::from_rgb(50, 100, 25));
    }
}
