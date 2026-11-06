//! The Files tab: browse put.io folders, search, and act on what is there.
//!
//! One row per file or folder, in the family of the transfer and feed cards. Above the rows is a toolbar:
//! the way from the top level to the folder being shown (every step a button that goes back to it) and a
//! search field; while a search is showing, the toolbar says what was searched for instead. A folder opens
//! with a click, a file is selected, and everything else is in the row's menu.
//!
//! Everything is painted at fixed positions and every text is cut to its room (the breadcrumb is *planned*
//! like the tab bar is), so no language can push a row or the toolbar out of shape.

use eframe::egui::{
    self, Align2, CornerRadius, Id, Key, Pos2, Rect, RichText, Sense, Stroke, Ui, UiBuilder, Vec2,
};
use putio_api::{ExtractStatus, File};
use putio_core::files::{Kind, can_stream, display_name, is_folder, kind};
use putio_core::format;
use putio_core::i18n::{Locale, Msg};
use putio_core::state::{Crumb, Event};

use super::icons::{self, MenuIcon};
use super::menu::{self, Tint};
use super::table::mix;
use super::{theme, widgets};
use crate::app::App;

const ROW_HEIGHT: f32 = 56.0;
const ROW_GAP: f32 = 6.0;
/// Space kept free at the right of the rows for the scroll bar.
const SCROLLBAR_ROOM: f32 = 8.0;
const MORE_INSET: f32 = 24.0;
const MORE_SIZE: f32 = 28.0;
/// Width of the row context menu.
const MENU_WIDTH: f32 = 252.0;
/// Height of the toolbar above the rows.
const TOOLBAR_HEIGHT: f32 = 38.0;
/// Width of the search field.
const SEARCH_WIDTH: f32 = 176.0;
/// Between the toolbar's two halves.
const TOOLBAR_GAP: f32 = 12.0;
/// The "clear search" button.
const CLEAR_SIZE: f32 = 26.0;

pub(crate) fn show(ui: &mut Ui, app: &mut App) {
    let ctx = ui.ctx().clone();
    toolbar(ui, app, &ctx);
    ui.add_space(4.0);
    let rows = rows_to_show(app);
    let Some(rows) = rows else {
        state_message(ui, app);
        return;
    };
    let mut action: Option<(i64, Act)> = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = ROW_GAP;
            for file in &rows {
                if let Some(act) = card(ui, app, file) {
                    action = Some((file.id, act));
                }
            }
            if let Some(search) = app
                .state
                .files
                .search
                .as_ref()
                .filter(|s| s.total > rows.len())
            {
                let note = Msg::FilesSearchMore {
                    shown: rows.len(),
                    total: search.total,
                }
                .text(app.state.locale);
                ui.add_space(4.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(note)
                            .font(theme::text(10.5))
                            .color(theme::pal().muted),
                    )
                    .wrap(),
                );
            }
            ui.add_space(4.0);
        });
    if let Some((id, act)) = action
        && let Some(file) = rows.iter().find(|file| file.id == id)
    {
        apply(app, &ctx, file, act);
    }
}

/// What the list below the toolbar holds right now, or `None` when there is a message to show instead
/// (loading, failed, nothing here).
fn rows_to_show(app: &App) -> Option<Vec<File>> {
    let view = &app.state.files;
    let rows = match &view.search {
        Some(search) if search.loading || search.failed => return None,
        Some(search) => &search.items,
        None if !view.loaded || view.failed => return None,
        None => &view.items,
    };
    (!rows.is_empty()).then(|| rows.clone())
}

// ---- what a row does --------------------------------------------------------------------------

/// What a row's menu (or a click) can do. Always aimed at the file's *id*, never its row: the list can
/// change between opening a menu and clicking an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    Enter,
    OpenOnPutio,
    Download,
    Stream,
    Extract,
    CopyLink,
    Rename,
    Move,
    Delete,
}

struct Entry {
    icon: MenuIcon,
    label: Msg,
    tint: Tint,
    act: Act,
}

/// The menu of a row, in groups: what leaves the app, what copies, what changes, what removes.
fn entries(file: &File) -> Vec<Vec<Entry>> {
    let entry = |icon, label, act| Entry {
        icon,
        label,
        tint: Tint::Normal,
        act,
    };
    let mut open = vec![entry(
        MenuIcon::External,
        Msg::MenuOpenOnPutio,
        Act::OpenOnPutio,
    )];
    open.push(entry(MenuIcon::Download, Msg::MenuDownload, Act::Download));
    if can_stream(file) {
        open.push(entry(MenuIcon::Play, Msg::MenuStream, Act::Stream));
    }
    if kind(file) == Kind::Archive {
        open.push(entry(
            MenuIcon::Download,
            Msg::FileMenuExtract,
            Act::Extract,
        ));
    }
    vec![
        open,
        vec![entry(MenuIcon::Copy, Msg::MenuCopyLink, Act::CopyLink)],
        vec![
            entry(MenuIcon::Edit, Msg::FileMenuRename, Act::Rename),
            entry(MenuIcon::Folder, Msg::FileMenuMove, Act::Move),
        ],
        vec![Entry {
            icon: MenuIcon::Trash,
            label: Msg::FileMenuDelete,
            tint: Tint::Danger,
            act: Act::Delete,
        }],
    ]
}

