//! Small icons drawn with the painter, so there is no icon font to ship or to be missing.

use eframe::egui::{Color32, Painter, Pos2, Rect, Stroke, Vec2};
use putio_api::TransferStatus;

use super::theme;

/// Three dots (a menu button).
pub(crate) fn dots(painter: &Painter, center: Pos2, color: Color32) {
    for dx in [-5.5, 0.0, 5.5] {
        painter.circle_filled(center + Vec2::new(dx, 0.0), 1.9, color);
    }
}

/// A chevron pointing down, rotated by `turn` half-turns (0 = down, 1 = up).
pub(crate) fn chevron(painter: &Painter, center: Pos2, turn: f32, color: Color32) {
    let angle = turn * std::f32::consts::PI;
    let (sin, cos) = angle.sin_cos();
    let rotate = |v: Vec2| Vec2::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos);
    let a = center + rotate(Vec2::new(-5.0, -2.5));
    let b = center + rotate(Vec2::new(0.0, 2.5));
    let c = center + rotate(Vec2::new(5.0, -2.5));
    let stroke = Stroke::new(2.0, color);
    painter.line_segment([a, b], stroke);
    painter.line_segment([b, c], stroke);
}

fn check(painter: &Painter, center: Pos2, color: Color32) {
    let stroke = Stroke::new(2.0, color);
    painter.line_segment(
        [center + Vec2::new(-4.0, 0.5), center + Vec2::new(-1.2, 3.3)],
        stroke,
    );
    painter.line_segment(
        [center + Vec2::new(-1.2, 3.3), center + Vec2::new(4.2, -3.0)],
        stroke,
    );
}

fn cross(painter: &Painter, center: Pos2, color: Color32) {
    let stroke = Stroke::new(2.0, color);
    painter.line_segment(
        [center + Vec2::new(-3.2, -3.2), center + Vec2::new(3.2, 3.2)],
        stroke,
    );
    painter.line_segment(
        [center + Vec2::new(-3.2, 3.2), center + Vec2::new(3.2, -3.2)],
        stroke,
    );
}

fn arrow_down(painter: &Painter, center: Pos2, color: Color32) {
    let stroke = Stroke::new(2.0, color);
    painter.line_segment(
        [center + Vec2::new(0.0, -4.5), center + Vec2::new(0.0, 3.5)],
        stroke,
    );
    painter.line_segment(
        [center + Vec2::new(-3.8, 0.0), center + Vec2::new(0.0, 4.0)],
        stroke,
    );
    painter.line_segment(
        [center + Vec2::new(3.8, 0.0), center + Vec2::new(0.0, 4.0)],
        stroke,
    );
}

/// The round badge at the left of a transfer row, by state.
pub(crate) fn status_badge(painter: &Painter, center: Pos2, status: TransferStatus) {
    let radius = 12.0;
    match status {
        TransferStatus::Completed | TransferStatus::Seeding => {
            painter.circle_filled(center, radius, theme::YELLOW);
            check(painter, center, Color32::BLACK);
        }
        TransferStatus::Error => {
            painter.circle_filled(center, radius, Color32::from_rgb(0x3A, 0x1E, 0x1B));
            painter.circle_stroke(center, radius, Stroke::new(1.0, theme::ERROR));
            cross(painter, center, theme::ERROR);
        }
        TransferStatus::Downloading | TransferStatus::Completing => {
            painter.circle_filled(center, radius, Color32::from_rgb(0x2E, 0x28, 0x10));
            arrow_down(painter, center, theme::YELLOW);
        }
        TransferStatus::Waiting
        | TransferStatus::InQueue
        | TransferStatus::PreparingDownload
        | TransferStatus::Unknown => {
            painter.circle_filled(center, radius, Color32::from_rgb(0x26, 0x26, 0x26));
            painter.circle_stroke(center, 4.5, Stroke::new(1.6, theme::MUTED));
        }
    }
}

/// A big round badge for the empty state.
pub(crate) fn empty_badge(painter: &Painter, center: Pos2) {
    painter.circle_filled(center, 26.0, Color32::from_rgb(0x2E, 0x28, 0x10));
    painter.circle_stroke(
        center,
        26.0,
        Stroke::new(1.0, Color32::from_rgb(0x5A, 0x4A, 0x1A)),
    );
    let stroke = Stroke::new(3.0, theme::YELLOW);
    painter.line_segment(
        [center + Vec2::new(0.0, -9.0), center + Vec2::new(0.0, 8.0)],
        stroke,
    );
    painter.line_segment(
        [center + Vec2::new(-7.5, 1.0), center + Vec2::new(0.0, 8.5)],
        stroke,
    );
    painter.line_segment(
        [center + Vec2::new(7.5, 1.0), center + Vec2::new(0.0, 8.5)],
        stroke,
    );
}

/// A slim rounded progress bar: `fraction` in `0.0..=1.0` painted over an empty track.
pub(crate) fn progress_bar(painter: &Painter, rect: Rect, fraction: f32, fill: Color32) {
    let radius = rect.height() / 2.0;
    painter.rect_filled(rect, radius, theme::TRACK);
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction > 0.0 {
        // Never narrower than the bar is tall, so a tiny fraction still shows as a dot.
        let width = (rect.width() * fraction).max(rect.height());
        painter.rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(width, rect.height())),
            radius,
            fill,
        );
    }
}
