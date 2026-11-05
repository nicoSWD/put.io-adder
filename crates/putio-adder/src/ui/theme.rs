//! Colours and styling: black header strip, put.io gold, light body.

use std::sync::Arc;

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId};

pub(crate) const BODY: Color32 = Color32::from_rgb(0xFE, 0xFE, 0xFE);
pub(crate) const STRIP: Color32 = Color32::BLACK;
/// The row with the logo: a shade lighter than the strip below it.
pub(crate) const TOP: Color32 = Color32::from_rgb(0x12, 0x12, 0x12);
pub(crate) const GOLD_BACK: Color32 = Color32::from_rgb(0xDD, 0xA3, 0x2F);
pub(crate) const GOLD_FILL: Color32 = Color32::from_rgb(0xFF, 0xBF, 0x36);
pub(crate) const INK: Color32 = Color32::from_rgb(0x14, 0x14, 0x14);
pub(crate) const MUTED: Color32 = Color32::from_rgb(0x55, 0x55, 0x55);
pub(crate) const ERROR: Color32 = Color32::from_rgb(0xB0, 0x2A, 0x2A);

/// The put.io logo (white with a gold shadow on transparent: it needs a dark background).
pub(crate) const LOGO_PNG: &[u8] = include_bytes!("../../../../assets/brand/putio-logo.png");
/// Width / height of the logo image (564 x 144).
pub(crate) const LOGO_ASPECT: f32 = 564.0 / 144.0;

const MONTSERRAT_REGULAR: &[u8] = include_bytes!("../../../../assets/fonts/Montserrat-Regular.ttf");
const MONTSERRAT_BOLD: &[u8] = include_bytes!("../../../../assets/fonts/Montserrat-Bold.ttf");

/// Montserrat Bold at `size` (the font of the quota bar and the account name).
pub(crate) fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
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
