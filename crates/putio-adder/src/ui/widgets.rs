//! Reusable controls for settings screens: switches, a segmented control, grouped rows, a close button.

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, Layout, Pos2, Rect, Response, RichText, Sense,
    Stroke, Vec2,
};

use super::theme;

const SWITCH_SIZE: Vec2 = Vec2::new(40.0, 24.0);
/// Minimum height of a settings row.
const ROW_HEIGHT: f32 = 50.0;

/// An on/off switch. Returns the response; `changed()` is set when it was flipped.
pub(crate) fn switch(ui: &mut egui::Ui, on: &mut bool) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(SWITCH_SIZE, Sense::click());
    let response_cursor = response
        .clone()
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if response_cursor.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, "")
    });
    let t = ui.ctx().animate_bool_with_time(response.id, *on, 0.14);
    let p = theme::pal();
    let track = p.track.lerp_to_gamma(theme::YELLOW, t);
    let radius = rect.height() / 2.0;
    ui.painter().rect_filled(rect, radius, track);
    if response.has_focus() || response.hovered() {
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, p.outline),
            egui::StrokeKind::Outside,
        );
    }
    let knob_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
    let knob = Pos2::new(knob_x, rect.center().y);
    ui.painter().circle_filled(
        knob + Vec2::new(0.0, 1.0),
        radius - 3.0,
        Color32::from_black_alpha(40),
    );
    ui.painter()
        .circle_filled(knob, radius - 3.0, Color32::WHITE);
    response
}

/// A row of mutually exclusive options. Returns the newly picked value, if it changed.
pub(crate) fn segmented<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    options: &[(T, String)],
    current: T,
) -> Option<T> {
    let p = theme::pal();
    let height = 32.0;
    let pad = 16.0;
    let inner = 3.0;
    let galleys: Vec<_> = options
        .iter()
        .map(|(value, label)| {
            let font = if *value == current {
                theme::bold(12.0)
            } else {
                theme::text(12.0)
            };
            ui.painter().layout_no_wrap(label.clone(), font, p.text)
        })
        .collect();
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x + pad * 2.0).collect();
    let total: f32 = widths.iter().sum::<f32>() + inner * 2.0;
    let (outer, _) = ui.allocate_exact_size(Vec2::new(total, height), Sense::hover());
    ui.painter()
        .rect_filled(outer, CornerRadius::same(10), p.track.gamma_multiply(0.55));

    let mut picked = None;
    let mut x = outer.left() + inner;
    for ((value, label), (galley, width)) in options.iter().zip(galleys.into_iter().zip(widths)) {
        let rect = Rect::from_min_size(
            Pos2::new(x, outer.top() + inner),
            Vec2::new(width, height - inner * 2.0),
        );
        let response = ui
            .interact(rect, ui.id().with(label), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let selected = *value == current;
        if selected {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(8), theme::YELLOW);
        } else if response.hovered() {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(8), p.card_hover);
        }
        let color = if selected {
            p.on_yellow
        } else if response.hovered() {
            p.text
        } else {
            p.muted
        };
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, color);
        if response.clicked() && !selected {
            picked = Some(*value);
        }
        x += width;
    }
    picked
}

/// A small caption above a group.
pub(crate) fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(
        RichText::new(text.to_uppercase())
            .font(theme::bold(10.5))
            .color(theme::pal().muted),
    );
    ui.add_space(-2.0);
}

/// A rounded group holding rows. The rows are separated by hairlines.
pub(crate) fn group(ui: &mut egui::Ui, rows: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(theme::pal().inset)
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(16, 2))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            rows(ui);
        });
}

/// A hairline between two rows of a group.
pub(crate) fn divider(ui: &mut egui::Ui) {
    let rect = ui
        .allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover())
        .0;
    ui.painter().rect_filled(rect, 0.0, theme::pal().divider);
}

