//! Uploading by dropping files on the window: what happens to a drop, the hint while files are over the
//! window, and the rows that show the uploads going (each with a bar and a button that stops it).
//!
//! A `.torrent` that is dropped is added like one that is opened with the app (it becomes a transfer);
//! any other file goes into the folder the Files tab shows. Folders and empty files are not uploaded.

use std::path::Path;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, Id, LayerId, Order, Pos2, Rect, RichText, Sense, Stroke,
    StrokeKind, Vec2,
};
use putio_core::i18n::{Locale, Msg};
use putio_core::intake::Intake;
use putio_core::state::{Dropped, DroppedKind, Event, OpId, Upload};

use super::{icons, theme, widgets};
use crate::app::App;

/// Height of one row of an upload.
const ROW_HEIGHT: f32 = 44.0;
const ROW_GAP: f32 = 4.0;
/// How many uploads get a row; the rest are counted.
const MAX_ROWS: usize = 3;
/// Space kept free at the right, as in the file list below.
const SCROLLBAR_ROOM: f32 = 8.0;
/// The button that stops an upload.
const CANCEL_SIZE: f32 = 24.0;

/// What a dropped path is, found out from the disk.
fn describe(path: &Path) -> Dropped {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (size, kind) = match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => (meta.len(), DroppedKind::File),
        _ => (0, DroppedKind::Other),
    };
    Dropped {
        path: path.to_path_buf(),
        name,
        size,
        kind,
    }
}

/// Debug builds only: the Files tab with uploads going (`files-uploading`) for a preview.
#[cfg(debug_assertions)]
pub(crate) fn demo(app: &mut App, ctx: &egui::Context, view: &str) {
    if view != "files-uploading" {
        return;
    }
    let file = |name: &str, size: u64| Dropped {
        path: format!("/tmp/{name}").into(),
        name: name.into(),
        size,
        kind: DroppedKind::File,
    };
    app.dispatch(
        ctx,
        Event::FilesDropped(vec![
            file("Big.Buck.Bunny.2008.2160p.BluRay.x265.REMUX.HDR10Plus.DV.TrueHD.7.1.Atmos-GROUP.mkv", 8_000_000_000),
            file("holiday-photos-2026.zip", 640_000_000),
            file("notes.txt", 4_000),
            file("scan.pdf", 2_500_000),
            file("music.flac", 38_000_000),
        ]),
    );
    let first = app.state.uploads.first().map(|u| (u.op, u.size));
    if let Some((op, size)) = first {
        app.dispatch(
            ctx,
            Event::UploadProgress {
                op,
                sent: size * 37 / 100,
            },
        );
    }
}

/// Act on files dropped on the window since the last frame.
pub(crate) fn handle_drops(ctx: &egui::Context, app: &mut App) {
    let dropped = ctx.input(|input| input.raw.dropped_files.clone());
    if dropped.is_empty() {
        return;
    }
    let mut files = Vec::new();
    for file in dropped {
        // Native platforms give an absolute path; never read the contents here (they can be huge).
        let path = file.path().to_path_buf();
        if !path.is_absolute() {
            continue;
        }
        // A torrent is added to put.io, wherever it is dropped; the intake knows what to do when the user
        // is signed out.
        if let Ok(intake @ Intake::TorrentFile(_)) = Intake::parse(&path.to_string_lossy()) {
            app.dispatch(ctx, Event::Intake(intake));
        } else {
            files.push(describe(&path));
        }
    }
    if files.is_empty() || !app.state.is_signed_in() {
        return;
    }
    // Show where they are going: the Files tab, with its list open.
    if !app.state.transfers_expanded {
        app.dispatch(ctx, Event::ToggleTransfers);
    }
    app.dispatch(ctx, Event::SelectTab(putio_core::settings::Tab::Files));
    app.dispatch(ctx, Event::FilesDropped(files));
}

