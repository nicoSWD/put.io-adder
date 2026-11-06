//! The "..." menu: a rounded card of icon rows, grouped by hairlines.

use eframe::egui::{self, Align2, CornerRadius, Pos2, RectAlign, Sense, Vec2};
use putio_core::i18n::Msg;
use putio_core::state::Event;
use putio_core::update::UpdateStatus;

use super::icons::{self, MenuIcon};
use super::{theme, update};
use crate::app::App;

const WIDTH: f32 = 236.0;
const ROW_HEIGHT: f32 = 38.0;
const ICON_X: f32 = 20.0;
const TEXT_X: f32 = 42.0;
/// A hairline and the space around it (see [`divider`]).
const DIVIDER_HEIGHT: f32 = 9.0;
/// Where the menu's top edge is, measured down from the top of the window's content: under the "..."
/// button, plus the gap.
const MENU_TOP: f32 = 50.0;
/// The frame around the rows, top and bottom together.
const MENU_PADDING: f32 = 14.0;
/// Space kept under the menu when the window is grown to hold it.
const MENU_BOTTOM_MARGIN: f32 = 12.0;

/// Whether the menu starts with the "new version" row (see [`show`]).
fn has_update_row(state: &putio_core::state::State) -> bool {
    update::look(state).is_some()
}

/// How tall the window has to be for the whole menu to be inside it. A menu is drawn inside the window,
/// so the collapsed window (only the header and the status row) would cut it off. The rows are: Preferences,
/// check for updates, project page, sign out and quit, in three groups; the new-version row and its hairline
/// come first when there is one.
pub(crate) fn window_height(update_row: bool) -> f32 {
    let (rows, dividers) = if update_row { (6.0, 3.0) } else { (5.0, 2.0) };
    MENU_TOP + MENU_PADDING + rows * ROW_HEIGHT + dividers * DIVIDER_HEIGHT + MENU_BOTTOM_MARGIN
}

/// Whether the window has to make room for the menu when it is open: with the new-version row or not.
pub(crate) fn window_room(app: &App) -> Option<bool> {
    app.ui.menu_open.then(|| has_update_row(&app.state))
}

/// How the shortcut for Preferences is written on this platform.
pub(crate) fn preferences_shortcut_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘,"
    } else {
        "Ctrl+,"
    }
}

/// The key combination itself (⌘, on macOS, Ctrl+, elsewhere).
pub(crate) fn preferences_shortcut() -> egui::KeyboardShortcut {
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Comma)
}

/// An item that deserves attention (a new version) is drawn in the accent colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tint {
    Normal,
    Accent,
    /// Removes or stops something.
    Danger,
}

/// One row. Returns whether it was clicked.
pub(crate) fn item(
    ui: &mut egui::Ui,
    icon: MenuIcon,
    label: &str,
    hint: Option<&str>,
    tint: Tint,
) -> bool {
    let p = theme::pal();
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_HEIGHT), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    super::widgets::describe(&response, egui::Role::Button, label);
    super::widgets::focus_ring(ui, &response, rect, CornerRadius::same(9));
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(9), p.card_hover);
    }
    let icon_color = match (tint, response.hovered()) {
        (Tint::Accent, _) => p.accent_text,
        (Tint::Danger, _) => p.error,
        (Tint::Normal, true) => p.text,
        (Tint::Normal, false) => p.muted,
    };
    icons::menu_icon(
        ui.painter(),
        Pos2::new(rect.left() + ICON_X, rect.center().y),
        icon,
        icon_color,
    );
    let (font, color) = match tint {
        Tint::Accent => (theme::bold(12.5), p.accent_text),
        Tint::Normal => (theme::text(12.5), p.text),
        Tint::Danger => (theme::text(12.5), p.error),
    };
    ui.painter().text(
        Pos2::new(rect.left() + TEXT_X, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        color,
    );
    if let Some(hint) = hint {
        ui.painter().text(
            Pos2::new(rect.right() - 14.0, rect.center().y),
            Align2::RIGHT_CENTER,
            hint,
            theme::text(11.0),
            p.muted,
        );
    }
    response.clicked()
}

/// A hairline between groups.
pub(crate) fn divider(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .rect_filled(rect.shrink2(Vec2::new(8.0, 0.0)), 0.0, theme::pal().divider);
    ui.add_space(4.0);
}

/// A quiet title row above the first group (the name of what the menu is about). Not clickable.
pub(crate) fn caption(ui: &mut egui::Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::hover());
    let font = theme::bold(11.5);
    let shown = super::widgets::fit_to_width(ui.painter(), text, &font, rect.width() - 28.0);
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, rect.center().y),
        Align2::LEFT_CENTER,
        shown,
        font,
        theme::pal().muted,
    );
}

