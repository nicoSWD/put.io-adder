//! The transfers list: one card per transfer, with a slim progress bar.

use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, Sense, Vec2};
use putio_api::{Transfer, TransferStatus};
use putio_core::format;
use putio_core::i18n::{Locale, Msg};
use putio_core::state::Event;
use putio_core::transfers::{self, SortKey};

use super::icons::MenuIcon;
use super::menu::{self, Tint};
use super::{icons, theme, widgets};
use crate::app::App;

const ROW_HEIGHT: f32 = 56.0;
/// Width of the sort drop-down card.
const SORT_MENU_WIDTH: f32 = 236.0;
const ROW_GAP: f32 = 6.0;
/// Space kept free at the right of the cards for the scroll bar.
const SCROLLBAR_ROOM: f32 = 8.0;
/// Room kept free on the right of a row for the size, percentage and the "⋯" button.
const RIGHT_COLUMN: f32 = 122.0;
/// Where the size and percentage end, counted from the right edge of the card.
const SIZE_INSET: f32 = 46.0;
/// The "⋯" button: its centre counted from the right edge, and its size.
const MORE_INSET: f32 = 24.0;
const MORE_SIZE: f32 = 28.0;
/// Width of the card context menu.
const MENU_WIDTH: f32 = 252.0;

/// The list of transfers (the tab bar above it is [`super::tabs`]).
pub(crate) fn show(ui: &mut egui::Ui, app: &mut App) {
    let ctx = ui.ctx().clone();

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
            let clearable = transfers::clearable_ids(&rows).len();
            for transfer in &rows {
                if let Some(event) = card(ui, app, transfer, clearable) {
                    action = Some(event);
                }
            }
            ui.add_space(4.0);
        });
    if let Some(event) = action {
        app.dispatch(&ctx, event);
    }
}

fn sort_label(key: SortKey) -> Msg {
    match key {
        SortKey::Default => Msg::SortStatus,
        SortKey::Name => Msg::ColName,
        SortKey::Progress => Msg::SortProgress,
        SortKey::Speed => Msg::SortSpeed,
        SortKey::Eta => Msg::ColEta,
        SortKey::Size => Msg::ColSize,
    }
}

/// The sort control: a quiet pill showing the current order, and a card to change it.
pub(crate) fn sort_menu(ui: &mut egui::Ui, app: &mut App) {
    let p = theme::pal();
    let locale = app.state.locale;
    let text = format!(
        "{}: {}",
        Msg::SortBy.text(locale),
        sort_label(app.ui.sort).text(locale)
    );
    let response = sort_trigger(ui, &text, app.ui.ascending);

    let _ = p;
    let mut popup = egui::Popup::menu(&response)
        .align(egui::RectAlign::BOTTOM_END)
        .gap(6.0)
        .width(SORT_MENU_WIDTH)
        .frame(widgets::popup_frame())
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    if app.ui.open_once == Some(super::Overlay::Sort) {
        app.ui.open_once = None;
        popup = popup.open_memory(Some(true.into()));
    }
    popup.show(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for key in [
            SortKey::Default,
            SortKey::Name,
            SortKey::Progress,
            SortKey::Speed,
            SortKey::Eta,
            SortKey::Size,
        ] {
            if super::widgets::choice_row(ui, &sort_label(key).text(locale), app.ui.sort == key) {
                if app.ui.sort != key {
                    app.ui.sort = key;
                    app.ui.ascending = true;
                }
                ui.close();
            }
        }
        ui.add_space(8.0);
        let (line, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
        ui.painter()
            .rect_filled(line.shrink2(Vec2::new(8.0, 0.0)), 0.0, p.divider);
        ui.add_space(8.0);
        // The direction stays open after a change, so the result is visible straight away.
        let options = [
            (true, Msg::SortAscending.text(locale)),
            (false, Msg::SortDescending.text(locale)),
        ];
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            if let Some(ascending) = super::widgets::segmented(ui, &options, app.ui.ascending) {
                app.ui.ascending = ascending;
            }
        });
        ui.add_space(4.0);
    });
}

