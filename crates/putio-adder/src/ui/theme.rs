//! put.io's colours: black, yellow and white, in a light and a dark palette.
//!
//! Yellow is a *fill* colour (progress, badges, buttons, the avatar). On the dark palette it also works
//! as text; on the light one it does not, so [`Palette::accent_text`] is yellow on dark and black on
//! light. Code asks the current palette for colours with [`pal`]; the palette is chosen once per frame
//! by [`select`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, RichText, Stroke,
};
use putio_core::settings::ThemeChoice;

/// The put.io yellow: the same in both palettes.
pub(crate) const YELLOW: Color32 = Color32::from_rgb(0xFD, 0xCE, 0x45);

#[derive(Debug)]
pub(crate) struct Palette {
    pub(crate) dark: bool,
    /// Window background.
    pub(crate) bg: Color32,
    /// The logo row.
    pub(crate) header: Color32,
    /// The status row under it.
    pub(crate) subheader: Color32,
    pub(crate) divider: Color32,
    /// Cards (transfer rows) and their hover / selected states.
    pub(crate) card: Color32,
    pub(crate) card_hover: Color32,
    pub(crate) card_selected: Color32,
    /// Empty progress track.
    pub(crate) track: Color32,
    /// Background of dialogs (Preferences), and the inset groups inside them.
    pub(crate) surface: Color32,
    pub(crate) inset: Color32,
    pub(crate) yellow_pressed: Color32,
    /// Soft tints behind icons.
    pub(crate) yellow_soft: Color32,
    pub(crate) neutral_soft: Color32,
    pub(crate) error_soft: Color32,
    pub(crate) text: Color32,
    pub(crate) muted: Color32,
    pub(crate) error: Color32,
    /// Text that must stand out as "the accent": yellow on dark, black on light.
    pub(crate) accent_text: Color32,
    /// Text on top of a yellow fill (always black).
    pub(crate) on_yellow: Color32,
    /// Outline of the quota pill when there is no quota, and the ring around the empty-state badge.
    pub(crate) outline: Color32,
    pub(crate) badge_border: Color32,
    /// The logo image for this background.
    pub(crate) logo: &'static [u8],
    pub(crate) logo_uri: &'static str,
}

const LIGHT: Palette = Palette {
    dark: false,
    bg: Color32::WHITE,
    header: Color32::WHITE,
    subheader: Color32::from_rgb(0xF6, 0xF6, 0xF4),
    divider: Color32::from_rgb(0xE6, 0xE6, 0xE2),
    card: Color32::from_rgb(0xF6, 0xF6, 0xF4),
    card_hover: Color32::from_rgb(0xEC, 0xEC, 0xE8),
    card_selected: Color32::from_rgb(0xFF, 0xF3, 0xC8),
    track: Color32::from_rgb(0xDF, 0xDF, 0xDA),
    surface: Color32::WHITE,
    inset: Color32::from_rgb(0xF6, 0xF6, 0xF4),
    yellow_pressed: Color32::from_rgb(0xE8, 0xB5, 0x1F),
    yellow_soft: Color32::from_rgb(0xFF, 0xEE, 0xB0),
    neutral_soft: Color32::from_rgb(0xE6, 0xE6, 0xE2),
    error_soft: Color32::from_rgb(0xFC, 0xE3, 0xDF),
    text: Color32::from_rgb(0x11, 0x11, 0x11),
    muted: Color32::from_rgb(0x6B, 0x6B, 0x66),
    error: Color32::from_rgb(0xD0, 0x30, 0x25),
    accent_text: Color32::from_rgb(0x11, 0x11, 0x11),
    on_yellow: Color32::BLACK,
    outline: Color32::from_rgb(0xE8, 0xB5, 0x1F),
    badge_border: YELLOW,
    logo: include_bytes!("../../../../assets/brand/putio-logo-light.png"),
    logo_uri: "bytes://putio-logo-light.png",
};

