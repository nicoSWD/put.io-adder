//! Colours and styling: black header strip, put.io gold, light body.

use eframe::egui::{self, Color32};

pub(crate) const BODY: Color32 = Color32::from_rgb(0xFE, 0xFE, 0xFE);
pub(crate) const STRIP: Color32 = Color32::BLACK;
pub(crate) const GOLD_BACK: Color32 = Color32::from_rgb(0xDD, 0xA3, 0x2F);
pub(crate) const GOLD_FILL: Color32 = Color32::from_rgb(0xFF, 0xBF, 0x36);
pub(crate) const INK: Color32 = Color32::from_rgb(0x14, 0x14, 0x14);
pub(crate) const MUTED: Color32 = Color32::from_rgb(0x55, 0x55, 0x55);
pub(crate) const ERROR: Color32 = Color32::from_rgb(0xB0, 0x2A, 0x2A);

pub(crate) fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = BODY;
    visuals.window_fill = BODY;
    visuals.override_text_color = Some(INK);
    visuals.selection.bg_fill = GOLD_FILL;
    visuals.selection.stroke.color = INK;
    visuals.hyperlink_color = MUTED; // links are never blue
    visuals.widgets.hovered.bg_fill = GOLD_FILL;
    visuals.widgets.active.bg_fill = GOLD_BACK;
    ctx.set_visuals(visuals);
}