/// The pill that shows the current order and opens the drop-down.
///
/// It is painted, not an `egui::Button`: stock buttons grow a little when hovered, which shifts their text.
/// Here only the background and the colour change; the text never moves.
fn sort_trigger(ui: &mut egui::Ui, text: &str, ascending: bool) -> egui::Response {
    let p = theme::pal();
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), theme::text(11.0), p.muted);
    let size = Vec2::new(galley.size().x + 12.0 + 22.0, 26.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    // Painted by hand, so a screen reader is told what it is.
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, text));
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), p.card_hover);
    }
    let color = if response.hovered() { p.text } else { p.muted };
    let text_pos = Pos2::new(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(text_pos, galley, color);
    direction_arrow(
        ui.painter(),
        Pos2::new(rect.right() - 12.0, rect.center().y),
        ascending,
        color,
    );
    response
}

/// A small triangle: pointing up for ascending, down for descending.
fn direction_arrow(painter: &egui::Painter, center: Pos2, ascending: bool, color: Color32) {
    let (tip, base) = if ascending { (-3.0, 2.5) } else { (3.0, -2.5) };
    painter.add(egui::Shape::convex_polygon(
        vec![
            center + Vec2::new(0.0, tip),
            center + Vec2::new(-3.8, base),
            center + Vec2::new(3.8, base),
        ],
        color,
        egui::Stroke::NONE,
    ));
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
        theme::pal().text,
    );
    let hint = ui.painter().layout(
        Msg::EmptyHint.text(locale),
        theme::text(11.5),
        theme::pal().muted,
        340.0,
    );
    let pos = Pos2::new(center.x - hint.size().x / 2.0, center.y + 64.0);
    ui.painter().galley(pos, hint, theme::pal().muted);
}

/// What a context-menu entry does. Mapped to a core event with the transfer's *id*, never its row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    Details,
    Open,
    Download,
    Stream,
    CopyLink,
    CopySource,
    Retry,
    Cancel,
    Clear,
}

