//! The tab bar under the header, and the view it switches: transfers, feeds, and room for more.
//!
//! The bar is one row: the tabs on the left, what belongs to the open tab on the right (the sort menu,
//! the "Add feed" button), and, when the row is wide enough, the update pill in between. German and
//! Spanish words are longer than English ones, so the row is *planned* before it is painted: counts go
//! first, then the labels are shortened with an ellipsis, and the right-hand side never moves.

use eframe::egui::{
    self, Align, CornerRadius, Layout, Pos2, Rect, Role, Sense, Ui, UiBuilder, Vec2, WidgetInfo,
};
use putio_core::i18n::Msg;
use putio_core::settings::Tab;
use putio_core::state::Event;

use super::{feeds, files, table, theme, update, widgets};
use crate::app::App;

/// Height of the bar, its hairline included.
pub(crate) const BAR_HEIGHT: f32 = 34.0;
/// Space between the hairline and the list.
const BAR_GAP: f32 = 8.0;
/// Room kept at either side of a label.
const TAB_PAD: f32 = 12.0;
/// Between a label and its count.
const COUNT_GAP: f32 = 6.0;
/// A label is never squeezed below this (about a short word and an ellipsis).
const MIN_LABEL: f32 = 44.0;
const UNDERLINE: f32 = 2.5;
/// The room at the right of the list that the cards leave free (for a scroll bar).
const EDGE: f32 = 8.0;
/// Between the tabs and the right-hand side (and the update pill).
const SIDE_GAP: f32 = 10.0;

/// A tab as it would like to be drawn, with the widths that takes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Natural {
    pub(crate) label: String,
    pub(crate) label_width: f32,
    /// The width of the count's pill, when the tab has a count.
    pub(crate) count_width: Option<f32>,
}

/// A tab as it will be drawn.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Slot {
    pub(crate) label: String,
    pub(crate) show_count: bool,
    /// The width of the whole tab, margins included.
    pub(crate) width: f32,
}

/// A count of things on screen, as a float (there are never more than a handful).
fn count_f32(count: usize) -> f32 {
    f32::from(u16::try_from(count).unwrap_or(u16::MAX))
}

fn natural_width(tab: &Natural, with_count: bool) -> f32 {
    let count = match (with_count, tab.count_width) {
        (true, Some(width)) => COUNT_GAP + width,
        _ => 0.0,
    };
    TAB_PAD * 2.0 + tab.label_width + count
}

/// Fit the tabs into `available` width. First everything as it wants to be; if that is too wide the
/// counts go; if that is still too wide the labels are cut (`fit` cuts a text to a width), the long
/// ones first so that short ones keep their words.
pub(crate) fn plan(
    tabs: &[Natural],
    available: f32,
    fit: impl Fn(&str, f32) -> String,
) -> Vec<Slot> {
    let total =
        |with_count: bool| -> f32 { tabs.iter().map(|tab| natural_width(tab, with_count)).sum() };
    for with_count in [true, false] {
        if total(with_count) <= available {
            return tabs
                .iter()
                .map(|tab| Slot {
                    label: tab.label.clone(),
                    show_count: with_count && tab.count_width.is_some(),
                    width: natural_width(tab, with_count),
                })
                .collect();
        }
    }
    // Labels only. Water-filling: a label shorter than its fair share keeps its width, and what it does
    // not use goes to the longer ones.
    let budget = (available - TAB_PAD * 2.0 * count_f32(tabs.len())).max(0.0);
    let mut order: Vec<(usize, f32)> = tabs.iter().map(|tab| tab.label_width).enumerate().collect();
    order.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut widths = vec![0.0; tabs.len()];
    let mut left = budget;
    let mut remaining = tabs.len();
    for (index, natural) in order {
        let share = (left / count_f32(remaining)).max(MIN_LABEL);
        let width = natural.min(share);
        if let Some(slot) = widths.get_mut(index) {
            *slot = width;
        }
        left = (left - width).max(0.0);
        remaining = remaining.saturating_sub(1);
    }
    tabs.iter()
        .zip(widths)
        .map(|(tab, width)| Slot {
            label: if width < tab.label_width {
                fit(&tab.label, width)
            } else {
                tab.label.clone()
            },
            show_count: false,
            width: TAB_PAD * 2.0 + width,
        })
        .collect()
}