/// While files are held over the window: say where they would go.
pub(crate) fn drop_hint(ctx: &egui::Context, app: &App) {
    if !app.state.is_signed_in() || ctx.input(|input| input.raw.hovered_files.is_empty()) {
        return;
    }
    let p = theme::pal();
    let locale = app.state.locale;
    let rect = ctx.content_rect();
    let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("upload-drop-hint")));
    painter.rect_filled(rect, 0.0, Color32::from_black_alpha(96));
    let frame = rect.shrink(12.0);
    painter.rect_filled(frame, CornerRadius::same(16), p.card.gamma_multiply(0.92));
    painter.rect_stroke(
        frame,
        CornerRadius::same(16),
        Stroke::new(2.0, theme::YELLOW),
        StrokeKind::Inside,
    );
    let trail = &app.state.files.trail;
    let folder = super::files::crumb_name(trail, trail.len().saturating_sub(1), locale);
    let font = theme::bold(15.0);
    let folder = widgets::fit_to_width(&painter, &folder, &font, (frame.width() - 120.0).max(60.0));
    painter.text(
        frame.center(),
        Align2::CENTER_CENTER,
        Msg::UploadDropHere { folder }.text(locale),
        font,
        p.text,
    );
}

/// The uploads going, as rows above the list. Does nothing when there are none. Returns the upload whose
/// cancel button was pressed.
pub(crate) fn strip(ui: &mut egui::Ui, uploads: &[Upload], locale: Locale) -> Option<OpId> {
    if uploads.is_empty() {
        return None;
    }
    let mut cancelled = None;
    for (index, upload) in uploads.iter().take(MAX_ROWS).enumerate() {
        if row(ui, upload, index == 0, locale) {
            cancelled = Some(upload.op);
        }
        ui.add_space(ROW_GAP);
    }
    if uploads.len() > MAX_ROWS {
        ui.add(
            egui::Label::new(
                RichText::new(
                    Msg::UploadsMore {
                        count: uploads.len() - MAX_ROWS,
                    }
                    .text(locale),
                )
                .font(theme::text(10.5))
                .color(theme::pal().muted),
            )
            .wrap(),
        );
        ui.add_space(ROW_GAP);
    }
    cancelled
}