/// One row: a title (and optional hint) on the left, the control on the right.
pub(crate) fn row(
    ui: &mut egui::Ui,
    title: &str,
    hint: Option<&str>,
    control: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.set_min_height(ROW_HEIGHT);
        ui.vertical(|ui| {
            ui.set_max_width((ui.available_width() - 76.0).max(160.0));
            // A title with a hint is padded evenly; a lone title gets more, which centres it in the row.
            let pad = if hint.is_some() { 8.0 } else { 17.0 };
            ui.add_space(pad);
            ui.label(RichText::new(title).font(theme::bold(12.5)));
            if let Some(hint) = hint {
                ui.add(
                    egui::Label::new(
                        RichText::new(hint)
                            .font(theme::text(10.5))
                            .color(theme::pal().muted),
                    )
                    .wrap(),
                );
            }
            ui.add_space(pad);
        });
        ui.with_layout(Layout::right_to_left(Align::Center), control);
    });
}

/// The round "x" at the top right of a dialog. Returns whether it was clicked.
pub(crate) fn close_button(ui: &mut egui::Ui) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let p = theme::pal();
    if response.hovered() {
        ui.painter()
            .circle_filled(rect.center(), 15.0, p.card_hover);
    }
    let stroke = Stroke::new(1.8, if response.hovered() { p.text } else { p.muted });
    let c = rect.center();
    ui.painter()
        .line_segment([c + Vec2::new(-4.5, -4.5), c + Vec2::new(4.5, 4.5)], stroke);
    ui.painter()
        .line_segment([c + Vec2::new(-4.5, 4.5), c + Vec2::new(4.5, -4.5)], stroke);
    response.clicked()
}

/// A back arrow button. Returns whether it was clicked.
pub(crate) fn back_button(ui: &mut egui::Ui) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let p = theme::pal();
    if response.hovered() {
        ui.painter()
            .circle_filled(rect.center(), 15.0, p.card_hover);
    }
    let stroke = Stroke::new(1.8, if response.hovered() { p.text } else { p.muted });
    let c = rect.center();
    ui.painter()
        .line_segment([c + Vec2::new(2.5, -5.0), c + Vec2::new(-2.5, 0.0)], stroke);
    ui.painter()
        .line_segment([c + Vec2::new(-2.5, 0.0), c + Vec2::new(2.5, 5.0)], stroke);
    response.clicked()
}

/// A right-pointing chevron drawn at the right edge of a list row.
pub(crate) fn row_chevron(painter: &egui::Painter, rect: Rect) {
    let c = Pos2::new(rect.right() - 14.0, rect.center().y);
    let stroke = Stroke::new(1.6, theme::pal().muted);
    painter.line_segment([c + Vec2::new(-2.0, -4.5), c + Vec2::new(2.5, 0.0)], stroke);
    painter.line_segment([c + Vec2::new(2.5, 0.0), c + Vec2::new(-2.0, 4.5)], stroke);
}

/// A small folder drawn at `center`.
pub(crate) fn folder_icon(painter: &egui::Painter, center: Pos2) {
    let body = Rect::from_center_size(center + Vec2::new(0.0, 1.5), Vec2::new(17.0, 12.0));
    let tab = Rect::from_min_size(body.min - Vec2::new(0.0, 3.0), Vec2::new(7.0, 4.0));
    painter.rect_filled(tab, CornerRadius::same(2), theme::YELLOW);
    painter.rect_filled(body, CornerRadius::same(3), theme::YELLOW);
}

/// Text centred in a rect (used for empty-state captions).
pub(crate) fn centered_text(ui: &egui::Ui, rect: Rect, text: &str, size: f32, color: Color32) {
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        theme::text(size),
        color,
    );
}

/// The rounded card used by every drop-down (the "..." menu, the sort menu).
pub(crate) fn popup_frame() -> egui::Frame {
    let p = theme::pal();
    egui::Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.divider))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(egui::Margin::same(6))
        .shadow(egui::Shadow {
            offset: [0, 6],
            blur: 22,
            spread: 0,
            color: Color32::from_black_alpha(if p.dark { 140 } else { 50 }),
        })
}