fn menu_body(ui: &mut Ui, locale: Locale, file: &File) -> Option<Act> {
    ui.spacing_mut().item_spacing.y = 0.0;
    menu::caption(ui, &display_name(file));
    let mut chosen = None;
    for group in entries(file) {
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

fn apply(app: &mut App, ctx: &egui::Context, file: &File, act: Act) {
    let id = file.id;
    match act {
        Act::Enter => {
            let name = display_name(file);
            app.ui.search_text.clear();
            app.dispatch(ctx, Event::FilesOpenFolder { id, name });
        }
        Act::OpenOnPutio => app.dispatch(ctx, Event::FileOpenRequested(id)),
        Act::Download => app.dispatch(ctx, Event::FileDownloadRequested(id)),
        Act::Stream => app.dispatch(ctx, Event::FileStreamRequested(id)),
        Act::Extract => app.dispatch(ctx, Event::FileExtractRequested { id, password: None }),
        Act::CopyLink => app.dispatch(ctx, Event::FileCopyLinkRequested(id)),
        Act::Rename => super::file_dialogs::open_rename(app, ctx, file),
        Act::Move => super::file_dialogs::open_move(app, ctx, file),
        Act::Delete => super::file_dialogs::ask_to_delete(app, file),
    }
}

// ---- a row ------------------------------------------------------------------------------------

struct Card {
    body: egui::Response,
    more: egui::Response,
}

fn kind_msg(kind: Kind) -> Msg {
    match kind {
        Kind::Folder => Msg::FileKindFolder,
        Kind::Video => Msg::FileKindVideo,
        Kind::Audio => Msg::FileKindAudio,
        Kind::Image => Msg::FileKindImage,
        Kind::Archive => Msg::FileKindArchive,
        Kind::Document => Msg::FileKindDocument,
        Kind::Other => Msg::FileKindOther,
    }
}

/// The second line of a row: the size of a file, and the day it was added.
fn detail(file: &File) -> String {
    let date = file
        .created_at
        .as_deref()
        .and_then(format::date)
        .unwrap_or_default();
    if is_folder(file) {
        return date;
    }
    let size = format::bytes(file.size);
    if date.is_empty() {
        size
    } else {
        format!("{size} · {date}")
    }
}

/// The pill of a row whose archive is being unpacked (or is waiting for a password), and the words for a
/// screen reader.
fn unpack_look(status: ExtractStatus) -> Option<(Msg, widgets::PillLook)> {
    let p = theme::pal();
    match status {
        ExtractStatus::InQueue => Some((Msg::StatusQueued, (p.neutral_soft, p.muted, None))),
        ExtractStatus::Extracting => {
            Some((Msg::PillExtracting, (p.yellow_soft, p.accent_text, None)))
        }
        ExtractStatus::Password => {
            Some((Msg::PillPassword, (p.error_soft, p.error, Some(p.error))))
        }
        ExtractStatus::Extracted | ExtractStatus::Error | ExtractStatus::Unknown => None,
    }
}

/// Where the state pill of a row ends, counted from the right edge of the row.
const PILL_INSET: f32 = 46.0;
/// Between a row's text and its pill.
const TEXT_GAP: f32 = 12.0;

fn draw_card(
    ui: &mut Ui,
    file: &File,
    selected: bool,
    unpacking: Option<ExtractStatus>,
    locale: Locale,
) -> Card {
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

    let kind = kind(file);
    let name = display_name(file);
    let detail = detail(file);
    let pill = unpacking.and_then(unpack_look);
    let mut description = if detail.is_empty() {
        format!("{name}, {}", kind_msg(kind).text(locale))
    } else {
        format!("{name}, {}, {detail}", kind_msg(kind).text(locale))
    };
    if let Some((msg, _)) = &pill {
        description = format!("{description}, {}", msg.text(locale));
    }
    widgets::describe(&body, egui::Role::Button, &description);
    widgets::describe(
        &more,
        egui::Role::Button,
        &format!("{}: {name}", Msg::A11yFileActions.text(locale)),
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
    let background = if selected {
        p.card_selected
    } else {
        mix(p.card, p.card_hover, hover)
    };
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(10), background);
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
    icons::file_badge(
        &painter,
        Pos2::new(rect.left() + 28.0, rect.center().y),
        kind,
    );

    let text_left = rect.left() + 52.0;
    // The pill first: how wide it is decides how much room the text has.
    let pill_left = pill.map(|(msg, look)| {
        widgets::status_pill(&painter, rect, PILL_INSET, &msg.text(locale), look)
    });
    let text_right = pill_left.map_or(rect.right() - MORE_INSET * 2.0, |left| left - TEXT_GAP);
    let text_width = (text_right - text_left).max(0.0);
    let title_font = theme::bold(12.5);
    let (title_at, title_align) = if detail.is_empty() {
        (Pos2::new(text_left, rect.center().y), Align2::LEFT_CENTER)
    } else {
        (Pos2::new(text_left, rect.top() + 11.0), Align2::LEFT_TOP)
    };
    painter.text(
        title_at,
        title_align,
        widgets::fit_to_width(&painter, &name, &title_font, text_width),
        title_font,
        p.text,
    );
    let detail_font = theme::text(10.5);
    painter.text(
        Pos2::new(text_left, rect.top() + 30.0),
        Align2::LEFT_TOP,
        widgets::fit_to_width(&painter, &detail, &detail_font, text_width),
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

fn card(ui: &mut Ui, app: &mut App, file: &File) -> Option<Act> {
    let locale = app.state.locale;
    let selected = app.ui.selected_file == Some(file.id);
    let unpacking = app
        .state
        .unpacking
        .iter()
        .find(|item| item.file_id == file.id)
        .map(|item| item.status);
    let Card { body, more } = draw_card(ui, file, selected, unpacking, locale);
    let folder = is_folder(file);
    if body.clicked() || more.clicked() {
        app.ui.selected_file = Some(file.id);
    }
    // A folder opens with a click; a file with a double click (on put.io, where it can be played).
    let mut chosen = if folder {
        body.clicked().then_some(Act::Enter)
    } else {
        body.double_clicked().then_some(Act::OpenOnPutio)
    };

    egui::Popup::context_menu(&body)
        .frame(widgets::popup_frame())
        .width(MENU_WIDTH)
        .show(|ui| chosen = chosen.or(menu_body(ui, locale, file)));
    let mut popup = egui::Popup::menu(&more)
        .align(egui::RectAlign::BOTTOM_END)
        .gap(4.0)
        .width(MENU_WIDTH)
        .frame(widgets::popup_frame());
    if app.ui.open_once == Some(super::Overlay::FileCard(file.id)) {
        app.ui.open_once = None;
        popup = popup.open_memory(Some(true.into()));
    }
    popup.show(|ui| chosen = chosen.or(menu_body(ui, locale, file)));
    chosen
}

// ---- the toolbar ------------------------------------------------------------------------------

/// One piece of the breadcrumb.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    /// The `n`-th folder of the trail.
    Crumb(usize),
    /// Folders left out between the top level and the end.
    Ellipsis,
}

/// Width of the arrow between two steps, and of the "…" that stands for the steps left out.
const SEPARATOR: f32 = 18.0;
const ELLIPSIS: f32 = 14.0;
/// The last step is never cut below this.
const MIN_LAST: f32 = 48.0;

/// The least the last step keeps when something has to give: enough to tell folders apart. Below it, steps
/// in the middle are left out before the name is cut any more.
const PREFERRED_LAST: f32 = 120.0;

/// Fit the way to the open folder into `available` width: everything when it fits; else the top level (the
/// way back, which must never disappear), "…" for the steps left out, and as many of the last steps as fit.
/// A long name at the end is cut with an ellipsis rather than pushing the top level out. Returns the pieces
/// and the width the last step may take.
pub(crate) fn plan_crumbs(widths: &[f32], available: f32) -> (Vec<Part>, f32) {
    let Some(last) = widths.len().checked_sub(1) else {
        return (Vec::new(), 0.0);
    };
    let width = |index: usize| widths.get(index).copied().unwrap_or(0.0);
    let all: f32 = widths.iter().sum::<f32>() + SEPARATOR * count_f32(last);
    if all <= available {
        return ((0..=last).map(Part::Crumb).collect(), width(last));
    }
    if last == 0 {
        // Only the top level, and it does not fit: cut it.
        return (vec![Part::Crumb(0)], available.max(MIN_LAST).min(width(0)));
    }
    // With the top level kept, which steps from `from` up to the last stay? The choice that keeps the most
    // steps wins, while the last step keeps at least `keep` of its width; if even the fewest steps cannot
    // give it that, it may shrink to the very least.
    for floor in [PREFERRED_LAST, MIN_LAST] {
        let keep = floor.min(width(last));
        let mut best: Option<(usize, usize, f32)> = None;
        for from in 1..=last {
            let ellipsis = from > 1;
            let others = width(0)
                + SEPARATOR
                + (from..last).map(|i| width(i) + SEPARATOR).sum::<f32>()
                + if ellipsis { ELLIPSIS + SEPARATOR } else { 0.0 };
            if others + keep <= available && best.is_none_or(|(kept, ..)| last - from + 1 > kept) {
                best = Some((last - from + 1, from, others));
            }
        }
        if let Some((_, from, others)) = best {
            let mut parts = vec![Part::Crumb(0)];
            if from > 1 {
                parts.push(Part::Ellipsis);
            }
            parts.extend((from..=last).map(Part::Crumb));
            return (parts, (available - others).min(width(last)));
        }
    }
    // Not even the top level and the last step's least: the end alone, so the user at least sees where
    // they are.
    if available >= ELLIPSIS + SEPARATOR + MIN_LAST {
        (
            vec![Part::Ellipsis, Part::Crumb(last)],
            (available - ELLIPSIS - SEPARATOR).min(width(last)),
        )
    } else {
        (
            vec![Part::Crumb(last)],
            available.max(MIN_LAST).min(width(last)),
        )
    }
}

/// A count of things on screen, as a float (a trail is never long).
fn count_f32(count: usize) -> f32 {
    f32::from(u16::try_from(count).unwrap_or(u16::MAX))
}

fn crumb_name(trail: &[Crumb], index: usize, locale: Locale) -> String {
    match trail.get(index) {
        Some(crumb) if index > 0 && !crumb.name.trim().is_empty() => crumb.name.clone(),
        Some(_) if index > 0 => "…".to_owned(),
        _ => Msg::FilesRoot.text(locale),
    }
}

fn toolbar(ui: &mut Ui, app: &mut App, ctx: &egui::Context) {
    let locale = app.state.locale;
    let (row, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width() - SCROLLBAR_ROOM, TOOLBAR_HEIGHT),
        Sense::hover(),
    );
    let searching = app.state.files.search.is_some();

    // The search field at the right, with a button inside its right end that empties it (and ends the
    // search) once there is something to empty.
    let field_rect = Rect::from_min_size(
        Pos2::new(
            row.right() - SEARCH_WIDTH,
            row.center().y - widgets::FIELD_HEIGHT / 2.0,
        ),
        Vec2::new(SEARCH_WIDTH, widgets::FIELD_HEIGHT),
    );
    let show_clear = searching || !app.ui.search_text.is_empty();
    search_field(ui, app, ctx, field_rect, show_clear);
    if show_clear {
        let clear = Rect::from_center_size(
            Pos2::new(
                field_rect.right() - 8.0 - CLEAR_SIZE / 2.0,
                field_rect.center().y,
            ),
            Vec2::splat(CLEAR_SIZE),
        );
        if clear_button(ui, clear, &Msg::A11yFilesClearSearch.text(locale)) {
            app.ui.search_text.clear();
            app.dispatch(ctx, Event::FilesSearchCleared);
        }
    }

    // The way to the open folder (or what was searched for) takes the rest.
    let left = Rect::from_min_max(
        row.min,
        Pos2::new(field_rect.left() - TOOLBAR_GAP, row.bottom()),
    );
    if let Some(search) = &app.state.files.search {
        let title = Msg::FilesSearchResults {
            query: search.query.clone(),
        }
        .text(locale);
        let font = theme::bold(12.5);
        let shown = widgets::fit_to_width(ui.painter(), &title, &font, left.width());
        ui.painter().text(
            Pos2::new(left.left() + 4.0, left.center().y),
            Align2::LEFT_CENTER,
            shown,
            font,
            theme::pal().text,
        );
    } else {
        breadcrumb(ui, app, ctx, left);
    }
}

fn clear_button(ui: &mut Ui, rect: Rect, label: &str) -> bool {
    let p = theme::pal();
    let response = ui
        .interact(rect, Id::unique("files-clear-search"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    widgets::describe(&response, egui::Role::Button, label);
    widgets::focus_ring(ui, &response, rect, CornerRadius::same(13));
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(13), p.card_hover);
    }
    let color = if response.hovered() { p.text } else { p.muted };
    let stroke = Stroke::new(1.6, color);
    let c = rect.center();
    ui.painter()
        .line_segment([c + Vec2::new(-3.5, -3.5), c + Vec2::new(3.5, 3.5)], stroke);
    ui.painter()
        .line_segment([c + Vec2::new(-3.5, 3.5), c + Vec2::new(3.5, -3.5)], stroke);
    response.clicked()
}

fn search_field(ui: &mut Ui, app: &mut App, ctx: &egui::Context, rect: Rect, room_for_clear: bool) {
    let locale = app.state.locale;
    let hint = Msg::FilesSearchHint.text(locale);
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
    let trailing = if room_for_clear {
        CLEAR_SIZE + 12.0
    } else {
        0.0
    };
    let field = widgets::text_field_with_room(
        &mut child,
        &mut app.ui.search_text,
        &hint,
        None,
        200,
        trailing,
    );
    if field.has_focus() && ctx.input(|input| input.key_pressed(Key::Escape)) {
        // Escape empties the field and ends the search.
        app.ui.search_text.clear();
        app.dispatch(ctx, Event::FilesSearchCleared);
        field.surrender_focus();
    } else if field.lost_focus() && ctx.input(|input| input.key_pressed(Key::Enter)) {
        let query = app.ui.search_text.clone();
        if query.trim().is_empty() {
            app.dispatch(ctx, Event::FilesSearchCleared);
        } else {
            app.dispatch(ctx, Event::FilesSearchRequested(query));
        }
    }
}

/// The small arrow between two steps of the breadcrumb, centred at `at`.
fn separator(painter: &egui::Painter, at: Pos2) {
    let stroke = Stroke::new(1.4, theme::pal().muted);
    painter.line_segment(
        [at + Vec2::new(-1.8, -3.5), at + Vec2::new(1.8, 0.0)],
        stroke,
    );
    painter.line_segment(
        [at + Vec2::new(1.8, 0.0), at + Vec2::new(-1.8, 3.5)],
        stroke,
    );
}

fn breadcrumb(ui: &mut Ui, app: &mut App, ctx: &egui::Context, area: Rect) {
    let p = theme::pal();
    let locale = app.state.locale;
    let trail = app.state.files.trail.clone();
    let font = |last: bool| {
        if last {
            theme::bold(12.5)
        } else {
            theme::text(12.5)
        }
    };
    let widths: Vec<f32> = trail
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let name = crumb_name(&trail, index, locale);
            let last = index + 1 == trail.len();
            ui.painter()
                .layout_no_wrap(name, font(last), egui::Color32::PLACEHOLDER)
                .size()
                .x
        })
        .collect();
    let (parts, last_cap) = plan_crumbs(&widths, (area.width() - 8.0).max(0.0));
    let mut x = area.left() + 4.0;
    let mut go_to = None;
    for (position, part) in parts.iter().enumerate() {
        if position > 0 {
            separator(
                ui.painter(),
                Pos2::new(x + SEPARATOR / 2.0, area.center().y),
            );
            x += SEPARATOR;
        }
        match *part {
            Part::Ellipsis => {
                ui.painter().text(
                    Pos2::new(x, area.center().y),
                    Align2::LEFT_CENTER,
                    "…",
                    theme::text(12.5),
                    p.muted,
                );
                x += ELLIPSIS;
            }
            Part::Crumb(index) => {
                let last = index + 1 == trail.len();
                let name = crumb_name(&trail, index, locale);
                let width = if last {
                    last_cap
                } else {
                    widths.get(index).copied().unwrap_or(0.0)
                };
                let shown = widgets::fit_to_width(ui.painter(), &name, &font(last), width);
                let rect = Rect::from_min_size(
                    Pos2::new(x, area.center().y - 12.0),
                    Vec2::new(width, 24.0),
                );
                if last {
                    ui.painter().text(
                        Pos2::new(x, area.center().y),
                        Align2::LEFT_CENTER,
                        shown,
                        font(true),
                        p.text,
                    );
                } else {
                    let response = ui
                        .interact(
                            rect.expand2(Vec2::new(4.0, 0.0)),
                            Id::unique(("crumb", index)),
                            Sense::click(),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    widgets::describe(&response, egui::Role::Button, &name);
                    widgets::focus_ring(
                        ui,
                        &response,
                        rect.expand2(Vec2::new(4.0, 0.0)),
                        CornerRadius::same(6),
                    );
                    let ink = if response.hovered() { p.text } else { p.muted };
                    ui.painter().text(
                        Pos2::new(x, area.center().y),
                        Align2::LEFT_CENTER,
                        shown,
                        font(false),
                        ink,
                    );
                    if response.clicked() {
                        go_to = Some(index);
                    }
                }
                x += width;
            }
        }
    }
    if let Some(index) = go_to {
        app.ui.search_text.clear();
        app.dispatch(ctx, Event::FilesGoTo(index));
    }
}

// ---- the other states of the list -------------------------------------------------------------

fn centered(ui: &Ui) -> Pos2 {
    let area = ui.available_rect_before_wrap();
    Pos2::new(
        area.center().x,
        area.top() + (area.height() * 0.34).min(110.0),
    )
}

fn message(ui: &Ui, badge: impl FnOnce(&egui::Painter, Pos2), title: String, hint: Option<String>) {
    let center = centered(ui);
    badge(ui.painter(), center);
    ui.painter().text(
        center + Vec2::new(0.0, 46.0),
        Align2::CENTER_CENTER,
        title,
        theme::bold(15.0),
        theme::pal().text,
    );
    if let Some(hint) = hint {
        let hint = ui
            .painter()
            .layout(hint, theme::text(11.5), theme::pal().muted, 340.0);
        ui.painter().galley(
            Pos2::new(center.x - hint.size().x / 2.0, center.y + 64.0),
            hint,
            theme::pal().muted,
        );
    }
}

fn spinner(ui: &mut Ui, text: String) {
    let center = centered(ui);
    ui.put(
        Rect::from_center_size(center, Vec2::splat(22.0)),
        egui::Spinner::new()
            .size(22.0)
            .color(theme::pal().accent_text),
    );
    ui.painter().text(
        center + Vec2::new(0.0, 32.0),
        Align2::CENTER_CENTER,
        text,
        theme::text(12.0),
        theme::pal().muted,
    );
}

fn retry_button(ui: &mut Ui, below: Pos2, label: &str) -> bool {
    let rect = Rect::from_center_size(
        Pos2::new(below.x, below.y + widgets::ACTION_HEIGHT / 2.0),
        Vec2::new(300.0, widgets::ACTION_HEIGHT),
    );
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    widgets::text_button(&mut row, label, false).clicked()
}

/// A message instead of the rows: loading, failed, or nothing here.
fn state_message(ui: &mut Ui, app: &mut App) {
    let ctx = ui.ctx().clone();
    let locale = app.state.locale;
    let center = centered(ui);
    if let Some(search) = &app.state.files.search {
        if search.loading {
            spinner(ui, Msg::FilesSearching.text(locale));
        } else if search.failed {
            ui.painter().text(
                center,
                Align2::CENTER_CENTER,
                Msg::FilesSearchFailed.text(locale),
                theme::bold(13.0),
                theme::pal().text,
            );
        } else {
            message(
                ui,
                icons::empty_badge,
                Msg::FilesSearchNone {
                    query: search.query.clone(),
                }
                .text(locale),
                None,
            );
        }
        return;
    }
    let view = &app.state.files;
    if view.failed {
        ui.painter().text(
            center,
            Align2::CENTER_CENTER,
            Msg::FilesLoadFailed.text(locale),
            theme::bold(13.0),
            theme::pal().text,
        );
        let label = Msg::FilesRetry.text(locale);
        if retry_button(ui, center + Vec2::new(0.0, 20.0), &label) {
            app.dispatch(&ctx, Event::FilesRefreshRequested);
        }
    } else if !view.loaded {
        spinner(ui, Msg::FilesLoading.text(locale));
    } else {
        message(
            ui,
            icons::empty_badge,
            Msg::FilesEmptyTitle.text(locale),
            Some(Msg::FilesEmptyHint.text(locale)),
        );
    }
}

// ---- the button of the tab bar ----------------------------------------------------------------

/// "New folder", at the right of the tab bar.
pub(crate) fn new_folder_button(ui: &mut Ui, app: &mut App) {
    let label = Msg::FilesNewFolder.text(app.state.locale);
    if widgets::accent_pill(ui, &label).clicked() {
        super::file_dialogs::open_new_folder(app, ui.ctx());
    }
}

#[cfg(test)]
mod tests {
    use putio_api::FileType;
    use putio_core::i18n::Locale;

    use super::*;
    use crate::ui::test_support::{accessible_names, text_boxes, text_with_and_without_hover};

    const LOCALES: [Locale; 3] = [Locale::En, Locale::De, Locale::Es];
    const NARROWEST: f32 = 536.0;

    fn file(kind: FileType, name: &str) -> File {
        File {
            id: 7,
            name: name.into(),
            parent_id: Some(0),
            file_type: kind,
            content_type: String::new(),
            size: 1_500_000_000,
            screenshot: None,
            created_at: Some("2024-05-01T12:34:56".into()),
        }
    }

    fn acts(file: &File) -> Vec<Act> {
        entries(file).iter().flatten().map(|e| e.act).collect()
    }

    #[test]
    fn a_menu_offers_what_the_file_can_do_and_deleting_last() {
        let video = acts(&file(FileType::Video, "a.mkv"));
        for act in [
            Act::OpenOnPutio,
            Act::Download,
            Act::Stream,
            Act::CopyLink,
            Act::Rename,
            Act::Move,
        ] {
            assert!(video.contains(&act), "{act:?}");
        }
        assert_eq!(video.last(), Some(&Act::Delete));
        assert!(!video.contains(&Act::Enter));
        // Only media and folders can be streamed.
        for (kind, streams) in [
            (FileType::Audio, true),
            (FileType::Folder, true),
            (FileType::Image, false),
            (FileType::Archive, false),
            (FileType::Pdf, false),
            (FileType::Unknown, false),
        ] {
            assert_eq!(
                acts(&file(kind, "x")).contains(&Act::Stream),
                streams,
                "{kind:?}"
            );
        }
        assert_eq!(
            entries(&file(FileType::File, "x")).last().unwrap()[0].tint,
            Tint::Danger
        );
    }

    #[test]
    fn the_second_line_says_the_size_and_the_day() {
        assert_eq!(detail(&file(FileType::Video, "a")), "1.4 GB · 2024-05-01");
        assert_eq!(detail(&file(FileType::Folder, "a")), "2024-05-01");
        let mut odd = file(FileType::Video, "a");
        odd.created_at = Some("not a date".into());
        assert_eq!(detail(&odd), "1.4 GB");
        odd.created_at = None;
        odd.file_type = FileType::Folder;
        assert_eq!(detail(&odd), "");
    }

    fn card_text(f: &File, locale: Locale) -> (Rect, Vec<crate::ui::test_support::TextBox>) {
        let rect = std::cell::Cell::new(Rect::NOTHING);
        let boxes = text_boxes(|ui| {
            ui.set_width(NARROWEST);
            rect.set(draw_card(ui, f, false, None, locale).body.rect);
        });
        (rect.get(), boxes)
    }

    #[test]
    fn no_text_of_a_row_sticks_out_of_it_however_long_the_name() {
        let long = "A file with a very long name that keeps going and going so that it has to be cut to fit.mkv";
        for locale in LOCALES {
            for kind in [FileType::Folder, FileType::Video, FileType::Unknown] {
                for name in [long, "Short", ""] {
                    let (card, boxes) = card_text(&file(kind, name), locale);
                    assert!(!boxes.is_empty());
                    for text in &boxes {
                        assert!(
                            card.expand(0.5).contains_rect(text.visible),
                            "{locale:?} {kind:?} {name:?}: {:?} sticks out ({:?} vs {card:?})",
                            text.text,
                            text.visible
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_row_does_not_move_its_text_when_hovered() {
        for locale in LOCALES {
            for kind in [FileType::Folder, FileType::Video] {
                let f = file(kind, "Some.Movie.2024.mkv");
                let (resting, hovered) = text_with_and_without_hover(|ui| {
                    let card = draw_card(ui, &f, false, None, locale);
                    (card.body.rect, card.body.hovered())
                });
                assert_eq!(resting, hovered, "{locale:?} {kind:?}");
            }
        }
    }

    #[test]
    fn a_row_and_its_actions_are_named_for_a_screen_reader() {
        let names = accessible_names(|ui| {
            draw_card(ui, &file(FileType::Video, "a.mkv"), false, None, Locale::En);
        });
        let has = |role: &str, name: &str| names.iter().any(|(r, n)| r == role && n == name);
        assert!(
            has("Button", "a.mkv, Video, 1.4 GB · 2024-05-01"),
            "{names:?}"
        );
        assert!(has("Button", "File actions: a.mkv"), "{names:?}");
    }

    // ---- the breadcrumb ----

    fn widths(n: usize) -> Vec<f32> {
        vec![60.0; n]
    }

    #[test]
    fn a_trail_that_fits_is_shown_whole() {
        let (parts, cap) = plan_crumbs(&widths(3), 1_000.0);
        assert_eq!(parts, [Part::Crumb(0), Part::Crumb(1), Part::Crumb(2)]);
        assert_eq!(cap, 60.0);
        assert_eq!(plan_crumbs(&[], 100.0).0, []);
        assert_eq!(plan_crumbs(&widths(1), 100.0).0, [Part::Crumb(0)]);
    }

    #[test]
    fn a_long_trail_keeps_the_top_level_and_the_end_and_leaves_the_middle_out() {
        // 6 steps of 60 + 5 arrows = 450; 280 fits root, "…" and two of the last steps.
        let (parts, _) = plan_crumbs(&widths(6), 280.0);
        assert_eq!(
            parts,
            [
                Part::Crumb(0),
                Part::Ellipsis,
                Part::Crumb(4),
                Part::Crumb(5)
            ]
        );
        // Less room: the end only, with the top level and "…" gone or kept as far as they fit.
        let (parts, _) = plan_crumbs(&widths(6), 100.0);
        assert_eq!(parts.last(), Some(&Part::Crumb(5)));
        assert!(parts.len() <= 3, "{parts:?}");
    }

    #[test]
    fn a_long_folder_name_is_cut_instead_of_pushing_the_way_back_out() {
        // The top level is 80 wide and the open folder's name 400: there are 250 to spare.
        let (parts, cap) = plan_crumbs(&[80.0, 400.0], 250.0);
        assert_eq!(parts, [Part::Crumb(0), Part::Crumb(1)]);
        assert!((cap - (250.0 - 80.0 - SEPARATOR)).abs() < 0.01, "{cap}");
        // Deeper: the top level stays, and the middle goes before the name is cut below a readable width.
        let (parts, cap) = plan_crumbs(&[80.0, 60.0, 60.0, 400.0], 300.0);
        assert_eq!(parts.first(), Some(&Part::Crumb(0)), "{parts:?}");
        assert_eq!(parts.last(), Some(&Part::Crumb(3)));
        assert!(cap >= PREFERRED_LAST, "{cap}");
        // As narrow as it gets while the top level and a little of the name still fit.
        let (parts, _) = plan_crumbs(&[80.0, 400.0], 80.0 + SEPARATOR + MIN_LAST);
        assert_eq!(parts, [Part::Crumb(0), Part::Crumb(1)]);
    }

    #[test]
    fn the_trail_always_ends_at_the_open_folder_and_fits_or_cuts_the_last_step() {
        for n in 1..8 {
            for available in [0.0, 30.0, 70.0, 120.0, 200.0, 400.0, 900.0] {
                let w = widths(n);
                let (parts, cap) = plan_crumbs(&w, available);
                assert_eq!(parts.last(), Some(&Part::Crumb(n - 1)), "{n} {available}");
                assert!(
                    cap >= MIN_LAST.min(60.0) && cap <= 60.0 + 0.5,
                    "{n} {available}: {cap}"
                );
                // The pieces take no more than there is, unless the last step is already at its least.
                let mut used = 0.0;
                for (position, part) in parts.iter().enumerate() {
                    if position > 0 {
                        used += SEPARATOR;
                    }
                    used += match part {
                        Part::Ellipsis => ELLIPSIS,
                        Part::Crumb(i) if *i == n - 1 => cap,
                        Part::Crumb(_) => 60.0,
                    };
                }
                assert!(
                    used <= available.max(MIN_LAST) + 0.5,
                    "{n} {available}: {used}"
                );
                // The way back stays whenever it and a little of the end fit.
                if n > 1 && available >= 60.0 + SEPARATOR + MIN_LAST {
                    assert_eq!(
                        parts.first(),
                        Some(&Part::Crumb(0)),
                        "{n} {available}: {parts:?}"
                    );
                }
                // No step appears twice, and they run in order.
                let steps: Vec<usize> = parts
                    .iter()
                    .filter_map(|p| {
                        if let Part::Crumb(i) = p {
                            Some(*i)
                        } else {
                            None
                        }
                    })
                    .collect();
                assert!(steps.windows(2).all(|pair| pair[0] < pair[1]), "{steps:?}");
            }
        }
    }

    #[test]
    fn only_an_archive_can_be_extracted() {
        for (kind, extracts) in [
            (FileType::Archive, true),
            (FileType::Video, false),
            (FileType::Folder, false),
            (FileType::File, false),
            (FileType::Unknown, false),
        ] {
            assert_eq!(
                acts(&file(kind, "x")).contains(&Act::Extract),
                extracts,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_row_being_unpacked_says_so_without_the_name_running_into_the_pill() {
        let long = "An archive with a very long name that keeps going and going so that it has to be cut.zip";
        for locale in LOCALES {
            for status in [
                ExtractStatus::InQueue,
                ExtractStatus::Extracting,
                ExtractStatus::Password,
            ] {
                let f = file(FileType::Archive, long);
                let rect = std::cell::Cell::new(Rect::NOTHING);
                let boxes = text_boxes(|ui| {
                    ui.set_width(NARROWEST);
                    rect.set(draw_card(ui, &f, false, Some(status), locale).body.rect);
                });
                let card = rect.get();
                for text in &boxes {
                    assert!(
                        card.expand(0.5).contains_rect(text.visible),
                        "{locale:?} {status:?}: {:?} sticks out",
                        text.text
                    );
                }
                let (msg, _) = unpack_look(status).unwrap();
                let pill = boxes.iter().find(|b| b.text == msg.text(locale)).unwrap();
                let name = boxes
                    .iter()
                    .find(|b| b.text.starts_with("An archive"))
                    .unwrap();
                assert!(
                    name.full.right() + TEXT_GAP - 1.0 <= pill.full.left(),
                    "{locale:?} {status:?}: the name runs into the pill"
                );
                assert!(
                    name.full.width() > 150.0,
                    "{locale:?} {status:?}: too little room for the name"
                );
            }
        }
        // A finished or failed extraction leaves no pill.
        for status in [
            ExtractStatus::Extracted,
            ExtractStatus::Error,
            ExtractStatus::Unknown,
        ] {
            assert!(unpack_look(status).is_none());
        }
    }

    #[test]
    fn a_row_being_unpacked_tells_a_screen_reader() {
        let names = accessible_names(|ui| {
            draw_card(
                ui,
                &file(FileType::Archive, "a.zip"),
                false,
                Some(ExtractStatus::Extracting),
                Locale::En,
            );
        });
        assert!(
            names
                .iter()
                .any(|(_, n)| n.ends_with(", Extracting") && n.starts_with("a.zip, ")),
            "{names:?}"
        );
    }
}
