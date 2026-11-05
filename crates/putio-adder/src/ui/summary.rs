//! The transfers summary in the status row: small colour-coded chips instead of a sentence.
//!
//! `⬇ 4 active · 8.5 MB/s`   `✓ 2 done`   `✕ 1 failed`
//!
//! Only chips with something to show appear. When the row is too narrow the speed takes the place of the
//! word "active", then goes away, then the words go too, so the numbers always fit. The full sentence is the tooltip.

use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, Sense, Vec2};
use putio_core::format;
use putio_core::i18n::Msg;

use super::icons::{self, Glyph};
use super::theme;
use crate::app::App;

const CHIP_HEIGHT: f32 = 24.0;
const GAP: f32 = 8.0;
const PAD: f32 = 10.0;
const ICON_SLOT: f32 = 12.0;
const ICON_GAP: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Active,
    Done,
    Failed,
}

/// How much of a chip to show, from everything to the bare number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Detail {
    /// `4 active · 8.5 MB/s`
    Full,
    /// `4 · 8.5 MB/s`: the speed replaces the word on the active chip.
    SpeedInsteadOfWord,
    /// `4 active`
    NoSpeed,
    /// `4`
    NumberOnly,
}

struct Chip {
    kind: Kind,
    count: usize,
    speed: Option<String>,
}

fn chips(app: &App) -> Vec<Chip> {
    let summary = app.state.summary;
    [
        (Kind::Active, summary.pending),
        (Kind::Done, summary.finished),
        (Kind::Failed, summary.failed),
    ]
    .into_iter()
    .filter(|(_, count)| *count > 0)
    .map(|(kind, count)| Chip {
        kind,
        count,
        speed: (kind == Kind::Active && summary.down_speed > 0)
            .then(|| format::speed(summary.down_speed)),
    })
    .collect()
}

fn label(kind: Kind, locale: putio_core::i18n::Locale) -> String {
    match kind {
        Kind::Active => Msg::ChipActive,
        Kind::Done => Msg::ChipDone,
        Kind::Failed => Msg::ChipFailed,
    }
    .text(locale)
}

/// Fill, icon colour and glyph of a chip.
fn look(kind: Kind) -> (Color32, Color32, Glyph) {
    let p = theme::pal();
    match kind {
        Kind::Active => (p.yellow_soft, p.accent_text, Glyph::ArrowDown),
        Kind::Done => (p.neutral_soft, p.text, Glyph::Check),
        Kind::Failed => (p.error_soft, p.error, Glyph::Cross),
    }
}

struct Laid {
    count: Arc<egui::Galley>,
    words: Option<Arc<egui::Galley>>,
    width: f32,
}

use std::sync::Arc;

fn lay_out(ui: &egui::Ui, chip: &Chip, detail: Detail, locale: putio_core::i18n::Locale) -> Laid {
    let p = theme::pal();
    let painter = ui.painter();
    let count = painter.layout_no_wrap(chip.count.to_string(), theme::bold(11.5), p.text);
    let words = match detail {
        Detail::NumberOnly => None,
        Detail::Full | Detail::SpeedInsteadOfWord | Detail::NoSpeed => {
            let word = label(chip.kind, locale);
            let text = match (detail, &chip.speed) {
                (Detail::Full, Some(speed)) => format!("{word}  ·  {speed}"),
                (Detail::SpeedInsteadOfWord, Some(speed)) => speed.clone(),
                _ => word,
            };
            Some(painter.layout_no_wrap(text, theme::text(11.0), p.muted))
        }
    };
    let words_width = words.as_ref().map_or(0.0, |w| w.size().x + 5.0);
    Laid {
        width: PAD + ICON_SLOT + ICON_GAP + count.size().x + words_width + PAD,
        count,
        words,
    }
}

/// Width of chips of the given widths laid in a row with a gap between them.
fn row_width(widths: &[f32]) -> f32 {
    widths
        .iter()
        .enumerate()
        .map(|(index, width)| if index == 0 { *width } else { *width + GAP })
        .sum()
}

/// Paint the summary inside `area` (left aligned, vertically centred).
pub(crate) fn show(ui: &egui::Ui, app: &App, area: Rect) {
    let locale = app.state.locale;
    let p = theme::pal();
    let chips = chips(app);
    if chips.is_empty() {
        ui.painter().with_clip_rect(area).text(
            Pos2::new(area.left(), area.center().y),
            Align2::LEFT_CENTER,
            Msg::EmptyTitle.text(locale),
            theme::text(12.0),
            p.muted,
        );
        return;
    }

    // The most detailed layout that fits.
    let mut laid = Vec::new();
    for detail in [
        Detail::Full,
        Detail::SpeedInsteadOfWord,
        Detail::NoSpeed,
        Detail::NumberOnly,
    ] {
        laid = chips
            .iter()
            .map(|chip| lay_out(ui, chip, detail, locale))
            .collect();
        let total = row_width(&laid.iter().map(|l| l.width).collect::<Vec<_>>());
        if total <= area.width() {
            break;
        }
    }

    let painter = ui.painter().with_clip_rect(area);
    let mut x = area.left();
    for (chip, laid) in chips.iter().zip(laid) {
        let rect = Rect::from_min_size(
            Pos2::new(x, area.center().y - CHIP_HEIGHT / 2.0),
            Vec2::new(laid.width, CHIP_HEIGHT),
        );
        let (fill, icon_color, glyph) = look(chip.kind);
        painter.rect_filled(rect, CornerRadius::same(12), fill);
        icons::glyph(
            &painter,
            Pos2::new(rect.left() + PAD + ICON_SLOT / 2.0, rect.center().y),
            glyph,
            0.62,
            icon_color,
        );
        let mut text_x = rect.left() + PAD + ICON_SLOT + ICON_GAP;
        let count_width = laid.count.size().x;
        painter.galley(
            Pos2::new(text_x, rect.center().y - laid.count.size().y / 2.0),
            laid.count,
            p.text,
        );
        text_x += count_width + 5.0;
        if let Some(words) = laid.words {
            painter.galley(
                Pos2::new(text_x, rect.center().y - words.size().y / 2.0),
                words,
                p.muted,
            );
        }
        x += laid.width + GAP;
    }

    // The whole sentence, for anyone who wants it.
    let shown = Rect::from_min_max(area.min, Pos2::new((x - GAP).min(area.right()), area.max.y));
    ui.interact(shown, egui::Id::new("summary-chips"), Sense::hover())
        .on_hover_text(app.state.status_text(app.now()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_of_chips_counts_one_gap_between_neighbours() {
        assert!((row_width(&[]) - 0.0).abs() < f32::EPSILON);
        assert!((row_width(&[100.0]) - 100.0).abs() < f32::EPSILON);
        assert!((row_width(&[100.0, 50.0, 30.0]) - (180.0 + 2.0 * GAP)).abs() < f32::EPSILON);
    }

    #[test]
    fn every_kind_has_a_distinct_look() {
        let looks: Vec<_> = [Kind::Active, Kind::Done, Kind::Failed]
            .into_iter()
            .map(|k| look(k).0)
            .collect();
        assert_ne!(looks[0], looks[1]);
        assert_ne!(looks[1], looks[2]);
        assert_ne!(looks[0], looks[2]);
    }
}