struct Item {
    tab: Tab,
    label: String,
    count: Option<usize>,
}

/// The shortcut that opens the `index`-th tab (⌘1, ⌘2, … on macOS, Ctrl+1, … elsewhere).
pub(crate) fn shortcut(index: usize) -> Option<egui::KeyboardShortcut> {
    let key = [
        egui::Key::Num1,
        egui::Key::Num2,
        egui::Key::Num3,
        egui::Key::Num4,
    ]
    .get(index)
    .copied()?;
    Some(egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, key))
}

/// The tab the user asked for with its shortcut this frame, if any.
pub(crate) fn pressed(ctx: &egui::Context) -> Option<Tab> {
    TABS.iter().enumerate().find_map(|(index, tab)| {
        let shortcut = shortcut(index)?;
        ctx.input_mut(|input| input.consume_shortcut(&shortcut))
            .then_some(*tab)
    })
}

/// The tabs, in the order they are shown (and numbered for the shortcuts).
const TABS: [Tab; 3] = [Tab::Transfers, Tab::Files, Tab::Feeds];

fn items(app: &App) -> Vec<Item> {
    let locale = app.state.locale;
    vec![
        Item {
            tab: Tab::Transfers,
            label: Msg::TransfersTitle.text(locale),
            count: Some(app.state.transfers.len()),
        },
        Item {
            tab: Tab::Files,
            label: Msg::TabFiles.text(locale),
            count: None,
        },
        Item {
            tab: Tab::Feeds,
            label: Msg::TabFeeds.text(locale),
            count: app
                .state
                .feeds
                .loaded
                .then_some(app.state.feeds.items.len()),
        },
    ]
}

/// The bar and the open tab's view.
pub(crate) fn show(ui: &mut Ui, app: &mut App) {
    let ctx = ui.ctx().clone();
    // The cards leave room at the right for a scroll bar; the bar lines up with the cards, not with that room.
    let (row, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width() - EDGE, BAR_HEIGHT),
        Sense::hover(),
    );
    let p = theme::pal();
    ui.painter().rect_filled(
        Rect::from_min_size(
            Pos2::new(row.left(), row.bottom() - 1.0),
            Vec2::new(row.width(), 1.0),
        ),
        0.0,
        p.divider,
    );
    let content = Rect::from_min_max(row.min, Pos2::new(row.right(), row.bottom() - 1.0));

    // What belongs to the open tab goes first, at the right edge: it is the part that must never be
    // pushed out, and it says how much room is left for the rest.
    let mut side = ui.new_child(
        UiBuilder::new()
            .max_rect(content)
            .layout(Layout::right_to_left(Align::Center)),
    );
    match app.state.tab {
        Tab::Transfers => table::sort_menu(&mut side, app),
        Tab::Files => files::new_folder_button(&mut side, app),
        // The empty list has a big button of its own; two of them would be one too many.
        Tab::Feeds if feeds::is_empty(app) => {}
        Tab::Feeds => feeds::add_button(&mut side, app),
    }
    let side_left = side.min_rect().left().min(content.right());

    // The tabs in what is left.
    let current = app.state.tab;
    let items = items(app);
    let tab_room = (side_left - SIDE_GAP - content.left()).max(0.0);
    let painter = ui.painter().clone();
    let naturals: Vec<Natural> = items
        .iter()
        .map(|item| Natural {
            label: item.label.clone(),
            label_width: label_width(&painter, &item.label, item.tab == current),
            count_width: item.count.map(|count| count_width(&painter, count)),
        })
        .collect();
    let font_for = |label: &str| {
        let active = items
            .iter()
            .any(|item| item.label == label && item.tab == current);
        label_font(active)
    };
    let slots = plan(&naturals, tab_room, |label, width| {
        widgets::fit_to_width(&painter, label, &font_for(label), width)
    });
    let mut x = content.left();
    let mut clicked = None;
    for (item, slot) in items.iter().zip(&slots) {
        let rect = Rect::from_min_size(
            Pos2::new(x, content.top()),
            Vec2::new(slot.width, content.height()),
        );
        if tab(ui, rect, item, slot, item.tab == current).clicked() {
            clicked = Some(item.tab);
        }
        x += slot.width;
    }

    // The update pill, only when there is room for all of it.
    if let Some(look) = update::look(&app.state) {
        let width = update::pill_width(ui, &look);
        if x + SIDE_GAP + width <= side_left - SIDE_GAP {
            let pill_rect = Rect::from_min_size(
                Pos2::new(x + SIDE_GAP, content.center().y - update::PILL_HEIGHT / 2.0),
                Vec2::new(width, update::PILL_HEIGHT),
            );
            let mut pill_ui = ui.new_child(UiBuilder::new().max_rect(pill_rect));
            let response = update::pill(&mut pill_ui, &look);
            if response.clicked()
                && let Some(click) = look.click
            {
                app.dispatch(&ctx, click.event());
            }
        }
    }

    if let Some(tab) = clicked {
        app.dispatch(&ctx, Event::SelectTab(tab));
    }
    ui.add_space(BAR_GAP);
    match app.state.tab {
        Tab::Transfers => table::show(ui, app),
        Tab::Files => files::show(ui, app),
        Tab::Feeds => feeds::show(ui, app),
    }
}