const DARK: Palette = Palette {
    dark: true,
    // Dark grey rather than near-black, with clear steps between the window, the status row and the cards.
    bg: Color32::from_rgb(0x1E, 0x1E, 0x21),
    header: Color32::from_rgb(0x12, 0x12, 0x14),
    subheader: Color32::from_rgb(0x28, 0x28, 0x2C),
    divider: Color32::from_rgb(0x44, 0x44, 0x4A),
    card: Color32::from_rgb(0x2B, 0x2B, 0x30),
    card_hover: Color32::from_rgb(0x37, 0x37, 0x3D),
    card_selected: Color32::from_rgb(0x41, 0x3A, 0x22),
    track: Color32::from_rgb(0x52, 0x52, 0x59),
    surface: Color32::from_rgb(0x2A, 0x2A, 0x2F),
    inset: Color32::from_rgb(0x36, 0x36, 0x3C),
    yellow_pressed: Color32::from_rgb(0xD9, 0xAE, 0x2E),
    yellow_soft: Color32::from_rgb(0x4A, 0x3F, 0x16),
    neutral_soft: Color32::from_rgb(0x40, 0x40, 0x46),
    error_soft: Color32::from_rgb(0x55, 0x2A, 0x26),
    text: Color32::WHITE,
    muted: Color32::from_rgb(0xBC, 0xBC, 0xC2),
    error: Color32::from_rgb(0xFF, 0x7A, 0x6C),
    accent_text: YELLOW,
    on_yellow: Color32::BLACK,
    outline: YELLOW,
    badge_border: Color32::from_rgb(0x8A, 0x71, 0x24),
    logo: include_bytes!("../../../../assets/brand/putio-logo-dark.png"),
    logo_uri: "bytes://putio-logo-dark.png",
};

/// Width / height of the logo images (2170 x 725 source, scaled to 900 x 301).
pub(crate) const LOGO_ASPECT: f32 = 2170.0 / 725.0;

static DARK_MODE: AtomicBool = AtomicBool::new(false);

/// The palette in use. Cheap: call it wherever a colour is needed.
pub(crate) fn pal() -> &'static Palette {
    if DARK_MODE.load(Ordering::Relaxed) {
        &DARK
    } else {
        &LIGHT
    }
}

/// Choose the palette for this frame from what egui resolved the preference to.
pub(crate) fn select(ctx: &egui::Context) {
    DARK_MODE.store(ctx.theme() == egui::Theme::Dark, Ordering::Relaxed);
}

/// Apply the user's choice: egui follows the system unless told otherwise.
pub(crate) fn preference(choice: ThemeChoice) -> egui::ThemePreference {
    match choice {
        ThemeChoice::System => egui::ThemePreference::System,
        ThemeChoice::Light => egui::ThemePreference::Light,
        ThemeChoice::Dark => egui::ThemePreference::Dark,
    }
}

/// The native window (title bar) theme for a choice.
pub(crate) fn window_theme(choice: ThemeChoice) -> egui::SystemTheme {
    match choice {
        ThemeChoice::System => egui::SystemTheme::SystemDefault,
        ThemeChoice::Light => egui::SystemTheme::Light,
        ThemeChoice::Dark => egui::SystemTheme::Dark,
    }
}

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