impl Act {
    /// The core event for this entry.
    fn event(self, id: i64) -> Event {
        match self {
            Self::Details => Event::TransferDetailsOpened(id),
            Self::Open => Event::OpenTransferOnPutio(id),
            Self::Download => Event::DownloadRequested(id),
            Self::Stream => Event::StreamRequested(id),
            Self::CopyLink => Event::CopyLinkRequested(id),
            Self::CopySource => Event::CopySourceRequested(id),
            Self::Retry => Event::RetryRequested(id),
            Self::Cancel => Event::CancelRequested(id),
            Self::Clear => Event::ClearRequested,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    icon: MenuIcon,
    label: Msg,
    tint: Tint,
    act: Act,
}

fn entry(icon: MenuIcon, label: Msg, act: Act) -> Entry {
    Entry {
        icon,
        label,
        tint: Tint::Normal,
        act,
    }
}

/// The context menu of a transfer, in groups (drawn with hairlines between them). Only what makes
/// sense for this transfer in its current state is offered.
fn entries(transfer: &Transfer, clearable: usize) -> Vec<Vec<Entry>> {
    let mut groups = Vec::new();

    let mut open = vec![
        entry(MenuIcon::Info, Msg::MenuDetails, Act::Details),
        entry(MenuIcon::External, Msg::MenuOpenOnPutio, Act::Open),
    ];
    if transfer.file_id.is_some() {
        open.push(entry(MenuIcon::Play, Msg::MenuStream, Act::Stream));
        open.push(entry(MenuIcon::Download, Msg::MenuDownload, Act::Download));
    }
    groups.push(open);

    let mut copy = Vec::new();
    if transfer.file_id.is_some() {
        copy.push(entry(MenuIcon::Link, Msg::MenuCopyLink, Act::CopyLink));
    }
    if let Some(source) = transfers::copyable_source(transfer) {
        let label = if source.is_magnet {
            Msg::MenuCopyMagnet
        } else {
            Msg::MenuCopySource
        };
        copy.push(entry(MenuIcon::Copy, label, Act::CopySource));
    }
    if !copy.is_empty() {
        groups.push(copy);
    }

    let mut manage = Vec::new();
    if transfer.status == TransferStatus::Error {
        manage.push(entry(MenuIcon::Refresh, Msg::MenuRetry, Act::Retry));
    }
    let (icon, label) = match transfer.status {
        TransferStatus::Seeding => (MenuIcon::Cancel, Msg::MenuStopSeeding),
        status if status.is_pending() => (MenuIcon::Cancel, Msg::MenuCancelTransfer),
        _ => (MenuIcon::Trash, Msg::MenuRemoveFromList),
    };
    manage.push(Entry {
        tint: Tint::Danger,
        ..entry(icon, label, Act::Cancel)
    });
    if clearable > 0 {
        manage.push(Entry {
            tint: Tint::Danger,
            ..entry(
                MenuIcon::Trash,
                Msg::MenuClearFinished { count: clearable },
                Act::Clear,
            )
        });
    }
    groups.push(manage);
    groups
}

/// The content of the card's menu (right-click, or the "⋯" button). Returns the entry that was chosen.
fn menu_body(
    ui: &mut egui::Ui,
    locale: Locale,
    transfer: &Transfer,
    clearable: usize,
) -> Option<Act> {
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut chosen = None;
    let name = if transfer.name.is_empty() {
        "…"
    } else {
        transfer.name.as_str()
    };
    menu::caption(ui, name);
    for group in entries(transfer, clearable) {
        menu::divider(ui);
        for entry in group {
            if menu::item(ui, entry.icon, &entry.label.text(locale), None, entry.tint) {
                chosen = Some(entry.act);
                ui.close();
            }
        }
    }
    chosen
}

/// The parts of a drawn card that react to the pointer.
struct Card {
    #[cfg_attr(not(test), allow(dead_code))] // the tests use it to find the card
    rect: Rect,
    body: egui::Response,
    more: egui::Response,
}

/// Draw one card at fixed positions: text and controls never move when the pointer arrives, only
/// colours change.
#[allow(clippy::too_many_lines)] // one card is drawn top to bottom, left to right
fn draw_card(ui: &mut egui::Ui, transfer: &Transfer, selected: bool, locale: Locale) -> Card {
    let p = theme::pal();
    let (rect, body) = ui.allocate_exact_size(
        Vec2::new(ui.available_width() - SCROLLBAR_ROOM, ROW_HEIGHT),
        Sense::click(),
    );
    let more_rect = Rect::from_center_size(
        Pos2::new(rect.right() - MORE_INSET, rect.center().y),
        Vec2::splat(MORE_SIZE),
    );
    // Added after the body, so it wins where they overlap.
    let more = ui.interact(more_rect, body.id.with("more"), Sense::click());
    let more = more.on_hover_cursor(egui::CursorIcon::PointingHand);
    let name = if transfer.name.is_empty() {
        "…"
    } else {
        transfer.name.as_str()
    };
    widgets::describe(
        &body,
        egui::Role::Button,
        &format!("{name}, {}", format::detail(transfer, locale)),
    );
    widgets::describe(
        &more,
        egui::Role::Button,
        &format!("{}: {name}", Msg::A11yTransferActions.text(locale)),
    );
    widgets::focus_ring(ui, &body, rect, CornerRadius::same(10));
    widgets::focus_ring(ui, &more, more_rect, CornerRadius::same(8));
    let body = if more.hovered() {
        body
    } else {
        body.on_hover_cursor(egui::CursorIcon::PointingHand)
    };

    let over = body.hovered() || more.hovered();
    let hover = ui
        .ctx()
        .animate_bool_with_time(body.id.with("hover"), over, 0.12);
    let failed = transfer.status == TransferStatus::Error;
    let fill = if selected {
        p.card_selected
    } else {
        mix(p.card, p.card_hover, hover)
    };

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(10), fill);
    if selected || failed {
        painter.rect_filled(
            Rect::from_min_size(
                rect.min + Vec2::new(0.0, 12.0),
                Vec2::new(3.0, rect.height() - 24.0),
            ),
            CornerRadius::same(2),
            if selected { theme::YELLOW } else { p.error },
        );
    }
    icons::status_badge(
        &painter,
        Pos2::new(rect.left() + 28.0, rect.center().y),
        transfer.status,
    );

    let text_left = rect.left() + 52.0;
    let text_right = rect.right() - RIGHT_COLUMN;
    let text_width = text_right - text_left;
    let name_font = theme::bold(12.5);
    let shown_name = widgets::fit_to_width(&painter, name, &name_font, text_width);
    painter.text(
        Pos2::new(text_left, rect.top() + 11.0),
        Align2::LEFT_TOP,
        shown_name,
        name_font,
        p.text,
    );

    let detail_font = theme::text(10.5);
    let detail = widgets::fit_to_width(
        &painter,
        &format::detail(transfer, locale),
        &detail_font,
        text_width,
    );
    painter.text(
        Pos2::new(text_left, rect.top() + 30.0),
        Align2::LEFT_TOP,
        detail,
        detail_font,
        if failed { p.error } else { p.muted },
    );

    // Right column: size, and the percentage while the transfer is running.
    let right = rect.right() - SIZE_INSET;
    painter.text(
        Pos2::new(right, rect.top() + 11.0),
        Align2::RIGHT_TOP,
        format::bytes(transfer.size, locale),
        theme::text(11.0),
        p.muted,
    );
    if transfer.status.is_pending() {
        let percent = ui.ctx().animate_value_with_time(
            body.id.with("percent"),
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
                p.accent_text,
            );
        }
        let bar = Rect::from_min_max(
            Pos2::new(text_left, rect.bottom() - 10.0),
            Pos2::new(text_right + 6.0, rect.bottom() - 6.0),
        );
        icons::progress_bar(&painter, bar, percent / 100.0, theme::YELLOW);
    }

