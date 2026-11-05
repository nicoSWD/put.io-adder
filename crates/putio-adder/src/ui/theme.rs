//! put.io's colours: black, yellow and white.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, RichText, Stroke,
};

/// Window background.
pub(crate) const BG: Color32 = Color32::from_rgb(0x0F, 0x0F, 0x0F);
/// The logo row.
pub(crate) const HEADER: Color32 = Color32::BLACK;
/// The status row under it.
pub(crate) const SUBHEADER: Color32 = Color32::from_rgb(0x14, 0x14, 0x14);
pub(crate) const DIVIDER: Color32 = Color32::from_rgb(0x26, 0x26, 0x26);
/// Cards (transfer rows) and their hover / selected states.
pub(crate) const CARD: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x1A);
pub(crate) const CARD_HOVER: Color32 = Color32::from_rgb(0x23, 0x23, 0x23);
pub(crate) const CARD_SELECTED: Color32 = Color32::from_rgb(0x2A, 0x27, 0x1A);
/// Empty progress track.
pub(crate) const TRACK: Color32 = Color32::from_rgb(0x30, 0x30, 0x30);
/// The put.io yellow.
pub(crate) const YELLOW: Color32 = Color32::from_rgb(0xFD, 0xCE, 0x45);
pub(crate) const YELLOW_PRESSED: Color32 = Color32::from_rgb(0xD9, 0xAE, 0x2E);
pub(crate) const TEXT: Color32 = Color32::WHITE;
pub(crate) const MUTED: Color32 = Color32::from_rgb(0x9E, 0x9E, 0x9E);
pub(crate) const ERROR: Color32 = Color32::from_rgb(0xFF, 0x6B, 0x5E);

/// The put.io logo (white with a gold shadow on transparent: it needs a dark background).
pub(crate) const LOGO_PNG: &[u8] = include_bytes!("../../../../assets/brand/putio-logo.png");
/// Width / height of the logo image (564 x 144).
pub(crate) const LOGO_ASPECT: f32 = 564.0 / 144.0;

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

    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = Color32::from_rgb(0x1A, 0x1A, 0x1A);
    visuals.extreme_bg_color = Color32::from_rgb(0x0A, 0x0A, 0x0A);
    visuals.faint_bg_color = CARD;
    visuals.window_stroke = Stroke::new(1.0, DIVIDER);
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.override_text_color = Some(TEXT);
    visuals.selection.bg_fill = YELLOW;
    visuals.selection.stroke = Stroke::new(1.0, Color32::BLACK);
    visuals.hyperlink_color = YELLOW;
    visuals.window_shadow.color = Color32::from_black_alpha(160);
    visuals.popup_shadow.color = Color32::from_black_alpha(160);

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
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x26, 0x26, 0x26);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(0x26, 0x26, 0x26);
    visuals.widgets.inactive.bg_stroke = Stroke::NONE;
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x34, 0x34, 0x34);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x34, 0x34, 0x34);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, YELLOW);
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
