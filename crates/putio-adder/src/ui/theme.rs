//! put.io's colours on white: black text, yellow accents.
//!
//! Yellow is for *fills* (progress, badges, buttons, the avatar): yellow text is unreadable on white.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, RichText, Stroke,
};

/// Window background.
pub(crate) const BG: Color32 = Color32::WHITE;
/// The logo row.
pub(crate) const HEADER: Color32 = Color32::WHITE;
/// The status row under it.
pub(crate) const SUBHEADER: Color32 = Color32::from_rgb(0xF6, 0xF6, 0xF4);
pub(crate) const DIVIDER: Color32 = Color32::from_rgb(0xE6, 0xE6, 0xE2);
/// Cards (transfer rows) and their hover / selected states.
pub(crate) const CARD: Color32 = Color32::from_rgb(0xF6, 0xF6, 0xF4);
pub(crate) const CARD_HOVER: Color32 = Color32::from_rgb(0xEC, 0xEC, 0xE8);
pub(crate) const CARD_SELECTED: Color32 = Color32::from_rgb(0xFF, 0xF3, 0xC8);
/// Empty progress track.
pub(crate) const TRACK: Color32 = Color32::from_rgb(0xDF, 0xDF, 0xDA);
/// The put.io yellow, and soft tints of the colours used behind icons.
pub(crate) const YELLOW: Color32 = Color32::from_rgb(0xFD, 0xCE, 0x45);
pub(crate) const YELLOW_PRESSED: Color32 = Color32::from_rgb(0xE8, 0xB5, 0x1F);
pub(crate) const YELLOW_SOFT: Color32 = Color32::from_rgb(0xFF, 0xEE, 0xB0);
pub(crate) const NEUTRAL_SOFT: Color32 = Color32::from_rgb(0xE6, 0xE6, 0xE2);
pub(crate) const ERROR_SOFT: Color32 = Color32::from_rgb(0xFC, 0xE3, 0xDF);
pub(crate) const TEXT: Color32 = Color32::from_rgb(0x11, 0x11, 0x11);
pub(crate) const MUTED: Color32 = Color32::from_rgb(0x6B, 0x6B, 0x66);
pub(crate) const ERROR: Color32 = Color32::from_rgb(0xD0, 0x30, 0x25);

/// The put.io logo (pixel-art magnet, transparent background; works on white).
pub(crate) const LOGO_PNG: &[u8] = include_bytes!("../../../../assets/brand/putio-logo.png");
/// Width / height of the logo image (2170 x 725 source, scaled to 900 x 301).
pub(crate) const LOGO_ASPECT: f32 = 2170.0 / 725.0;

const MONTSERRAT_REGULAR: &[u8] = include_bytes!("../../../../assets/fonts/Montserrat-Regular.ttf");
const MONTSERRAT_BOLD: &[u8] = include_bytes!("../../../../assets/fonts/Montserrat-Bold.ttf");

/// Montserrat Bold at `size`.
pub(crate) fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

pub(crate) fn text(size: f32) -> FontId {
    FontId::proportional(size)
}

/// Montserrat first; egui's built-in fonts stay behind it for symbols and anything Montserrat lacks.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "Montserrat".into(),
        Arc::new(FontData::from_static(MONTSERRAT_REGULAR)),
    );
    fonts.font_data.insert(
        "Montserrat-Bold".into(),
        Arc::new(FontData::from_static(MONTSERRAT_BOLD)),
    );
    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "Montserrat".into());
    let mut bold_chain = vec!["Montserrat-Bold".to_owned()];
    bold_chain.extend(fallback);
    fonts
        .families
        .insert(FontFamily::Name("bold".into()), bold_chain);
    ctx.set_fonts(fonts);
}

pub(crate) fn apply(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
    install_fonts(ctx);

    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = BG;
    visuals.window_fill = Color32::WHITE;
    visuals.extreme_bg_color = Color32::from_rgb(0xF1, 0xF1, 0xEE);
    visuals.faint_bg_color = CARD;
    visuals.window_stroke = Stroke::new(1.0, DIVIDER);
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.override_text_color = Some(TEXT);
    visuals.selection.bg_fill = YELLOW;
    visuals.selection.stroke = Stroke::new(1.0, TEXT);
    visuals.hyperlink_color = TEXT; // links are never blue
    visuals.window_shadow.color = Color32::from_black_alpha(46);
    visuals.popup_shadow.color = Color32::from_black_alpha(46);

    let radius = CornerRadius::same(8);
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = radius;
    }
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, DIVIDER);
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(0xEE, 0xEE, 0xEA);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(0xEE, 0xEE, 0xEA);
    visuals.widgets.inactive.bg_stroke = Stroke::NONE;
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0xE2, 0xE2, 0xDD);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(0xE2, 0xE2, 0xDD);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, YELLOW_PRESSED);
    visuals.widgets.active.weak_bg_fill = YELLOW_PRESSED;
    visuals.widgets.active.bg_fill = YELLOW_PRESSED;
    visuals.widgets.active.bg_stroke = Stroke::NONE;
    ctx.set_visuals(visuals);

    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.interaction.tooltip_delay = 0.4;
    });
}

/// A solid yellow button with black text: the one action that matters on a screen.
pub(crate) fn primary_button(ui: &mut egui::Ui, label: impl Into<String>) -> egui::Response {
    let text = RichText::new(label.into())
        .font(bold(12.0))
        .color(Color32::BLACK);
    ui.add(
        egui::Button::new(text)
            .fill(YELLOW)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(8))
            .min_size(egui::vec2(0.0, 30.0)),
    )
}

/// A quiet button for secondary actions.
pub(crate) fn secondary_button(ui: &mut egui::Ui, label: impl Into<String>) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label.into()).font(text(12.0)))
            .min_size(egui::vec2(0.0, 30.0)),
    )
}
