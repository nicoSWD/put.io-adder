//! The top of the window: logo and account on black, then a status row with the list toggle, the
//! current message and the quota pill.

use eframe::egui::{
    self, Align2, CornerRadius, Pos2, Rect, RectAlign, Sense, Stroke, StrokeKind, Vec2,
};
use putio_core::format;
use putio_core::i18n::Msg;
use putio_core::state::Event;

use super::{STRIP_HEIGHT, TOP_HEIGHT, WIDTH, icons, theme};
use crate::app::App;

const PAD: f32 = 16.0;
const PILL_WIDTH: f32 = 156.0;
const PILL_HEIGHT: f32 = 22.0;

pub(crate) fn show(ui: &mut egui::Ui, app: &mut App) {
    let origin = ui.max_rect().min;
    top_row(ui, app, origin);
    status_row(ui, app, origin + Vec2::new(0.0, TOP_HEIGHT));
    ui.allocate_rect(
        Rect::from_min_size(origin, Vec2::new(WIDTH, TOP_HEIGHT + STRIP_HEIGHT)),
        Sense::hover(),
    );
}

fn top_row(ui: &mut egui::Ui, app: &mut App, origin: Pos2) {
    let top = Rect::from_min_size(origin, Vec2::new(WIDTH, TOP_HEIGHT));
    ui.painter()
        .rect_filled(top, CornerRadius::ZERO, theme::pal().header);
    ui.painter().line_segment(
        [top.left_bottom(), top.right_bottom()],
        Stroke::new(1.0, theme::pal().divider),
    );

    // Logo (white with a gold shadow, so it lives on the black row).
    let logo_height = 40.0;
    let logo = Rect::from_min_size(
        origin + Vec2::new(PAD, (TOP_HEIGHT - logo_height) / 2.0),
        Vec2::new(logo_height * theme::LOGO_ASPECT, logo_height),
    );
    egui::Image::from_bytes(theme::pal().logo_uri, theme::pal().logo)
        .fit_to_exact_size(logo.size())
        .paint_at(ui, logo);

    // Menu button, at the far right.
    let menu_rect = Rect::from_center_size(
        Pos2::new(top.right() - PAD - 14.0, top.center().y),
        Vec2::splat(32.0),
    );
    let menu = ui
        .interact(menu_rect, egui::Id::new("menu-button"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if menu.hovered() {
        ui.painter()
            .rect_filled(menu_rect, CornerRadius::same(8), theme::pal().card_hover);
    }
    icons::dots(
        ui.painter(),
        menu_rect.center(),
        if menu.hovered() {
            theme::pal().text
        } else {
            theme::pal().muted
        },
    );
    menu_popup(&menu, app);

    // Account: name, then a yellow avatar with the initial. (A Gravatar would need your e-mail hash
    // sent to a third party, so the initial is the default.)
    if let Some(account) = &app.state.account {
        let avatar = Pos2::new(menu_rect.left() - 10.0 - 14.0, top.center().y);
        ui.painter().circle_filled(avatar, 14.0, theme::YELLOW);
        let initial = account
            .username
            .chars()
            .next()
            .map_or_else(|| "?".into(), |c| c.to_uppercase().to_string());
        ui.painter().text(
            avatar + Vec2::new(0.0, 0.5),
            Align2::CENTER_CENTER,
            initial,
            theme::bold(13.0),
            theme::pal().on_yellow,
        );
        ui.painter().text(
            Pos2::new(avatar.x - 22.0, top.center().y),
            Align2::RIGHT_CENTER,
            &account.username,
            theme::bold(13.0),
            theme::pal().text,
        );
    }
}

fn menu_popup(button: &egui::Response, app: &mut App) {
    let ctx = button.ctx.clone();
    let locale = app.state.locale;
    egui::Popup::menu(button)
        .align(RectAlign::BOTTOM_END)
        .width(210.0)
        .show(|ui| {
            if ui.button(Msg::MenuPreferences.text(locale)).clicked() {
                app.ui.prefs_open = true;
            }
            if ui.button(Msg::MenuCheckUpdates.text(locale)).clicked() {
                app.dispatch(
                    &ctx,
                    Event::CheckForUpdates {
                        user_initiated: true,
                    },
                );
            }
            if ui.button(Msg::MenuProjectPage.text(locale)).clicked() {
                app.dispatch(&ctx, Event::OpenProjectPage);
            }
            if app.state.is_signed_in() {
                ui.separator();
                if ui.button(Msg::MenuSignOut.text(locale)).clicked() {
                    app.dispatch(&ctx, Event::SignOutRequested);
                }
            }
            ui.separator();
            if ui.button("Quit").clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
}

fn status_row(ui: &mut egui::Ui, app: &mut App, origin: Pos2) {
    let ctx = ui.ctx().clone();
    let locale = app.state.locale;
    let now = app.now();
    let strip = Rect::from_min_size(origin, Vec2::new(WIDTH, STRIP_HEIGHT));
    ui.painter()
        .rect_filled(strip, CornerRadius::ZERO, theme::pal().subheader);
    ui.painter().line_segment(
        [strip.left_bottom(), strip.right_bottom()],
        Stroke::new(1.0, theme::pal().divider),
    );

    // The list toggle.
    let mut text_left = PAD;
    if app.state.is_signed_in() {
        let center = Pos2::new(strip.left() + PAD + 8.0, strip.center().y);
        let hit = ui.interact(
            Rect::from_center_size(center, Vec2::splat(28.0)),
            egui::Id::new("toggle"),
            Sense::click(),
        );
        let hit = hit
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(Msg::TooltipToggleTransfers.text(locale));
        let turn = ctx.animate_bool_with_time(
            egui::Id::new("chevron"),
            app.state.transfers_expanded,
            0.25,
        );
        icons::chevron(
            ui.painter(),
            center,
            turn,
            if hit.hovered() {
                theme::pal().text
            } else {
                theme::pal().muted
            },
        );
        if hit.clicked() {
            app.dispatch(&ctx, Event::ToggleTransfers);
        }
        text_left += 28.0;
    }

    // The quota pill on the right.
    let pill = Rect::from_center_size(
        Pos2::new(strip.right() - PAD - PILL_WIDTH / 2.0, strip.center().y),
        Vec2::new(PILL_WIDTH, PILL_HEIGHT),
    );
    quota_pill(ui, app, pill);

    if app.state.is_busy() {
        let spinner = Rect::from_center_size(
            Pos2::new(pill.left() - 18.0, strip.center().y),
            Vec2::splat(14.0),
        );
        ui.put(
            spinner,
            egui::Spinner::new()
                .size(14.0)
                .color(theme::pal().accent_text),
        );
    }

    // The message, clipped so it never runs under the pill.
    let area = Rect::from_min_max(
        Pos2::new(strip.left() + text_left, strip.top()),
        Pos2::new(pill.left() - 26.0, strip.bottom()),
    );
    if matches!(app.state.status(now), Msg::Summary { .. }) {
        // The standing message is the transfers summary: show it as chips.
        super::summary::show(ui, app, area);
    } else {
        // A notice ("Added!", an error, a sign-in step) takes the row over for a few seconds.
        ui.painter().with_clip_rect(area).text(
            Pos2::new(area.left(), strip.center().y),
            Align2::LEFT_CENTER,
            app.state.status_text(now),
            theme::text(12.0),
            theme::pal().text,
        );
    }
}

/// Disk space used (a filled pill) or, when put.io reports no quota, this month's bandwidth (an outline).
fn quota_pill(ui: &egui::Ui, app: &App, pill: Rect) {
    let Some(account) = &app.state.account else {
        return;
    };
    let locale = app.state.locale;
    let painter = ui.painter();
    let radius = CornerRadius::same(11);
    if let Some(used) = account
        .disk
        .as_ref()
        .and_then(putio_api::Disk::used_fraction)
    {
        let shown = ui
            .ctx()
            .animate_value_with_time(egui::Id::new("quota"), used, 1.2);
        painter.rect_filled(pill, radius, theme::pal().track);
        let fill = Rect::from_min_size(
            pill.min,
            Vec2::new((pill.width() * shown).max(PILL_HEIGHT), pill.height()),
        );
        painter.rect_filled(fill, radius, theme::YELLOW);
        let left = account
            .disk
            .map_or(0, |disk| disk.avail.max(disk.size - disk.used));
        // Black over the yellow part, the normal text colour over the rest, so it reads wherever the
        // edge falls (on the light palette both are black).
        let label = Msg::DiskLeft {
            left: format::bytes(left),
        }
        .text(locale);
        let font = theme::bold(10.5);
        let rest = Rect::from_min_max(Pos2::new(fill.right(), pill.top()), pill.right_bottom());
        painter.with_clip_rect(fill).text(
            pill.center(),
            Align2::CENTER_CENTER,
            &label,
            font.clone(),
            theme::pal().on_yellow,
        );
        painter.with_clip_rect(rest).text(
            pill.center(),
            Align2::CENTER_CENTER,
            &label,
            font,
            theme::pal().text,
        );
    } else {
        painter.rect_stroke(
            pill,
            radius,
            Stroke::new(1.5, theme::pal().outline),
            StrokeKind::Inside,
        );
        painter.text(
            pill.center(),
            Align2::CENTER_CENTER,
            Msg::Bandwidth {
                used: format::bytes(account.monthly_bandwidth_usage),
            }
            .text(locale),
            theme::bold(10.5),
            theme::pal().accent_text,
        );
    }
}
