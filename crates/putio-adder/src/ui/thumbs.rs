//! The small pictures of files: which have been asked for, which have arrived, and the textures they became.
//!
//! A row asks for its picture only when it is on screen, so a folder of a thousand videos does not ask for a
//! thousand pictures; at most a dozen are on their way at a time, and one that fails is not asked for again.
//! A row without a picture keeps the icon of its type, so nothing moves when one arrives.

use std::collections::HashMap;

use eframe::egui::{
    self, Color32, ColorImage, CornerRadius, Rect, TextureHandle, TextureOptions, Vec2,
};

use crate::worker::Thumbnail;

/// How many pictures may be on their way at once.
const MAX_IN_FLIGHT: usize = 12;
/// How many textures are kept; beyond this the ones that are not wanted any more go (and are asked for again
/// if their row comes back).
const MAX_KEPT: usize = 400;

enum Entry {
    Pending,
    Failed,
    Ready(TextureHandle),
}

impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Pending => "Pending",
            Self::Failed => "Failed",
            Self::Ready(_) => "Ready",
        })
    }
}

#[derive(Debug, Default)]
pub(crate) struct Thumbs {
    entries: HashMap<String, Entry>,
    in_flight: usize,
}

impl Thumbs {
    /// The picture at `url`, when it has arrived.
    pub(crate) fn texture(&self, url: &str) -> Option<&TextureHandle> {
        match self.entries.get(url) {
            Some(Entry::Ready(texture)) => Some(texture),
            _ => None,
        }
    }

    /// Whether the picture should be asked for now. When it should, it is marked as on its way.
    pub(crate) fn wanted(&mut self, url: &str) -> bool {
        if self.entries.contains_key(url) || self.in_flight >= MAX_IN_FLIGHT {
            return false;
        }
        self.entries.insert(url.to_owned(), Entry::Pending);
        self.in_flight += 1;
        true
    }

    /// A picture came (or could not be had).
    pub(crate) fn arrived(
        &mut self,
        ctx: &egui::Context,
        url: String,
        result: Result<Thumbnail, ()>,
    ) {
        if !matches!(self.entries.get(&url), Some(Entry::Pending)) {
            return; // not asked for (any more): nothing to do with it
        }
        self.in_flight = self.in_flight.saturating_sub(1);
        let entry = match result {
            Ok(picture) => Entry::Ready(ctx.load_texture(
                format!("thumbnail:{url}"),
                ColorImage::from_rgba_unmultiplied(picture.size, &picture.rgba),
                TextureOptions::LINEAR,
            )),
            Err(()) => Entry::Failed,
        };
        self.entries.insert(url, entry);
        if self.entries.len() > MAX_KEPT {
            self.entries
                .retain(|_, entry| !matches!(entry, Entry::Ready(_)));
        }
        ctx.request_repaint();
    }

    /// Forget everything (signing out): what was on its way is answered to nobody.
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.in_flight = 0;
    }

    /// Debug builds only: a picture that is already there, for previews.
    #[cfg(debug_assertions)]
    pub(crate) fn insert_ready(&mut self, ctx: &egui::Context, url: &str, picture: &Thumbnail) {
        let texture = ctx.load_texture(
            format!("thumbnail:{url}"),
            ColorImage::from_rgba_unmultiplied(picture.size, &picture.rgba),
            TextureOptions::LINEAR,
        );
        self.entries.insert(url.to_owned(), Entry::Ready(texture));
    }
}

/// The part of a picture that fills a square without being stretched: the middle of its longer side.
pub(crate) fn cover_uv(size: Vec2) -> Rect {
    let (w, h) = (size.x.max(1.0), size.y.max(1.0));
    let (u, v) = if w > h {
        let shown = h / w;
        ((1.0 - shown) / 2.0, 0.0)
    } else {
        let shown = w / h;
        (0.0, (1.0 - shown) / 2.0)
    };
    Rect::from_min_max(egui::pos2(u, v), egui::pos2(1.0 - u, 1.0 - v))
}

/// Paint `texture` into `rect` with rounded corners, filling it (cropped, not stretched).
pub(crate) fn paint(painter: &egui::Painter, rect: Rect, texture: &TextureHandle, radius: u8) {
    painter.add(
        egui::epaint::RectShape::filled(rect, CornerRadius::same(radius), Color32::WHITE)
            .with_texture(texture.id(), cover_uv(texture.size_vec2())),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_fills_the_square_by_cropping_its_longer_side_and_never_stretches() {
        // Landscape: the middle of the width.
        let uv = cover_uv(Vec2::new(160.0, 90.0));
        assert!((uv.height() - 1.0).abs() < 1e-6);
        assert!((uv.width() - 90.0 / 160.0).abs() < 1e-6);
        assert!((uv.center().x - 0.5).abs() < 1e-6);
        // Portrait: the middle of the height.
        let uv = cover_uv(Vec2::new(90.0, 160.0));
        assert!((uv.width() - 1.0).abs() < 1e-6);
        assert!((uv.height() - 90.0 / 160.0).abs() < 1e-6);
        // Square, and a degenerate size, show it all.
        assert_eq!(
            cover_uv(Vec2::new(50.0, 50.0)),
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))
        );
        let _ = cover_uv(Vec2::ZERO);
    }

    #[test]
    fn a_picture_is_asked_for_once_a_dozen_at_a_time_and_a_failure_is_not_asked_again() {
        let ctx = egui::Context::default();
        let mut thumbs = Thumbs::default();
        for i in 0..MAX_IN_FLIGHT {
            assert!(thumbs.wanted(&format!("u{i}")), "{i}");
        }
        // The dozen are on their way: a thirteenth waits, and one that is on its way is not asked for twice.
        assert!(!thumbs.wanted("u-extra"));
        assert!(!thumbs.wanted("u0"));
        thumbs.arrived(&ctx, "u0".into(), Err(()));
        assert!(thumbs.wanted("u-extra"), "room again");
        assert!(!thumbs.wanted("u0"), "a failed picture is not retried");
        let picture = Thumbnail {
            size: [2, 2],
            rgba: vec![255; 16],
        };
        thumbs.arrived(&ctx, "u1".into(), Ok(picture.clone()));
        assert!(thumbs.texture("u1").is_some() && thumbs.texture("u0").is_none());
        // An answer nobody asked for is ignored, and signing out forgets everything.
        thumbs.arrived(&ctx, "stranger".into(), Ok(picture));
        assert!(thumbs.texture("stranger").is_none());
        thumbs.clear();
        assert!(thumbs.wanted("u0"));
    }
}