/// Fit `text` into `max_width` by cutting it and adding an ellipsis. `measure` gives the width of a
/// string. Returns the text to draw and whether it was cut.
pub(crate) fn fit(text: &str, max_width: f32, measure: impl Fn(&str) -> f32) -> (String, bool) {
    if measure(text) <= max_width {
        return (text.to_owned(), false);
    }
    let chars: Vec<char> = text.chars().collect();
    let with_ellipsis = |count: usize| {
        let mut cut: String = chars.iter().take(count).collect();
        cut.push('…');
        cut
    };
    // Longest prefix that still fits with the ellipsis (width grows with the prefix).
    let (mut low, mut high) = (0, chars.len());
    while low < high {
        let middle = (low + high).div_ceil(2);
        if measure(&with_ellipsis(middle)) <= max_width {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    (with_ellipsis(low), true)
}

/// `text` cut with an ellipsis so that it fits `max_width` when drawn with `font`.
pub(crate) fn fit_to_width(
    painter: &egui::Painter,
    text: &str,
    font: &egui::FontId,
    max_width: f32,
) -> String {
    fit(text, max_width, |candidate| {
        painter
            .layout_no_wrap(candidate.to_owned(), font.clone(), Color32::PLACEHOLDER)
            .size()
            .x
    })
    .0
}

/// A radio-style row for a drop-down: a dot, then the label. Everything is painted at fixed positions,
/// so nothing moves when the pointer is over it (egui's stock buttons grow slightly on hover, which
/// makes their text shift). Returns whether it was clicked.
pub(crate) fn choice_row(ui: &mut egui::Ui, label: &str, selected: bool) -> bool {
    let p = theme::pal();
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(9), p.card_hover);
    }
    let dot = Pos2::new(rect.left() + 18.0, rect.center().y);
    if selected {
        ui.painter().circle_filled(dot, 6.0, theme::YELLOW);
        ui.painter().circle_filled(dot, 2.4, p.on_yellow);
    } else {
        ui.painter().circle_stroke(
            dot,
            5.5,
            Stroke::new(1.4, if response.hovered() { p.text } else { p.muted }),
        );
    }
    ui.painter().text(
        Pos2::new(rect.left() + 38.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::text(12.5),
        p.text,
    );
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::test_support::text_with_and_without_hover;

    #[test]
    fn a_choice_row_does_not_move_its_text_when_hovered() {
        for selected in [false, true] {
            let (resting, hovered) = text_with_and_without_hover(|ui| {
                let origin = ui.cursor().min;
                choice_row(ui, "Progress", selected);
                let rect = Rect::from_min_size(origin, Vec2::new(ui.available_width(), 34.0));
                (rect, ui.rect_contains_pointer(rect))
            });
            assert_eq!(
                resting, hovered,
                "text position changed on hover (selected: {selected})"
            );
        }
    }

    #[test]
    fn text_is_cut_with_an_ellipsis_only_when_it_does_not_fit() {
        #[allow(clippy::cast_precision_loss)] // tiny test strings
        let measure = |text: &str| text.chars().count() as f32 * 10.0;
        assert_eq!(fit("short", 100.0, measure), ("short".to_owned(), false));
        assert_eq!(
            fit("exactly10c", 100.0, measure),
            ("exactly10c".to_owned(), false)
        );
        let (cut, was_cut) = fit("this is far too long", 100.0, measure);
        assert!(was_cut);
        assert_eq!(cut, "this is f…");
        assert!(measure(&cut) <= 100.0);
        assert_eq!(fit("anything", 5.0, measure), ("…".to_owned(), true));
        assert_eq!(fit("", 0.0, measure), (String::new(), false));
        // Multibyte characters are cut on character boundaries.
        let (cut, _) = fit("日本語日本語日本語日本語", 50.0, measure);
        assert_eq!(cut.chars().count(), 5);
    }
}
