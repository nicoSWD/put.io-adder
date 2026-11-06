//! The feeds list: one card per RSS feed, in the family of the transfer cards.
//!
//! A feed is something put.io does on its own, so the list is calm: a round badge in the feed's state, its
//! name, the site it comes from and the words it filters by, and a pill only when something needs saying
//! (paused, or put.io could not read it). Everything is painted at fixed positions, so hovering changes
//! colours and nothing moves, and every text is cut to its room, so no language can push a card out of shape.

use eframe::egui::{self, Align2, CornerRadius, Pos2, Rect, Sense, Stroke, Ui, Vec2, WidgetType};
use putio_api::RssFeed;
use putio_core::feeds::{FeedStatus, display_title, host_of, status_of};
use putio_core::i18n::{Locale, Msg};
use putio_core::state::Event;

use super::icons::{self, MenuIcon};
use super::menu::{self, Tint};
use super::table::mix;
use super::{theme, widgets};
use crate::app::App;

const ROW_HEIGHT: f32 = 56.0;
const ROW_GAP: f32 = 6.0;
/// Space kept free at the right of the cards for the scroll bar.
const SCROLLBAR_ROOM: f32 = 8.0;
/// The "⋯" button: its centre counted from the right edge, and its size.
const MORE_INSET: f32 = 24.0;
const MORE_SIZE: f32 = 28.0;
/// Where the state pill ends, counted from the right edge of the card.
const PILL_INSET: f32 = 46.0;
/// Between the text and the pill.
const TEXT_GAP: f32 = 12.0;
/// Width of the card context menu.
const MENU_WIDTH: f32 = 252.0;

/// The list has loaded and there is nothing in it.
pub(crate) fn is_empty(app: &App) -> bool {
    app.state.feeds.loaded && app.state.feeds.items.is_empty()
}

pub(crate) fn show(ui: &mut Ui, app: &mut App) {
    let ctx = ui.ctx().clone();
    if app.state.feeds.items.is_empty() {
        if app.state.feeds.failed {
            failed(ui, app);
        } else if app.state.feeds.loaded {
            empty(ui, app);
        } else {
            loading(ui, app);
        }
        return;
    }

    let feeds = app.state.feeds.items.clone();
    let mut action: Option<(i64, Act)> = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = ROW_GAP;
            for feed in &feeds {
                if let Some(act) = card(ui, app, feed) {
                    action = Some((feed.id, act));
                }
            }
            ui.add_space(4.0);
        });
    if let Some((id, act)) = action
        && let Some(feed) = feeds.iter().find(|feed| feed.id == id)
    {
        apply(app, &ctx, feed, act);
    }
}

/// What a card's menu can do. Always aimed at the feed's *id*, never its row: the list can change
/// between opening a menu and clicking an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    Edit,
    Pause,
    Resume,
    CopyAddress,
    Delete,
}

fn apply(app: &mut App, ctx: &egui::Context, feed: &RssFeed, act: Act) {
    match act {
        Act::Edit => super::feed_form::open_edit(app, ctx, feed),
        Act::Pause => app.dispatch(
            ctx,
            Event::FeedPauseRequested {
                id: feed.id,
                paused: true,
            },
        ),
        Act::Resume => app.dispatch(
            ctx,
            Event::FeedPauseRequested {
                id: feed.id,
                paused: false,
            },
        ),
        Act::CopyAddress => app.dispatch(ctx, Event::FeedCopyAddressRequested(feed.id)),
        Act::Delete => app.ui.feed_dialog.ask_to_delete(feed.id),
    }
}

struct Entry {
    icon: MenuIcon,
    label: Msg,
    tint: Tint,
    act: Act,
}

/// The menu of a feed, in groups: what changes it, what copies it, what removes it.
fn entries(feed: &RssFeed) -> Vec<Vec<Entry>> {
    let (icon, label, act) = if feed.paused {
        (MenuIcon::Play, Msg::FeedMenuResume, Act::Resume)
    } else {
        (MenuIcon::Pause, Msg::FeedMenuPause, Act::Pause)
    };
    let mut copy = Vec::new();
    // Only a web address is offered for copying; the core refuses anything else too.
    if host_of(&feed.rss_source_url).is_some() {
        copy.push(Entry {
            icon: MenuIcon::Copy,
            label: Msg::FeedMenuCopyAddress,
            tint: Tint::Normal,
            act: Act::CopyAddress,
        });
    }
    let mut groups = vec![
        vec![
            Entry {
                icon,
                label,
                tint: Tint::Normal,
                act,
            },
            Entry {
                icon: MenuIcon::Edit,
                label: Msg::FeedMenuEdit,
                tint: Tint::Normal,
                act: Act::Edit,
            },
        ],
        copy,
        vec![Entry {
            icon: MenuIcon::Trash,
            label: Msg::FeedMenuDelete,
            tint: Tint::Danger,
            act: Act::Delete,
        }],
    ];
    groups.retain(|group| !group.is_empty());
    groups
}