/// egui visuals (widgets, windows, menus) for one palette.
fn visuals(p: &Palette) -> egui::Visuals {
    let mut visuals = if p.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.panel_fill = p.bg;
    visuals.window_fill = p.surface;
    visuals.extreme_bg_color = if p.dark {
        Color32::from_rgb(0x16, 0x16, 0x18)
    } else {
        Color32::from_rgb(0xF1, 0xF1, 0xEE)
    };
    visuals.faint_bg_color = p.card;
    visuals.window_stroke = Stroke::new(1.0, p.divider);
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.override_text_color = Some(p.text);
    visuals.selection.bg_fill = YELLOW;
    visuals.selection.stroke = Stroke::new(1.0, p.on_yellow);
    visuals.hyperlink_color = p.accent_text; // links are never blue
    let shadow = Color32::from_black_alpha(if p.dark { 160 } else { 46 });
    visuals.window_shadow.color = shadow;
    visuals.popup_shadow.color = shadow;

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
    let (rest, hover) = if p.dark {
        (
            Color32::from_rgb(0x3A, 0x3A, 0x40),
            Color32::from_rgb(0x4A, 0x4A, 0x51),
        )
    } else {
        (
            Color32::from_rgb(0xEE, 0xEE, 0xEA),
            Color32::from_rgb(0xE2, 0xE2, 0xDD),
        )
    };
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.divider);
    visuals.widgets.inactive.weak_bg_fill = rest;
    visuals.widgets.inactive.bg_fill = rest;
    visuals.widgets.inactive.bg_stroke = Stroke::NONE;
    visuals.widgets.hovered.weak_bg_fill = hover;
    visuals.widgets.hovered.bg_fill = hover;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, p.outline);
    visuals.widgets.active.weak_bg_fill = p.yellow_pressed;
    visuals.widgets.active.bg_fill = p.yellow_pressed;
    visuals.widgets.active.bg_stroke = Stroke::NONE;
    visuals
}

pub(crate) fn apply(ctx: &egui::Context, choice: ThemeChoice) {
    egui_extras::install_image_loaders(ctx);
    install_fonts(ctx);
    ctx.set_visuals_of(egui::Theme::Light, visuals(&LIGHT));
    ctx.set_visuals_of(egui::Theme::Dark, visuals(&DARK));
    ctx.set_theme(preference(choice));
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.interaction.tooltip_delay = 0.4;
    });
    select(ctx);
}

/// A solid yellow button with black text: the one action that matters on a screen.
pub(crate) fn primary_button(ui: &mut egui::Ui, label: impl Into<String>) -> egui::Response {
    let text = RichText::new(label.into())
        .font(bold(12.0))
        .color(pal().on_yellow);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The contrast the design relies on: text must stand out from its background in both palettes.
    fn luminance(c: Color32) -> f32 {
        let channel = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    #[test]
    fn text_is_readable_on_every_surface_in_both_palettes() {
        for p in [&LIGHT, &DARK] {
            for surface in [
                p.bg,
                p.header,
                p.subheader,
                p.card,
                p.card_hover,
                p.card_selected,
                p.surface,
                p.inset,
            ] {
                assert!(
                    contrast(p.text, surface) >= 7.0,
                    "text on {surface:?} (dark: {})",
                    p.dark
                );
                assert!(
                    contrast(p.muted, surface) >= 4.5,
                    "muted on {surface:?} (dark: {})",
                    p.dark
                );
                assert!(
                    contrast(p.accent_text, surface) >= 7.0,
                    "accent text on {surface:?} (dark: {})",
                    p.dark
                );
            }
            assert!(contrast(p.on_yellow, YELLOW) >= 7.0, "black on yellow");
            assert!(
                contrast(p.error, p.card) >= 3.0,
                "error text on a card (dark: {})",
                p.dark
            );
        }
    }

    #[test]
    fn the_light_palette_never_uses_yellow_as_text() {
        assert_ne!(LIGHT.accent_text, YELLOW);
        assert_eq!(DARK.accent_text, YELLOW);
    }

    #[test]
    fn the_logo_images_are_embedded_and_distinct() {
        assert!(LIGHT.logo.starts_with(b"\x89PNG") && DARK.logo.starts_with(b"\x89PNG"));
        assert_ne!(LIGHT.logo, DARK.logo);
        assert_ne!(LIGHT.logo_uri, DARK.logo_uri);
    }

    #[test]
    fn choices_map_to_egui_preferences() {
        assert_eq!(
            preference(ThemeChoice::System),
            egui::ThemePreference::System
        );
        assert_eq!(preference(ThemeChoice::Dark), egui::ThemePreference::Dark);
        assert_eq!(window_theme(ThemeChoice::Light), egui::SystemTheme::Light);
    }
}