    // The "⋯" button: fades in with the pointer, so a quiet list stays quiet.
    let visible = hover.max(if selected { 1.0 } else { 0.0 });
    if visible > 0.0 {
        if more.hovered() {
            painter.rect_filled(more_rect, CornerRadius::same(8), p.divider);
        }
        let color = if more.hovered() { p.text } else { p.muted };
        icons::dots(&painter, more_rect.center(), color.gamma_multiply(visible));
    }

    Card { rect, body, more }
}

/// One transfer. Returns the action the user asked for, if any. Everything is keyed by the transfer
/// *id*, never by row position: the list can change between opening a menu and clicking an item.
fn card(ui: &mut egui::Ui, app: &mut App, transfer: &Transfer, clearable: usize) -> Option<Event> {
    let locale = app.state.locale;
    let selected = app.ui.selected == Some(transfer.id);
    let Card { body, more, .. } = draw_card(ui, transfer, selected, locale);

    if body.clicked() || more.clicked() {
        app.ui.selected = Some(transfer.id);
    }
    let mut action = body
        .double_clicked()
        .then_some(Event::OpenTransferOnPutio(transfer.id));

    let mut chosen = None;
    egui::Popup::context_menu(&body)
        .frame(widgets::popup_frame())
        .width(MENU_WIDTH)
        .show(|ui| chosen = menu_body(ui, locale, transfer, clearable));

    let mut popup = egui::Popup::menu(&more)
        .align(egui::RectAlign::BOTTOM_END)
        .gap(4.0)
        .width(MENU_WIDTH)
        .frame(widgets::popup_frame());
    if app.ui.open_once == Some(super::Overlay::Card(transfer.id)) {
        app.ui.open_once = None;
        popup = popup.open_memory(Some(true.into()));
    }
    popup.show(|ui| chosen = chosen.or(menu_body(ui, locale, transfer, clearable)));

    if let Some(act) = chosen {
        action = Some(act.event(transfer.id));
    }
    action
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // each channel stays within 0..=255
pub(crate) fn mix(from: Color32, to: Color32, amount: f32) -> Color32 {
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
    fn mixing_colours_stays_between_the_ends() {
        let a = Color32::from_rgb(0, 0, 0);
        let b = Color32::from_rgb(100, 200, 50);
        assert_eq!(mix(a, b, 0.0), a);
        assert_eq!(mix(a, b, 1.0), b);
        assert_eq!(mix(a, b, 2.0), b);
        assert_eq!(mix(a, b, 0.5), Color32::from_rgb(50, 100, 25));
    }

    #[test]
    fn the_sort_trigger_does_not_move_its_text_when_hovered() {
        use crate::ui::test_support::text_with_and_without_hover;
        for ascending in [true, false] {
            let (resting, hovered) = text_with_and_without_hover(|ui| {
                let response = sort_trigger(ui, "Sort: Status", ascending);
                (response.rect, response.hovered())
            });
            assert_eq!(
                resting, hovered,
                "the sort label moved on hover (ascending: {ascending})"
            );
        }
    }

    #[test]
    fn the_sort_trigger_is_a_named_button_for_screen_readers() {
        let names = crate::ui::test_support::accessible_names(|ui| {
            sort_trigger(ui, "Sort: Status", true);
        });
        assert!(
            names.contains(&("Button".to_owned(), "Sort: Status".to_owned())),
            "{names:?}"
        );
    }

    fn sample(status: TransferStatus, file_id: Option<i64>, source: Option<&str>) -> Transfer {
        let mut t = Transfer {
            id: 9,
            name: "Some.Movie.2024".into(),
            percent_done: 40,
            ..Transfer::default()
        };
        t.status = status;
        t.file_id = file_id;
        t.source = source.map(str::to_owned);
        t
    }

    fn acts(transfer: &Transfer, clearable: usize) -> Vec<Vec<Act>> {
        entries(transfer, clearable)
            .into_iter()
            .map(|group| group.into_iter().map(|e| e.act).collect())
            .collect()
    }

    #[test]
    fn a_running_transfer_can_be_opened_and_cancelled_but_not_streamed_or_downloaded() {
        let t = sample(TransferStatus::Downloading, None, None);
        assert_eq!(
            acts(&t, 0),
            [vec![Act::Details, Act::Open], vec![Act::Cancel]]
        );
        let cancel = &entries(&t, 0)[1][0];
        assert_eq!(cancel.label, Msg::MenuCancelTransfer);
        assert_eq!(cancel.tint, Tint::Danger);
    }

    #[test]
    fn a_completed_transfer_offers_stream_copy_and_removal() {
        let t = sample(
            TransferStatus::Completed,
            Some(5),
            Some("https://x.test/a.torrent"),
        );
        assert_eq!(
            acts(&t, 3),
            [
                vec![Act::Details, Act::Open, Act::Stream, Act::Download],
                vec![Act::CopyLink, Act::CopySource],
                vec![Act::Cancel, Act::Clear],
            ]
        );
        let groups = entries(&t, 3);
        assert_eq!(groups[1][1].label, Msg::MenuCopySource);
        assert_eq!(groups[2][0].label, Msg::MenuRemoveFromList);
        assert_eq!(groups[2][1].label, Msg::MenuClearFinished { count: 3 });
    }

    #[test]
    fn a_magnet_source_is_labelled_as_a_magnet_link() {
        let t = sample(
            TransferStatus::Downloading,
            None,
            Some("magnet:?xt=urn:btih:abc"),
        );
        assert_eq!(entries(&t, 0)[1][0].label, Msg::MenuCopyMagnet);
    }

    #[test]
    fn a_seeding_transfer_stops_seeding_instead_of_being_cancelled() {
        let t = sample(TransferStatus::Seeding, Some(5), None);
        let manage = entries(&t, 0).pop().unwrap();
        assert_eq!(manage[0].label, Msg::MenuStopSeeding);
        assert_eq!(manage[0].act, Act::Cancel);
    }

    #[test]
    fn a_failed_transfer_can_be_retried_before_it_is_removed() {
        let t = sample(TransferStatus::Error, None, None);
        let manage = entries(&t, 0).pop().unwrap();
        assert_eq!(
            manage.iter().map(|e| e.act).collect::<Vec<_>>(),
            [Act::Retry, Act::Cancel]
        );
        assert_eq!(manage[1].label, Msg::MenuRemoveFromList);
        assert_eq!(manage[0].tint, Tint::Normal);
    }

    #[test]
    fn clearing_is_only_offered_when_something_can_be_cleared() {
        let t = sample(TransferStatus::Downloading, None, None);
        assert!(!acts(&t, 0).concat().contains(&Act::Clear));
        assert!(acts(&t, 1).concat().contains(&Act::Clear));
    }

    #[test]
    fn unsafe_sources_are_not_offered_for_copying() {
        let t = sample(TransferStatus::Downloading, None, Some("magnet:?x\ny"));
        assert!(!acts(&t, 0).concat().contains(&Act::CopySource));
    }

    #[test]
    fn every_action_targets_the_transfer_it_was_opened_for() {
        let event = |act: Act| act.event(9);
        assert!(matches!(
            event(Act::Details),
            Event::TransferDetailsOpened(9)
        ));
        assert!(matches!(event(Act::Open), Event::OpenTransferOnPutio(9)));
        assert!(matches!(event(Act::Stream), Event::StreamRequested(9)));
        assert!(matches!(event(Act::Download), Event::DownloadRequested(9)));
        assert!(matches!(event(Act::CopyLink), Event::CopyLinkRequested(9)));
        assert!(matches!(
            event(Act::CopySource),
            Event::CopySourceRequested(9)
        ));
        assert!(matches!(event(Act::Retry), Event::RetryRequested(9)));
        assert!(matches!(event(Act::Cancel), Event::CancelRequested(9)));
        assert!(matches!(event(Act::Clear), Event::ClearRequested));
    }

    #[test]
    fn a_card_does_not_move_its_text_when_hovered() {
        use crate::ui::test_support::text_with_and_without_hover;
        for status in [
            TransferStatus::Downloading,
            TransferStatus::Completed,
            TransferStatus::Error,
        ] {
            for selected in [false, true] {
                let t = sample(status, Some(5), None);
                let (resting, hovered) = text_with_and_without_hover(|ui| {
                    let card = draw_card(ui, &t, selected, Locale::En);
                    (card.rect, card.body.hovered())
                });
                assert_eq!(
                    resting, hovered,
                    "card text moved ({status:?}, selected {selected})"
                );
            }
        }
    }

    #[test]
    fn the_more_button_does_not_move_anything_when_hovered() {
        use crate::ui::test_support::text_with_and_without_hover;
        let t = sample(TransferStatus::Downloading, Some(5), None);
        let (resting, hovered) = text_with_and_without_hover(|ui| {
            let card = draw_card(ui, &t, false, Locale::En);
            let more = Rect::from_center_size(
                Pos2::new(card.rect.right() - MORE_INSET, card.rect.center().y),
                Vec2::splat(MORE_SIZE),
            );
            (more, card.more.hovered())
        });
        assert_eq!(resting, hovered);
    }

    #[test]
    fn a_card_and_its_actions_button_are_named_for_a_screen_reader() {
        let t = sample(TransferStatus::Downloading, Some(5), None);
        let names = crate::ui::test_support::accessible_names(|ui| {
            draw_card(ui, &t, false, Locale::En);
        });
        let button = |wanted: &dyn Fn(&str) -> bool| {
            names
                .iter()
                .any(|(role, name)| role == "Button" && wanted(name))
        };
        assert!(button(&|n| n.starts_with("Some.Movie.2024, ")), "{names:?}");
        assert!(button(&|n| n == "Actions: Some.Movie.2024"), "{names:?}");
    }
}
