//! The app icon in its light and dark form.
//!
//! The bundle (or installer) carries the light icon. While the app runs, the icon follows the theme it
//! is showing: the Dock icon on macOS (`putio_platform::app_icon`), the window icon elsewhere.

use std::sync::{Arc, OnceLock};

use eframe::egui::IconData;

const LIGHT_DOCK: &[u8] = include_bytes!("../../../assets/icon/app-light-512.png");
const DARK_DOCK: &[u8] = include_bytes!("../../../assets/icon/app-dark-512.png");
const LIGHT_WINDOW: &[u8] = include_bytes!("../../../assets/icon/app-light-256.png");
const DARK_WINDOW: &[u8] = include_bytes!("../../../assets/icon/app-dark-256.png");

/// The 512 px PNG used for the macOS Dock.
pub(crate) fn dock_png(dark: bool) -> &'static [u8] {
    if dark { DARK_DOCK } else { LIGHT_DOCK }
}

fn decode(png: &[u8]) -> Option<IconData> {
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    let (width, height) = image.dimensions();
    Some(IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

/// The window icon (Windows and Linux title bars and task bars), decoded once per theme.
pub(crate) fn window_icon(dark: bool) -> Option<Arc<IconData>> {
    static LIGHT: OnceLock<Option<Arc<IconData>>> = OnceLock::new();
    static DARK: OnceLock<Option<Arc<IconData>>> = OnceLock::new();
    let (cell, png) = if dark {
        (&DARK, DARK_WINDOW)
    } else {
        (&LIGHT, LIGHT_WINDOW)
    };
    cell.get_or_init(|| decode(png).map(Arc::new)).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_icons_are_valid_and_square() {
        for dark in [false, true] {
            let icon = window_icon(dark).expect("a valid window icon");
            assert_eq!((icon.width, icon.height), (256, 256));
            assert_eq!(icon.rgba.len(), 256 * 256 * 4);
            assert!(dock_png(dark).starts_with(b"\x89PNG"));
        }
    }

    #[test]
    fn the_two_variants_differ() {
        assert_ne!(dock_png(false), dock_png(true));
        let (light, dark) = (window_icon(false).unwrap(), window_icon(true).unwrap());
        // Compare the centre-left of the body, away from the magnet: light body vs dark body.
        let pixel = |icon: &IconData| {
            let i = (128 * 256 + 60) * 4;
            u32::from(icon.rgba[i]) + u32::from(icon.rgba[i + 1]) + u32::from(icon.rgba[i + 2])
        };
        assert!(
            pixel(&light) > pixel(&dark) + 300,
            "the light icon must have a much lighter body"
        );
    }
}
