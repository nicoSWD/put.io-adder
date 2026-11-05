//! Top of the window: logo and account, then the black strip with the chevron, the gold quota bar
//! and the status line.

use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use putio_core::format;
use putio_core::i18n::Msg;
use putio_core::state::Event;

use super::{STRIP_HEIGHT, TOP_HEIGHT, WIDTH, theme};
use crate::app::App;

const BAR_X: f32 = 35.0;
const BAR_WIDTH: f32 = 150.0;

#[allow(clippy::too_many_lines)] // immediate-mode drawing reads best as one top-to-bottom function
pub(crate) fn show(ui: &mut egui::Ui, app: &mut App) {
    let ctx = ui.ctx().clone();
    let origin = ui.max_rect().min;
    let now = app.now();
    let locale = app.state.locale;

    // ---- top row: logo on the left, account and menu on the right (dark: the logo is white and gold) ----
    let top = Rect::from_min_size(origin, Vec2::new(WIDTH, TOP_HEIGHT));
    ui.painter()
        .rect_filled(top, CornerRadius::ZERO, theme::TOP);
    let logo_height = 22.0;
    let logo = Rect::from_min_size(
        origin + Vec2::new(14.0, (TOP_HEIGHT - logo_height) / 2.0),
        Vec2::new(logo_height * theme::LOGO_ASPECT, logo_height),
    );
    egui::Image::from_bytes("bytes://putio-logo.png", theme::LOGO_PNG)
        .fit_to_exact_size(logo.size())
        .paint_at(ui, logo);
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(top.shrink2(Vec2::new(10.0, 6.0))),
        |ui| {
            ui.visuals_mut().override_text_color = Some(Color32::WHITE);
            ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                menu(ui, app);
                if let Some(account) = &app.state.account {
                    ui.label(egui::RichText::new(&account.username).font(theme::bold(13.0)));
                }
            });
        },
    );

    // ---- the black strip ----
    let strip = Rect::from_min_size(
        origin + Vec2::new(0.0, TOP_HEIGHT),
        Vec2::new(WIDTH, STRIP_HEIGHT),
    );
    ui.painter()
        .rect_filled(strip, CornerRadius::ZERO, theme::STRIP);

    // Chevron (rotates when the list opens). Only while signed in; signing in needs no list.
    let center = Pos2::new(strip.left() + 17.0, strip.center().y);
    let open = app.state.transfers_expanded;
    let turn = ctx.animate_bool_with_time(egui::Id::new("chevron"), open, 0.4);
    let angle = turn * std::f32::consts::PI;
    let rotate = |v: Vec2| {
        Vec2::new(
            v.x * angle.cos() - v.y * angle.sin(),
            v.x * angle.sin() + v.y * angle.cos(),
        )
    };
    let points = [
        center + rotate(Vec2::new(-5.0, -3.0)),
        center + rotate(Vec2::new(0.0, 3.0)),
        center + rotate(Vec2::new(5.0, -3.0)),
    ];
    let stroke = Stroke::new(2.0, Color32::WHITE);
    ui.painter().line_segment([points[0], points[1]], stroke);
    ui.painter().line_segment([points[1], points[2]], stroke);
    if app.state.is_signed_in() {
        let click = ui.interact(
            Rect::from_center_size(center, Vec2::splat(26.0)),
            egui::Id::new("toggle"),
            Sense::click(),
        );
        let click = click
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(Msg::TooltipToggleTransfers.text(locale));
        if click.clicked() {
            app.dispatch(&ctx, Event::ToggleTransfers);
        }
    }

    // Gold bar: how much of the disk quota is left, or this month's bandwidth when no quota is known.
    let bar = Rect::from_min_size(
        Pos2::new(strip.left() + BAR_X, strip.top()),
        Vec2::new(BAR_WIDTH, STRIP_HEIGHT),
    );
    let (fraction, label) = match &app.state.account {
        Some(account) => match account
            .disk
            .as_ref()
            .and_then(putio_api::Disk::used_fraction)
        {
            Some(used) => {
                let left = account
                    .disk
                    .map_or(0, |disk| disk.avail.max(disk.size - disk.used));
                (
                    used,
                    Msg::DiskLeft {
                        left: format::bytes(left),
                    }
                    .text(locale),
                )
            }
            None => (
                1.0,
                Msg::Bandwidth {
                    used: format::bytes(account.monthly_bandwidth_usage),
                }
                .text(locale),
            ),
        },
        None => (0.0, String::new()),
    };
    let shown = ctx.animate_value_with_time(egui::Id::new("quota"), fraction, 2.0);
    ui.painter()
        .rect_filled(bar, CornerRadius::ZERO, theme::GOLD_BACK);
    ui.painter().rect_filled(
        Rect::from_min_size(bar.min, Vec2::new(bar.width() * shown, bar.height())),
        CornerRadius::ZERO,
        theme::GOLD_FILL,
    );
    ui.painter().text(
        bar.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(11.5),
        Color32::BLACK,
    );

    // Status line, clipped so a long message never runs under the spinner.
    let text_left = bar.right() + 8.0;
    let text_area = Rect::from_min_max(
        Pos2::new(text_left, strip.top()),
        Pos2::new(strip.right() - 30.0, strip.bottom()),
    );
    ui.painter().with_clip_rect(text_area).text(
        Pos2::new(text_left, strip.center().y),
        Align2::LEFT_CENTER,
        app.state.status_text(now),
        FontId::proportional(11.0),
        Color32::WHITE,
    );
    if app.state.is_busy() {
        let spinner = Rect::from_center_size(
            Pos2::new(strip.right() - 16.0, strip.center().y),
            Vec2::splat(14.0),
        );
        ui.put(
            spinner,
            egui::Spinner::new().size(14.0).color(Color32::WHITE),
        );
    }

    ui.allocate_rect(
        Rect::from_min_size(origin, Vec2::new(WIDTH, TOP_HEIGHT + STRIP_HEIGHT)),
        Sense::hover(),
    );
}

fn menu(ui: &mut egui::Ui, app: &mut App) {
    let ctx = ui.ctx().clone();
    let locale = app.state.locale;
    ui.menu_button("…", |ui| {
        if ui.button(Msg::MenuPreferences.text(locale)).clicked() {
            app.ui.prefs_open = true;
            ui.close();
        }
        if ui.button(Msg::MenuCheckUpdates.text(locale)).clicked() {
            app.dispatch(
                &ctx,
                Event::CheckForUpdates {
                    user_initiated: true,
                },
            );
            ui.close();
        }
        if ui.button(Msg::MenuProjectPage.text(locale)).clicked() {
            app.dispatch(&ctx, Event::OpenProjectPage);
            ui.close();
        }
        if app.state.is_signed_in() {
            ui.separator();
            if ui.button(Msg::MenuSignOut.text(locale)).clicked() {
                app.dispatch(&ctx, Event::SignOutRequested);
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Quit").clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    });
}
