//! Helpers for testing how widgets draw, without a window.

use eframe::egui::{self, Pos2, Rect, Vec2};
use putio_core::settings::ThemeChoice;

use super::theme;

/// Where every piece of text in `shapes` is drawn.
fn text_positions(shapes: &[egui::epaint::ClippedShape]) -> Vec<Pos2> {
    fn collect(shape: &egui::Shape, out: &mut Vec<Pos2>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.pos),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in shapes {
        collect(&clipped.shape, &mut out);
    }
    out
}

/// Every piece of text drawn by one frame of `add`, in drawing order.
pub(crate) fn rendered_text(mut add: impl FnMut(&mut egui::Ui)) -> Vec<String> {
    fn collect(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.text().to_owned()),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
            _ => {}
        }
    }
    let ctx = egui::Context::default();
    theme::apply(&ctx, ThemeChoice::Light);
    let mut out = Vec::new();
    // Fonts and layout settle over the first frames; the last one is the one kept.
    for _ in 0..3 {
        out.clear();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 800.0))),
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| add(ui));
        for clipped in &output.shapes {
            collect(&clipped.shape, &mut out);
        }
        output.drop_without_applying_deltas();
    }
    out
}

/// A piece of text as drawn: what it says, and the part of it that can be seen.
#[derive(Debug, Clone)]
pub(crate) struct TextBox {
    pub(crate) text: String,
    /// The text's rectangle cut to the clip rectangle it was painted with: what the user sees.
    pub(crate) visible: Rect,
    /// The text's rectangle as laid out, before any clipping.
    pub(crate) full: Rect,
}

/// Every piece of text drawn by one frame of `add`, with where it is. Layout is what the tests of
/// "does this language fit" are made of: a text that sticks out of its container shows up here as a box
/// whose `visible` rectangle crosses the container's edge.
pub(crate) fn text_boxes(mut add: impl FnMut(&mut egui::Ui)) -> Vec<TextBox> {
    fn collect(shape: &egui::Shape, clip: Rect, out: &mut Vec<TextBox>) {
        match shape {
            egui::Shape::Text(text) => {
                let full = text.visual_bounding_rect();
                out.push(TextBox {
                    text: text.galley.text().to_owned(),
                    visible: full.intersect(clip),
                    full,
                });
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, clip, out)),
            _ => {}
        }
    }
    let ctx = egui::Context::default();
    theme::apply(&ctx, ThemeChoice::Light);
    let mut out = Vec::new();
    for _ in 0..3 {
        out.clear();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 1200.0))),
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| add(ui));
        for clipped in &output.shapes {
            collect(&clipped.shape, clipped.clip_rect, &mut out);
        }
        output.drop_without_applying_deltas();
    }
    out
}

/// What a screen reader is told about one frame of `add`: (role, name) of every node that has a name.
/// A control that is only painted and never described does not show up here.
pub(crate) fn accessible_names(mut add: impl FnMut(&mut egui::Ui)) -> Vec<(String, String)> {
    let ctx = egui::Context::default();
    theme::apply(&ctx, ThemeChoice::Light);
    ctx.enable_accesskit();
    let mut names = Vec::new();
    for _ in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 800.0))),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| add(ui));
        if let Some(update) = output.platform_output.accesskit_update.take() {
            names = update
                .nodes
                .iter()
                .filter_map(|(_, node)| {
                    let name = node.label().or_else(|| node.value())?;
                    Some((format!("{:?}", node.role()), name.to_owned()))
                })
                .collect();
        }
        output.drop_without_applying_deltas();
    }
    names
}

/// One frame of `add` (which returns the widget's rect and whether it is hovered), with the pointer
/// at `pointer` if given. Returns the rect, the hover state and the position of all text drawn.
fn frame(
    ctx: &egui::Context,
    pointer: Option<Pos2>,
    add: &mut dyn FnMut(&mut egui::Ui) -> (Rect, bool),
) -> (Rect, bool, Vec<Pos2>) {
    let mut input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 300.0))),
        ..Default::default()
    };
    if let Some(position) = pointer {
        input.events.push(egui::Event::PointerMoved(position));
    }
    let (mut rect, mut hovered) = (Rect::NOTHING, false);
    let output = ctx.run_ui(input, |ui| {
        (rect, hovered) = add(ui);
    });
    let positions = text_positions(&output.shapes);
    // The textures egui wants uploaded are irrelevant here (nothing is rendered).
    output.drop_without_applying_deltas();
    (rect, hovered, positions)
}

/// Draw a widget with and without the pointer over it and return the text positions of both.
///
/// Panics (failing the test) when the widget never reports being hovered, so a test cannot pass
/// merely because hovering did not happen.
pub(crate) fn text_with_and_without_hover(
    mut add: impl FnMut(&mut egui::Ui) -> (Rect, bool),
) -> (Vec<Pos2>, Vec<Pos2>) {
    let ctx = egui::Context::default();
    theme::apply(&ctx, ThemeChoice::Light);
    // Fonts and layout settle over the first frames.
    for _ in 0..3 {
        frame(&ctx, None, &mut add);
    }
    let (rect, hovered, resting) = frame(&ctx, None, &mut add);
    assert!(!hovered, "not hovered without a pointer");
    // egui decides hover from the previous frame's layout, so give it two frames with the pointer in place.
    let inside = rect.center();
    frame(&ctx, Some(inside), &mut add);
    let (_, hovered, over) = frame(&ctx, Some(inside), &mut add);
    assert!(
        hovered,
        "the widget must be hovered with the pointer over it"
    );
    assert!(!resting.is_empty(), "the widget draws text");
    (resting, over)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guards the guard: the harness must be able to *see* text move, or the "does not move on hover"
    /// tests prove nothing. This widget deliberately draws its text one pixel further right when hovered.
    #[test]
    fn the_harness_detects_text_that_moves_on_hover() {
        let (resting, hovered) = text_with_and_without_hover(|ui| {
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(120.0, 30.0), egui::Sense::hover());
            let nudge = if response.hovered() { 1.0 } else { 0.0 };
            ui.painter().text(
                rect.left_center() + Vec2::new(8.0 + nudge, 0.0),
                egui::Align2::LEFT_CENTER,
                "Progress",
                theme::text(12.0),
                egui::Color32::BLACK,
            );
            (rect, response.hovered())
        });
        assert_ne!(
            resting, hovered,
            "the harness failed to notice a 1 px shift"
        );
    }

    #[test]
    fn the_harness_sees_identical_positions_for_text_that_stays_put() {
        let (resting, hovered) = text_with_and_without_hover(|ui| {
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(120.0, 30.0), egui::Sense::hover());
            ui.painter().text(
                rect.left_center() + Vec2::new(8.0, 0.0),
                egui::Align2::LEFT_CENTER,
                "Progress",
                theme::text(12.0),
                egui::Color32::BLACK,
            );
            (rect, response.hovered())
        });
        assert_eq!(resting, hovered);
    }
}