fn label_font(active: bool) -> egui::FontId {
    if active {
        theme::bold(13.0)
    } else {
        theme::text(13.0)
    }
}

fn label_width(painter: &egui::Painter, label: &str, active: bool) -> f32 {
    painter
        .layout_no_wrap(
            label.to_owned(),
            label_font(active),
            egui::Color32::PLACEHOLDER,
        )
        .size()
        .x
}

fn count_width(painter: &egui::Painter, count: usize) -> f32 {
    let galley = painter.layout_no_wrap(
        count.to_string(),
        theme::bold(10.5),
        egui::Color32::PLACEHOLDER,
    );
    (galley.size().x + 14.0).max(22.0)
}

/// One tab: its label, its count, and a yellow line under the open one. Painted at fixed positions, so the
/// pointer arriving changes colours only.
fn tab(ui: &mut Ui, rect: Rect, item: &Item, slot: &Slot, selected: bool) -> egui::Response {
    let p = theme::pal();
    let response = ui
        .interact(
            rect,
            egui::Id::unique(("tab", item.tab as u8)),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let name = match item.count {
        Some(count) => format!("{}, {count}", item.label),
        None => item.label.clone(),
    };
    response.widget_info(|| WidgetInfo::selected(Role::RadioButton, true, selected, &name));
    widgets::focus_ring(
        ui,
        &response,
        rect.shrink2(Vec2::new(2.0, 4.0)),
        CornerRadius::same(8),
    );

    let ink = if selected || response.hovered() {
        p.text
    } else {
        p.muted
    };
    let galley = ui
        .painter()
        .layout_no_wrap(slot.label.clone(), label_font(selected), ink);
    let label_pos = Pos2::new(
        rect.left() + TAB_PAD,
        rect.center().y - galley.size().y / 2.0,
    );
    let label_right = label_pos.x + galley.size().x;
    ui.painter().galley(label_pos, galley, ink);
    if slot.show_count
        && let Some(count) = item.count
    {
        let galley = ui.painter().layout_no_wrap(
            count.to_string(),
            theme::bold(10.5),
            if selected { p.on_yellow } else { p.muted },
        );
        let pill = Rect::from_min_size(
            Pos2::new(label_right + COUNT_GAP, rect.center().y - 9.0),
            Vec2::new((galley.size().x + 14.0).max(22.0), 18.0),
        );
        ui.painter().rect_filled(
            pill,
            CornerRadius::same(9),
            if selected { theme::YELLOW } else { p.track },
        );
        let ink = if selected { p.on_yellow } else { p.muted };
        ui.painter()
            .galley(pill.center() - galley.size() / 2.0, galley, ink);
    }
    if selected {
        ui.painter().rect_filled(
            Rect::from_min_size(
                Pos2::new(rect.left() + 6.0, rect.bottom() - UNDERLINE),
                Vec2::new(rect.width() - 12.0, UNDERLINE),
            ),
            CornerRadius::same(2),
            theme::YELLOW,
        );
    }
    response
}

#[cfg(test)]
#[allow(clippy::cast_precision_loss)] // tiny test numbers
mod tests {
    use super::*;

    #[test]
    fn each_tab_has_its_own_number_shortcut() {
        let keys: Vec<_> = (0..TABS.len())
            .map(|index| shortcut(index).unwrap())
            .collect();
        assert_eq!(keys[0].logical_key, egui::Key::Num1);
        assert_eq!(keys[1].logical_key, egui::Key::Num2);
        assert_eq!(keys[2].logical_key, egui::Key::Num3);
        assert!(keys.iter().all(|key| key.modifiers.command));
        // The row of tabs and the table of shortcuts are one list.
        assert_eq!(TABS, [Tab::Transfers, Tab::Files, Tab::Feeds]);
        assert!(shortcut(TABS.len() + 50).is_none());
    }

    /// Seven pixels a letter: wide enough to cut, simple enough to count.
    fn measure(text: &str) -> f32 {
        text.chars().count() as f32 * 7.0
    }

    fn natural(label: &str, count: Option<f32>) -> Natural {
        Natural {
            label: label.into(),
            label_width: measure(label),
            count_width: count,
        }
    }

    fn fit(label: &str, width: f32) -> String {
        widgets::fit(label, width, measure).0
    }

    #[test]
    fn everything_is_kept_when_there_is_room() {
        let tabs = [
            natural("Transfers", Some(22.0)),
            natural("Feeds", Some(22.0)),
        ];
        let slots = plan(&tabs, 1_000.0, fit);
        assert!(slots.iter().all(|slot| slot.show_count));
        assert_eq!(slots[0].label, "Transfers");
        assert_eq!(slots[0].width, 24.0 + 63.0 + 6.0 + 22.0);
    }

    #[test]
    fn counts_go_before_any_word_is_cut() {
        let tabs = [
            natural("Übertragungen", Some(22.0)),
            natural("Feeds", Some(22.0)),
        ];
        let with = plan(&tabs, 1_000.0, fit)
            .iter()
            .map(|s| s.width)
            .sum::<f32>();
        let slots = plan(&tabs, with - 1.0, fit);
        assert!(slots.iter().all(|slot| !slot.show_count), "counts dropped");
        assert_eq!(slots[0].label, "Übertragungen", "no word is cut yet");
        assert_eq!(slots[1].label, "Feeds");
    }

    #[test]
    fn labels_are_cut_longest_first_and_the_row_fits() {
        let tabs = [
            natural("Übertragungen", Some(22.0)),
            natural("Feeds", Some(22.0)),
        ];
        // Room for the short word and 70 px of the long one.
        let room = 24.0 + 35.0 + 24.0 + 70.0;
        let slots = plan(&tabs, room, fit);
        assert_eq!(slots[1].label, "Feeds", "the short word is untouched");
        assert!(slots[0].label.ends_with('…'), "{}", slots[0].label);
        assert!(slots.iter().map(|s| s.width).sum::<f32>() <= room + 0.5);
        assert!(slots.iter().all(|s| !s.show_count));
    }

    #[test]
    fn a_label_is_never_squeezed_to_nothing() {
        let tabs = [natural("Übertragungen", None), natural("Descargas", None)];
        for room in [0.0, 10.0, 60.0] {
            for slot in plan(&tabs, room, fit) {
                assert!(slot.width >= TAB_PAD * 2.0 + MIN_LABEL.min(7.0), "{slot:?}");
                assert_ne!(slot.label, "");
            }
        }
    }

    #[test]
    fn a_tab_without_a_count_never_shows_one() {
        let slots = plan(&[natural("Feeds", None)], 1_000.0, fit);
        assert!(!slots[0].show_count);
    }
}

#[cfg(test)]
mod layout_tests {
    use putio_core::i18n::Locale;
    use putio_core::transfers::SortKey;

    use super::*;
    use crate::ui::test_support::{accessible_names, text_boxes, text_with_and_without_hover};

    const LOCALES: [Locale; 3] = [Locale::En, Locale::De, Locale::Es];
    /// The bar in the narrowest window: 560, less the margins and the room the cards leave.
    const NARROWEST: f32 = 536.0 - EDGE;

    fn width_of(text: &str, font: &egui::FontId) -> f32 {
        let width = std::cell::Cell::new(0.0);
        text_boxes(|ui| {
            width.set(
                ui.painter()
                    .layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::PLACEHOLDER)
                    .size()
                    .x,
            );
        });
        width.get()
    }

    /// The widest right-hand side the bar can have in `locale`: the longest "Sort: …" label with its arrow.
    fn widest_side(locale: Locale) -> f32 {
        [
            SortKey::Default,
            SortKey::Name,
            SortKey::Progress,
            SortKey::Speed,
            SortKey::Eta,
            SortKey::Size,
        ]
        .into_iter()
        .map(|key| {
            let label = match key {
                SortKey::Default => Msg::SortStatus,
                SortKey::Name => Msg::ColName,
                SortKey::Progress => Msg::SortProgress,
                SortKey::Speed => Msg::SortSpeed,
                SortKey::Eta => Msg::ColEta,
                SortKey::Size => Msg::ColSize,
            };
            let text = format!("{}: {}", Msg::SortBy.text(locale), label.text(locale));
            width_of(&text, &theme::text(11.0)) + 34.0
        })
        .fold(0.0, f32::max)
    }

    #[test]
    fn the_tab_labels_are_never_cut_at_the_narrowest_window_in_any_language() {
        for locale in LOCALES {
            let naturals: Vec<Natural> = [
                (Msg::TransfersTitle, Some(36.0)),
                (Msg::TabFiles, None),
                (Msg::TabFeeds, Some(22.0)),
            ]
            .iter()
            .map(|(msg, count_width)| {
                let label = msg.text(locale);
                Natural {
                    // The open tab is the bold one: measure the widest it can be.
                    label_width: width_of(&label, &theme::bold(13.0)),
                    label,
                    count_width: *count_width,
                }
            })
            .collect();
            let room = NARROWEST - widest_side(locale) - SIDE_GAP;
            let slots = plan(&naturals, room, |_, _| String::from("cut"));
            for (slot, natural) in slots.iter().zip(&naturals) {
                assert_eq!(
                    slot.label,
                    natural.label,
                    "{locale:?}: {:?} is cut ({} of {room})",
                    natural.label,
                    slots.iter().map(|s| s.width).sum::<f32>()
                );
            }
            assert!(
                slots.iter().map(|s| s.width).sum::<f32>() <= room + 0.5,
                "{locale:?}: the tabs overflow their room"
            );
        }
    }

    fn item(tab: Tab, label: &str, count: Option<usize>) -> Item {
        Item {
            tab,
            label: label.into(),
            count,
        }
    }

    #[test]
    fn a_tab_does_not_move_its_text_when_hovered() {
        for locale in LOCALES {
            for selected in [true, false] {
                let it = item(Tab::Feeds, &Msg::TabFeeds.text(locale), Some(12));
                let slot = Slot {
                    label: it.label.clone(),
                    show_count: true,
                    width: 140.0,
                };
                let (resting, hovered) = text_with_and_without_hover(|ui| {
                    let rect =
                        Rect::from_min_size(Pos2::new(20.0, 20.0), Vec2::new(140.0, BAR_HEIGHT));
                    let response = tab(ui, rect, &it, &slot, selected);
                    (response.rect, response.hovered())
                });
                assert_eq!(resting, hovered, "{locale:?} selected: {selected}");
            }
        }
    }

    #[test]
    fn the_tabs_are_radio_buttons_with_their_counts_and_the_open_one_is_selected() {
        let names = accessible_names(|ui| {
            let slot = |label: &str| Slot {
                label: label.into(),
                show_count: true,
                width: 140.0,
            };
            let a = item(Tab::Transfers, "Transfers", Some(7));
            let b = item(Tab::Feeds, "Feeds", None);
            let rect =
                |x: f32| Rect::from_min_size(Pos2::new(x, 10.0), Vec2::new(140.0, BAR_HEIGHT));
            tab(ui, rect(10.0), &a, &slot("Transfers"), true);
            tab(ui, rect(160.0), &b, &slot("Feeds"), false);
        });
        assert!(
            names
                .iter()
                .any(|(r, n)| r == "RadioButton" && n == "Transfers, 7"),
            "{names:?}"
        );
        assert!(
            names
                .iter()
                .any(|(r, n)| r == "RadioButton" && n == "Feeds"),
            "{names:?}"
        );
    }
}