/// One upload: its name, how far it has got (or that it waits for its turn), a bar and a stop button.
/// Everything is painted at fixed positions. Returns whether the stop button was pressed.
fn row(ui: &mut egui::Ui, upload: &Upload, first: bool, locale: Locale) -> bool {
    let p = theme::pal();
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width() - SCROLLBAR_ROOM, ROW_HEIGHT),
        Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, CornerRadius::same(12), p.inset);

    let cancel_rect = Rect::from_center_size(
        Pos2::new(rect.right() - 14.0 - CANCEL_SIZE / 2.0, rect.top() + 18.0),
        Vec2::splat(CANCEL_SIZE),
    );
    let state_right = cancel_rect.left() - 6.0;
    // The first is being sent; the others wait for it (they go one at a time).
    let state = if first || upload.sent > 0 {
        format!("{}%", upload.percent())
    } else {
        Msg::UploadWaiting.text(locale)
    };
    let state = ui
        .painter()
        .layout_no_wrap(state, theme::text(11.0), p.muted);
    let state_left = state_right - state.size().x;
    ui.painter().galley(
        Pos2::new(state_left, rect.top() + 18.0 - state.size().y / 2.0),
        state,
        p.muted,
    );
    // The name gets what the state leaves, so the two never meet.
    let name_room = (state_left - 12.0 - (rect.left() + 14.0)).max(40.0);
    let font = theme::bold(12.0);
    let name = widgets::fit_to_width(ui.painter(), &upload.name, &font, name_room);
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, rect.top() + 18.0),
        Align2::LEFT_CENTER,
        name,
        font,
        p.text,
    );
    let bar = Rect::from_min_max(
        Pos2::new(rect.left() + 14.0, rect.bottom() - 11.0),
        Pos2::new(rect.right() - 14.0, rect.bottom() - 7.0),
    );
    icons::progress_bar(
        ui.painter(),
        bar,
        f32::from(upload.percent()) / 100.0,
        theme::YELLOW,
    );

    let response = ui
        .interact(
            cancel_rect,
            Id::new(("upload-cancel", upload.op.0)),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    widgets::describe(
        &response,
        egui::WidgetType::Button,
        &Msg::A11yUploadCancel {
            name: upload.name.clone(),
        }
        .text(locale),
    );
    widgets::focus_ring(ui, &response, cancel_rect, CornerRadius::same(12));
    if response.hovered() {
        ui.painter()
            .rect_filled(cancel_rect, CornerRadius::same(12), p.card_hover);
    }
    let color = if response.hovered() { p.text } else { p.muted };
    let stroke = Stroke::new(1.6, color);
    let c = cancel_rect.center();
    ui.painter()
        .line_segment([c + Vec2::new(-3.5, -3.5), c + Vec2::new(3.5, 3.5)], stroke);
    ui.painter()
        .line_segment([c + Vec2::new(-3.5, 3.5), c + Vec2::new(3.5, -3.5)], stroke);
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::test_support::{accessible_names, text_boxes};

    fn upload(op: u64, name: &str, size: u64, sent: u64) -> Upload {
        Upload {
            op: OpId(op),
            name: name.into(),
            size,
            sent,
        }
    }

    #[test]
    fn a_dropped_path_is_described_from_the_disk() {
        let dir = std::env::temp_dir();
        let file = dir.join(format!("putio-adder-drop-{}.bin", std::process::id()));
        std::fs::write(&file, b"12345").unwrap();
        let found = describe(&file);
        assert_eq!((found.size, found.kind), (5, DroppedKind::File));
        assert!(found.name.starts_with("putio-adder-drop-"));
        let _ = std::fs::remove_file(&file);
        // A folder, and a path that is not there, are not files.
        assert_eq!(describe(&dir).kind, DroppedKind::Other);
        assert_eq!(
            describe(&dir.join("putio-adder-no-such-file")).kind,
            DroppedKind::Other
        );
    }

    #[test]
    fn the_first_upload_shows_its_percentage_and_the_others_wait() {
        let list = [upload(1, "a.mkv", 200, 100), upload(2, "b.zip", 50, 0)];
        let boxes = text_boxes(|ui| {
            strip(ui, &list, Locale::En);
        });
        let texts: Vec<&str> = boxes.iter().map(|b| b.text.as_str()).collect();
        for wanted in ["a.mkv", "50%", "b.zip", "Waiting"] {
            assert!(texts.contains(&wanted), "missing {wanted:?} in {texts:?}");
        }
    }

    #[test]
    fn only_a_few_rows_are_drawn_and_the_rest_are_counted() {
        let list: Vec<Upload> = (0..MAX_ROWS as u64 + 4)
            .map(|i| upload(i, &format!("f{i}"), 10, 0))
            .collect();
        let boxes = text_boxes(|ui| {
            strip(ui, &list, Locale::En);
        });
        let texts: Vec<&str> = boxes.iter().map(|b| b.text.as_str()).collect();
        assert!(texts.contains(&"and 4 more"), "{texts:?}");
        assert!(texts.contains(&"f2") && !texts.contains(&"f3"), "{texts:?}");
        assert!(
            text_boxes(|ui| {
                assert_eq!(strip(ui, &[], Locale::En), None);
            })
            .is_empty()
        );
    }

    #[test]
    fn every_row_can_be_stopped_by_a_button_named_for_its_file() {
        let list = [upload(1, "a.mkv", 200, 100)];
        let names = accessible_names(|ui| {
            strip(ui, &list, Locale::En);
        });
        assert!(
            names
                .iter()
                .any(|(role, name)| role == "Button" && name == "Cancel upload of a.mkv"),
            "{names:?}"
        );
    }

    #[test]
    fn a_long_name_is_cut_before_the_percentage_in_every_language() {
        let long = "An.Extremely.Long.File.Name.".repeat(8);
        let list = [upload(1, &long, 200, 100), upload(2, &long, 50, 0)];
        for locale in [Locale::En, Locale::De, Locale::Es] {
            let mut used = Rect::NOTHING;
            let boxes = text_boxes(|ui| {
                ui.allocate_ui(Vec2::new(512.0, 300.0), |ui| {
                    strip(ui, &list, locale);
                    used = ui.min_rect();
                });
            });
            assert!(used.width() <= 512.0 + 0.5, "{locale:?}: {used:?}");
            let state: Vec<_> = boxes
                .iter()
                .filter(|b| b.text.ends_with('%') || b.text == Msg::UploadWaiting.text(locale))
                .collect();
            assert_eq!(state.len(), 2, "{locale:?}: {boxes:?}");
            for name in boxes.iter().filter(|b| b.text.starts_with("An.Extremely")) {
                assert!(name.text.ends_with('…'), "{locale:?}: {:?}", name.text);
                for s in &state {
                    // The name and the state of the same row do not overlap.
                    let same_row = (name.full.center().y - s.full.center().y).abs() < 2.0;
                    assert!(
                        !same_row || name.full.right() <= s.full.left(),
                        "{locale:?}: {name:?} vs {s:?}"
                    );
                }
                assert!(name.full.right() <= used.right(), "{locale:?}: {name:?}");
            }
        }
    }
}