fn menu_body(ui: &mut Ui, locale: Locale, feed: &RssFeed) -> Option<Act> {
    ui.spacing_mut().item_spacing.y = 0.0;
    menu::caption(ui, &display_title(feed));
    let mut chosen = None;
    for group in entries(feed) {
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

/// What a drawn card hands back for its interactions.
struct Card {
    body: egui::Response,
    more: egui::Response,
}

#[allow(clippy::too_many_lines)] // a card is painted top to bottom, and reads best that way
fn draw_card(ui: &mut Ui, feed: &RssFeed, selected: bool, locale: Locale) -> Card {
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
    let more = ui
        .interact(more_rect, body.id.with("more"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);

    let status = status_of(feed);
    let title = display_title(feed);
    let title = if title.is_empty() {
        "…".to_owned()
    } else {
        title
    };
    let host = host_of(&feed.rss_source_url).unwrap_or_default();
    let status_text = match status {
        FeedStatus::Active => Msg::FeedActive,
        FeedStatus::Paused => Msg::FeedPaused,
        FeedStatus::Problem => Msg::FeedProblem,
    }
    .text(locale);
    widgets::describe(
        &body,
        WidgetType::Button,
        &format!("{title}, {host}, {status_text}"),
    );
    widgets::describe(
        &more,
        WidgetType::Button,
        &format!("{}: {title}", Msg::A11yFeedActions.text(locale)),
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
    let fill = if selected {
        p.card_selected
    } else {
        mix(p.card, p.card_hover, hover)
    };
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(10), fill);
    if selected || status == FeedStatus::Problem {
        painter.rect_filled(
            Rect::from_min_size(
                rect.min + Vec2::new(0.0, 12.0),
                Vec2::new(3.0, rect.height() - 24.0),
            ),
            CornerRadius::same(2),
            if selected { theme::YELLOW } else { p.error },
        );
    }
    icons::feed_badge(
        &painter,
        Pos2::new(rect.left() + 28.0, rect.center().y),
        status,
    );

    // The pill first: how wide it is decides how much room the text has.
    let pill_left = state_pill(&painter, rect, status, locale);
    let text_left = rect.left() + 52.0;
    let text_right = pill_left.map_or(rect.right() - PILL_INSET, |left| left - TEXT_GAP);
    let text_width = (text_right - text_left).max(0.0);

    let title_font = theme::bold(12.5);
    let second_line = detail(feed, &title, &host);
    // A feed with nothing to say under its name is centred instead of leaving a gap.
    let (title_at, title_align) = if second_line.is_empty() {
        (Pos2::new(text_left, rect.center().y), Align2::LEFT_CENTER)
    } else {
        (Pos2::new(text_left, rect.top() + 11.0), Align2::LEFT_TOP)
    };
    painter.text(
        title_at,
        title_align,
        widgets::fit_to_width(&painter, &title, &title_font, text_width),
        title_font,
        p.text,
    );
    let detail_font = theme::text(10.5);
    painter.text(
        Pos2::new(text_left, rect.top() + 30.0),
        Align2::LEFT_TOP,
        widgets::fit_to_width(&painter, &second_line, &detail_font, text_width),
        detail_font,
        p.muted,
    );

    // The "⋯" button fades in with the pointer, so a quiet list stays quiet.
    let visible = hover.max(if selected { 1.0 } else { 0.0 });
    if visible > 0.0 {
        if more.hovered() {
            painter.rect_filled(more_rect, CornerRadius::same(8), p.divider);
        }
        let color = if more.hovered() { p.text } else { p.muted };
        icons::dots(&painter, more_rect.center(), color.gamma_multiply(visible));
    }
    Card { body, more }
}

/// The second line of a card: the site (unless the name already is the site), then the words the feed is
/// filtered by.
fn detail(feed: &RssFeed, title: &str, host: &str) -> String {
    let words = feed.keyword.as_deref().map(str::trim).unwrap_or_default();
    let host = if host.eq_ignore_ascii_case(title) {
        ""
    } else {
        host
    };
    match (host.is_empty(), words.is_empty()) {
        (true, true) => String::new(),
        (false, true) => host.to_owned(),
        (true, false) => words.to_owned(),
        (false, false) => format!("{host} · {words}"),
    }
}

/// The small pill for a feed that needs saying something about. Returns where it starts, or `None` when the
/// feed is simply running (nothing to say).
fn state_pill(
    painter: &egui::Painter,
    card: Rect,
    status: FeedStatus,
    locale: Locale,
) -> Option<f32> {
    let p = theme::pal();
    let (msg, fill, ink, edge) = match status {
        FeedStatus::Active => return None,
        FeedStatus::Paused => (Msg::FeedPaused, p.neutral_soft, p.muted, None),
        FeedStatus::Problem => (Msg::FeedProblem, p.error_soft, p.error, Some(p.error)),
    };
    let galley = painter.layout_no_wrap(msg.text(locale), theme::bold(10.5), ink);
    let size = Vec2::new(galley.size().x + 20.0, 20.0);
    let right = card.right() - PILL_INSET;
    let rect = Rect::from_min_size(
        Pos2::new(right - size.x, card.center().y - size.y / 2.0),
        size,
    );
    painter.rect_filled(rect, CornerRadius::same(10), fill);
    if let Some(edge) = edge {
        painter.rect_stroke(
            rect,
            CornerRadius::same(10),
            Stroke::new(1.0, edge),
            egui::StrokeKind::Inside,
        );
    }
    painter.galley(rect.center() - galley.size() / 2.0, galley, ink);
    Some(rect.left())
}

fn card(ui: &mut Ui, app: &mut App, feed: &RssFeed) -> Option<Act> {
    let locale = app.state.locale;
    let selected = app.ui.selected_feed == Some(feed.id);
    let Card { body, more } = draw_card(ui, feed, selected, locale);
    if body.clicked() || more.clicked() {
        app.ui.selected_feed = Some(feed.id);
    }
    if status_of(feed) == FeedStatus::Problem {
        body.clone()
            .on_hover_text(Msg::FeedProblemHint.text(locale));
    }
    let mut chosen = body.double_clicked().then_some(Act::Edit);

    egui::Popup::context_menu(&body)
        .frame(widgets::popup_frame())
        .width(MENU_WIDTH)
        .show(|ui| chosen = chosen.or(menu_body(ui, locale, feed)));
    let mut popup = egui::Popup::menu(&more)
        .align(egui::RectAlign::BOTTOM_END)
        .gap(4.0)
        .width(MENU_WIDTH)
        .frame(widgets::popup_frame());
    if app.ui.open_once == Some(super::Overlay::FeedCard(feed.id)) {
        app.ui.open_once = None;
        popup = popup.open_memory(Some(true.into()));
    }
    popup.show(|ui| chosen = chosen.or(menu_body(ui, locale, feed)));
    chosen
}

// ---- the other states of the list -----------------------------------------------------------

/// The big badge, a title and a hint, centred in the free area; returns the point below the hint.
fn message(
    ui: &mut Ui,
    badge: impl FnOnce(&egui::Painter, Pos2),
    title: String,
    hint: String,
) -> Pos2 {
    let area = ui.available_rect_before_wrap();
    let center = Pos2::new(
        area.center().x,
        area.top() + (area.height() * 0.34).min(110.0),
    );
    badge(ui.painter(), center);
    ui.painter().text(
        center + Vec2::new(0.0, 46.0),
        Align2::CENTER_CENTER,
        title,
        theme::bold(15.0),
        theme::pal().text,
    );
    let hint = ui
        .painter()
        .layout(hint, theme::text(11.5), theme::pal().muted, 340.0);
    let below = Pos2::new(center.x, center.y + 64.0 + hint.size().y);
    ui.painter().galley(
        Pos2::new(center.x - hint.size().x / 2.0, center.y + 64.0),
        hint,
        theme::pal().muted,
    );
    below
}

/// A button centred at `at`: the buttons of these states are the only ones that are not in a layout.
fn centered_button(
    ui: &mut Ui,
    at: Pos2,
    add: impl FnOnce(&mut Ui) -> egui::Response,
) -> egui::Response {
    let rect = Rect::from_center_size(at, Vec2::new(300.0, widgets::ACTION_HEIGHT));
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    add(&mut row)
}

fn empty(ui: &mut Ui, app: &mut App) {
    let ctx = ui.ctx().clone();
    let locale = app.state.locale;
    let below = message(
        ui,
        icons::empty_feeds_badge,
        Msg::FeedsEmptyTitle.text(locale),
        Msg::FeedsEmptyHint.text(locale),
    );
    let at = Pos2::new(below.x, below.y + 12.0 + widgets::ACTION_HEIGHT / 2.0);
    let label = Msg::FeedsAdd.text(locale);
    if centered_button(ui, at, |ui| widgets::text_button(ui, &label, true)).clicked() {
        super::feed_form::open_new(app, &ctx);
    }
}

fn loading(ui: &mut Ui, app: &App) {
    let locale = app.state.locale;
    let area = ui.available_rect_before_wrap();
    let center = Pos2::new(
        area.center().x,
        area.top() + (area.height() * 0.34).min(110.0),
    );
    let spinner = Rect::from_center_size(center, Vec2::splat(22.0));
    ui.put(
        spinner,
        egui::Spinner::new()
            .size(22.0)
            .color(theme::pal().accent_text),
    );
    ui.painter().text(
        center + Vec2::new(0.0, 32.0),
        Align2::CENTER_CENTER,
        Msg::FeedsLoading.text(locale),
        theme::text(12.0),
        theme::pal().muted,
    );
}

fn failed(ui: &mut Ui, app: &mut App) {
    let ctx = ui.ctx().clone();
    let locale = app.state.locale;
    let area = ui.available_rect_before_wrap();
    let center = Pos2::new(
        area.center().x,
        area.top() + (area.height() * 0.34).min(110.0),
    );
    ui.painter().text(
        center,
        Align2::CENTER_CENTER,
        Msg::FeedsLoadFailed.text(locale),
        theme::bold(13.0),
        theme::pal().text,
    );
    let label = Msg::FeedsRetry.text(locale);
    let at = Pos2::new(center.x, center.y + 20.0 + widgets::ACTION_HEIGHT / 2.0);
    if centered_button(ui, at, |ui| widgets::text_button(ui, &label, false)).clicked() {
        app.dispatch(&ctx, Event::FeedsRefreshRequested);
    }
}

// ---- the "Add feed" button of the tab bar ---------------------------------------------------

/// The button at the right of the tab bar. Painted at fixed positions (only the fill changes on hover).
pub(crate) fn add_button(ui: &mut Ui, app: &mut App) {
    let label = Msg::FeedsAdd.text(app.state.locale);
    if widgets::accent_pill(ui, &label).clicked() {
        super::feed_form::open_new(app, ui.ctx());
    }
}

#[cfg(test)]
mod tests {
    use putio_core::i18n::Locale;

    use super::*;
    use crate::ui::test_support::{accessible_names, text_boxes, text_with_and_without_hover};

    const LOCALES: [Locale; 3] = [Locale::En, Locale::De, Locale::Es];
    /// The list's width in the narrowest window (560, less the margins).
    const NARROWEST: f32 = 536.0;

    fn sample() -> RssFeed {
        RssFeed {
            id: 7,
            title: "Weekly releases".into(),
            rss_source_url: "https://releases.example.invalid/rss.xml".into(),
            keyword: Some("1080p".into()),
            ..RssFeed::default()
        }
    }

    fn paused() -> RssFeed {
        RssFeed {
            paused: true,
            ..sample()
        }
    }

    fn broken() -> RssFeed {
        RssFeed {
            last_error: Some("FEED_CANNOT_FETCHED".into()),
            ..sample()
        }
    }

    #[test]
    fn a_paused_feed_offers_resume_and_a_running_one_pause() {
        let acts = |feed: &RssFeed| -> Vec<Act> {
            entries(feed)
                .iter()
                .flatten()
                .map(|entry| entry.act)
                .collect()
        };
        let running = acts(&sample());
        assert!(running.contains(&Act::Pause) && !running.contains(&Act::Resume));
        let paused = acts(&paused());
        assert!(paused.contains(&Act::Resume) && !paused.contains(&Act::Pause));
        // Editing and deleting are always there; deleting is last and red.
        for list in [&running, &paused] {
            assert!(list.contains(&Act::Edit));
            assert_eq!(list.last(), Some(&Act::Delete));
        }
        let groups = entries(&sample());
        assert_eq!(groups.last().unwrap()[0].tint, Tint::Danger);
    }

    #[test]
    fn only_a_web_address_can_be_copied() {
        let copyable = |feed: &RssFeed| {
            entries(feed)
                .iter()
                .flatten()
                .any(|entry| entry.act == Act::CopyAddress)
        };
        assert!(copyable(&sample()));
        let mut odd = sample();
        odd.rss_source_url = "javascript:alert(1)".into();
        assert!(!copyable(&odd));
        odd.rss_source_url = String::new();
        assert!(!copyable(&odd));
    }

    #[test]
    fn the_second_line_says_the_site_and_the_filter_without_repeating_the_name() {
        let f = sample();
        assert_eq!(
            detail(&f, "Weekly releases", "releases.example.invalid"),
            "releases.example.invalid · 1080p"
        );
        assert_eq!(
            detail(&f, "releases.example.invalid", "releases.example.invalid"),
            "1080p"
        );
        let mut plain = sample();
        plain.keyword = None;
        assert_eq!(
            detail(
                &plain,
                "releases.example.invalid",
                "releases.example.invalid"
            ),
            ""
        );
        assert_eq!(
            detail(&plain, "Weekly", "releases.example.invalid"),
            "releases.example.invalid"
        );
        assert_eq!(detail(&f, "x", ""), "1080p");
    }

    /// One card at the narrowest list width, in `locale`; its text.
    fn card_text(feed: &RssFeed, locale: Locale) -> (Rect, Vec<crate::ui::test_support::TextBox>) {
        let rect = std::cell::Cell::new(Rect::NOTHING);
        let boxes = text_boxes(|ui| {
            ui.set_width(NARROWEST);
            rect.set(draw_card(ui, feed, false, locale).body.rect);
        });
        (rect.get(), boxes)
    }

    #[test]
    fn no_language_pushes_the_name_into_the_pill_or_out_of_the_card() {
        let long = "A feed with a very long name that keeps going and going so that it has to be cut to fit";
        for locale in LOCALES {
            for (extra, base) in [
                ("paused", paused()),
                ("broken", broken()),
                ("running", sample()),
            ] {
                let mut f = base;
                f.title = long.into();
                let (card, boxes) = card_text(&f, locale);
                assert!(!boxes.is_empty());
                for text in &boxes {
                    assert!(
                        card.expand(0.5).contains_rect(text.visible),
                        "{locale:?} {extra}: {:?} sticks out of the card: {:?} vs {card:?}",
                        text.text,
                        text.visible
                    );
                }
                // Whatever pill there is, the name ends before it starts, with room to spare.
                let pill = [Msg::FeedPaused, Msg::FeedProblem]
                    .iter()
                    .find_map(|m| boxes.iter().find(|b| b.text == m.text(locale)));
                if let Some(pill) = pill {
                    let name = boxes.iter().find(|b| b.text.starts_with("A feed")).unwrap();
                    assert!(
                        name.full.right() + TEXT_GAP - 1.0 <= pill.full.left(),
                        "{locale:?}: the name runs into the pill"
                    );
                    assert!(
                        name.full.width() > 150.0,
                        "{locale:?}: the pill leaves too little for the name"
                    );
                }
            }
        }
    }

    #[test]
    fn a_card_does_not_move_its_text_when_hovered() {
        for locale in LOCALES {
            for (extra, f) in [
                ("paused", paused()),
                ("broken", broken()),
                ("running", sample()),
            ] {
                let (resting, hovered) = text_with_and_without_hover(|ui| {
                    let card = draw_card(ui, &f, false, locale);
                    (card.body.rect, card.body.hovered())
                });
                assert_eq!(resting, hovered, "{locale:?} {extra}");
            }
        }
    }

    #[test]
    fn the_add_button_does_not_move_its_text_when_hovered() {
        for locale in LOCALES {
            let label = Msg::FeedsAdd.text(locale);
            let (resting, hovered) = text_with_and_without_hover(|ui| {
                let response = widgets::accent_pill(ui, &label);
                (response.rect, response.hovered())
            });
            assert_eq!(resting, hovered, "{locale:?}");
        }
    }

    #[test]
    fn the_add_button_and_the_longest_sort_label_leave_the_tabs_their_room() {
        // The widest thing the right-hand side can be: the biggest button in any language.
        for locale in LOCALES {
            let label = Msg::FeedsAdd.text(locale);
            let width = std::cell::Cell::new(0.0);
            text_boxes(|ui| width.set(widgets::accent_pill(ui, &label).rect.width()));
            assert!(
                width.get() < 160.0,
                "{locale:?}: the add button is {} wide",
                width.get()
            );
        }
    }

    #[test]
    fn a_card_and_its_actions_are_named_for_a_screen_reader() {
        let names = accessible_names(|ui| {
            draw_card(ui, &paused(), false, Locale::En);
        });
        let has = |role: &str, name: &str| names.iter().any(|(r, n)| r == role && n == name);
        assert!(
            has(
                "Button",
                "Weekly releases, releases.example.invalid, Paused"
            ),
            "{names:?}"
        );
        assert!(has("Button", "Feed actions: Weekly releases"), "{names:?}");
        let names = accessible_names(|ui| {
            widgets::accent_pill(ui, "Add feed");
        });
        assert!(
            names.iter().any(|(r, n)| r == "Button" && n == "Add feed"),
            "{names:?}"
        );
    }
}
