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

/// The small drawn symbols, at `scale` times their normal size (1.0 fits a 24 px badge).
#[derive(Debug, Clone, Copy)]
pub(crate) enum Glyph {
    Check,
    Cross,
    ArrowDown,
}

pub(crate) fn glyph(painter: &Painter, center: Pos2, glyph: Glyph, scale: f32, color: Color32) {
    let stroke = Stroke::new((2.0 * scale).max(1.4), color);
    let at = |x: f32, y: f32| center + Vec2::new(x, y) * scale;
    match glyph {
        Glyph::Check => {
            painter.line_segment([at(-4.0, 0.5), at(-1.2, 3.3)], stroke);
            painter.line_segment([at(-1.2, 3.3), at(4.2, -3.0)], stroke);
        }
        Glyph::Cross => {
            painter.line_segment([at(-3.2, -3.2), at(3.2, 3.2)], stroke);
            painter.line_segment([at(-3.2, 3.2), at(3.2, -3.2)], stroke);
        }
        Glyph::ArrowDown => {
            painter.line_segment([at(0.0, -4.5), at(0.0, 3.5)], stroke);
            painter.line_segment([at(-3.8, 0.0), at(0.0, 4.0)], stroke);
            painter.line_segment([at(3.8, 0.0), at(0.0, 4.0)], stroke);
        }
    }
}

/// The round badge at the left of a transfer row, by state.
pub(crate) fn status_badge(painter: &Painter, center: Pos2, status: TransferStatus) {
    let radius = 12.0;
    match status {
        TransferStatus::Completed | TransferStatus::Seeding => {
            painter.circle_filled(center, radius, theme::YELLOW);
            glyph(painter, center, Glyph::Check, 1.0, theme::pal().on_yellow);
        }
        TransferStatus::Error => {
            painter.circle_filled(center, radius, theme::pal().error_soft);
            painter.circle_stroke(center, radius, Stroke::new(1.0, theme::pal().error));
            glyph(painter, center, Glyph::Cross, 1.0, theme::pal().error);
        }
        TransferStatus::Downloading | TransferStatus::Completing => {
            painter.circle_filled(center, radius, theme::pal().yellow_soft);
            glyph(
                painter,
                center,
                Glyph::ArrowDown,
                1.0,
                theme::pal().accent_text,
            );
        }
        TransferStatus::Waiting
        | TransferStatus::InQueue
        | TransferStatus::PreparingDownload
        | TransferStatus::Unknown => {
            painter.circle_filled(center, radius, theme::pal().neutral_soft);
            painter.circle_stroke(center, 4.5, Stroke::new(1.6, theme::pal().muted));
        }
    }
}

/// A big round badge for the empty state.
pub(crate) fn empty_badge(painter: &Painter, center: Pos2) {
    painter.circle_filled(center, 26.0, theme::pal().yellow_soft);
    painter.circle_stroke(center, 26.0, Stroke::new(1.5, theme::pal().badge_border));
    let stroke = Stroke::new(3.0, theme::pal().accent_text);
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
    painter.rect_filled(rect, radius, theme::pal().track);
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