/// The menu opened from `button`.
pub(crate) fn show(button: &egui::Response, app: &mut App) {
    let ctx = button.ctx.clone();
    let locale = app.state.locale;
    let signed_in = app.state.is_signed_in();
    let update = app.state.update.clone();
    let update_look = update::look(&app.state);

    let mut popup = egui::Popup::menu(button)
        .align(RectAlign::BOTTOM_END)
        .gap(6.0)
        // Always below the button: the window grows to fit, and a menu that jumped above or beside it
        // while that happens would flicker.
        .align_alternatives(&[])
        .width(WIDTH)
        .frame(super::widgets::popup_frame());
    if app.ui.open_once == Some(super::Overlay::Menu) {
        app.ui.open_once = None;
        // `bool` converts into egui's open command, so this opens the menu without a click.
        popup = popup.open_memory(Some(true.into()));
    }

    let shown = popup.show(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;

        // A new version comes first, because it is the only thing here that asks for attention.
        if let (Some(look), UpdateStatus::Available { version, .. }) =
            (update_look.as_ref(), &update)
        {
            let tint = match look.tone {
                update::Tone::Accent => Tint::Accent,
                update::Tone::Danger => Tint::Danger,
                update::Tone::Busy | update::Tone::Quiet => Tint::Normal,
            };
            if item(ui, look.icon, &look.label, Some(version), tint)
                && let Some(click) = look.click
            {
                app.dispatch(&ctx, click.event());
            }
            divider(ui);
        }

        if item(
            ui,
            MenuIcon::Settings,
            &Msg::MenuPreferences.text(locale),
            Some(preferences_shortcut_label()),
            Tint::Normal,
        ) {
            app.ui.prefs_open = true;
        }
        divider(ui);
        if item(
            ui,
            MenuIcon::Refresh,
            &Msg::MenuCheckUpdates.text(locale),
            None,
            Tint::Normal,
        ) {
            app.dispatch(
                &ctx,
                Event::CheckForUpdates {
                    user_initiated: true,
                },
            );
        }
        if item(
            ui,
            MenuIcon::External,
            &Msg::MenuProjectPage.text(locale),
            None,
            Tint::Normal,
        ) {
            app.dispatch(&ctx, Event::OpenProjectPage);
        }
        divider(ui);
        if signed_in
            && item(
                ui,
                MenuIcon::SignOut,
                &Msg::MenuSignOut.text(locale),
                None,
                Tint::Normal,
            )
        {
            app.dispatch(&ctx, Event::SignOutRequested);
        }
        if item(
            ui,
            MenuIcon::Power,
            &Msg::MenuQuit.text(locale),
            None,
            Tint::Normal,
        ) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    });
    app.ui.menu_open = shown.is_some();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::test_support::{accessible_names, text_with_and_without_hover};

    #[test]
    fn a_menu_row_is_a_button_with_its_label() {
        let names = accessible_names(|ui| {
            item(
                ui,
                MenuIcon::Settings,
                "Preferences",
                Some("⌘,"),
                Tint::Normal,
            );
        });
        assert!(
            names
                .iter()
                .any(|(r, n)| r == "Button" && n == "Preferences"),
            "{names:?}"
        );
    }

    #[test]
    fn the_window_makes_room_for_the_menu_and_for_the_update_row() {
        // 5 rows and 2 hairlines under the button, with the frame and a margin.
        let plain = window_height(false);
        assert_eq!(
            plain,
            MENU_TOP + MENU_PADDING + 5.0 * ROW_HEIGHT + 2.0 * DIVIDER_HEIGHT + MENU_BOTTOM_MARGIN
        );
        // The new-version row adds a row and a hairline.
        assert_eq!(window_height(true) - plain, ROW_HEIGHT + DIVIDER_HEIGHT);
        // The divider really is as tall as the constant says.
        assert_eq!(DIVIDER_HEIGHT, 4.0 + 1.0 + 4.0);
    }

    #[test]
    fn the_shortcut_label_matches_the_platform() {
        let label = preferences_shortcut_label();
        if cfg!(target_os = "macos") {
            assert_eq!(label, "⌘,");
        } else {
            assert_eq!(label, "Ctrl+,");
        }
        assert!(label.ends_with(','));
    }

    #[test]
    fn the_shortcut_is_the_platform_command_key_plus_comma() {
        let shortcut = preferences_shortcut();
        assert_eq!(shortcut.logical_key, egui::Key::Comma);
        assert!(shortcut.modifiers.command);
    }

    #[test]
    fn a_menu_row_does_not_move_its_text_when_hovered() {
        for (tint, hint) in [
            (Tint::Normal, None),
            (Tint::Normal, Some("⌘,")),
            (Tint::Accent, None),
        ] {
            let (resting, hovered) = text_with_and_without_hover(|ui| {
                let origin = ui.cursor().min;
                item(ui, MenuIcon::Settings, "Preferences…", hint, tint);
                let rect =
                    egui::Rect::from_min_size(origin, Vec2::new(ui.available_width(), ROW_HEIGHT));
                (rect, ui.rect_contains_pointer(rect))
            });
            assert_eq!(
                resting, hovered,
                "text moved on hover ({tint:?}, hint {hint:?})"
            );
        }
    }
}
