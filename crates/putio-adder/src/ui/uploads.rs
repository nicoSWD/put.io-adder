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
    // One that already failed, to show where the reason stays.
    let failed = app.state.uploads.get(1).map(|u| u.op);
    if let Some(op) = failed {
        app.dispatch(
            ctx,
            Event::UploadFinished {
                op,
                result: Err(putio_core::state::UploadFailure::Local(
                    "put.io says the file is too large for one upload.".into(),
                )),
            },
        );
    }
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
    // Native platforms give an absolute path; the contents are never read here (they can be huge).
    let paths = dropped
        .iter()
        .map(|file| file.path().to_path_buf())
        .filter(|path| path.is_absolute())
        .collect();
    add_paths(ctx, app, paths);
}

/// Ask for files with the system's dialog and upload what is chosen.
pub(crate) fn choose_files(ctx: &egui::Context, app: &mut App) {
    let title = Msg::UploadPickTitle.text(app.state.locale);
    add_paths(ctx, app, putio_platform::file_dialog::pick_files(&title));
}

/// Upload these files (dropped, or chosen in the dialog) into the folder the Files tab shows. A `.torrent`
/// is added to put.io as a transfer, wherever it came from; the intake knows what to do when the user is
/// signed out.
fn add_paths(ctx: &egui::Context, app: &mut App, paths: Vec<std::path::PathBuf>) {
    let mut files = Vec::new();
    for path in paths {
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

/// What a button in the rows asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Act {
    /// Stop an upload that is going or waiting.
    Cancel(OpId),
    /// Put away the row of an upload that failed.
    Dismiss(OpId),
}

/// The uploads going, and the ones that failed, as rows above the list. Does nothing when there are none.
pub(crate) fn strip(ui: &mut egui::Ui, uploads: &[Upload], locale: Locale) -> Option<Act> {
    if uploads.is_empty() {
        return None;
    }
    let mut chosen = None;
    // The first that has not failed is the one being sent; the others wait for it.
    let sending = uploads.iter().position(|upload| upload.failed.is_none());
    for (index, upload) in uploads.iter().take(MAX_ROWS).enumerate() {
        if row(ui, upload, sending == Some(index), locale) {
            chosen = Some(if upload.failed.is_some() {
                Act::Dismiss(upload.op)
            } else {
                Act::Cancel(upload.op)
            });
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
    chosen
}

/// One upload: its name, how far it has got (or that it waits for its turn, or why it failed), a bar and a
/// button that stops it (or puts the row away). Everything is painted at fixed positions. Returns whether
/// the button was pressed.
fn row(ui: &mut egui::Ui, upload: &Upload, first: bool, locale: Locale) -> bool {
    let p = theme::pal();
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width() - SCROLLBAR_ROOM, ROW_HEIGHT),
        Sense::hover(),
    );
    let fill = if upload.failed.is_some() {
        p.error_soft
    } else {
        p.inset
    };
    ui.painter().rect_filled(rect, CornerRadius::same(12), fill);

    let cancel_rect = Rect::from_center_size(
        Pos2::new(rect.right() - 14.0 - CANCEL_SIZE / 2.0, rect.top() + 18.0),
        Vec2::splat(CANCEL_SIZE),
    );
    let name_font = theme::bold(12.0);
    let left = rect.left() + 14.0;
    if let Some(reason) = &upload.failed {
        // Name above, the reason in red below, both cut to what the button leaves.
        let room = (cancel_rect.left() - 10.0 - left).max(40.0);
        let name = widgets::fit_to_width(ui.painter(), &upload.name, &name_font, room);
        ui.painter().text(
            Pos2::new(left, rect.top() + 15.0),
            Align2::LEFT_CENTER,
            name,
            name_font,
            p.text,
        );
        let font = theme::text(10.5);
        let reason = widgets::fit_to_width(ui.painter(), reason, &font, room);
        ui.painter().text(
            Pos2::new(left, rect.bottom() - 13.0),
            Align2::LEFT_CENTER,
            reason,
            font,
            p.error,
        );
    } else {
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
        let name_room = (state_left - 12.0 - left).max(40.0);
        let name = widgets::fit_to_width(ui.painter(), &upload.name, &name_font, name_room);
        ui.painter().text(
            Pos2::new(left, rect.top() + 18.0),
            Align2::LEFT_CENTER,
            name,
            name_font,
            p.text,
        );
        let bar = Rect::from_min_max(
            Pos2::new(left, rect.bottom() - 11.0),
            Pos2::new(rect.right() - 14.0, rect.bottom() - 7.0),
        );
        icons::progress_bar(
            ui.painter(),
            bar,
            f32::from(upload.percent()) / 100.0,
            theme::YELLOW,
        );
    }

    stop_button(ui, cancel_rect, upload, locale)
}

/// The ✕ of a row: stops an upload that is going, or puts a failed one away. Returns whether it was pressed.
fn stop_button(ui: &mut egui::Ui, cancel_rect: Rect, upload: &Upload, locale: Locale) -> bool {
    let p = theme::pal();
    let response = ui
        .interact(
            cancel_rect,
            Id::new(("upload-cancel", upload.op.0)),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let label = if upload.failed.is_some() {
        Msg::A11yUploadDismiss {
            name: upload.name.clone(),
        }
    } else {
        Msg::A11yUploadCancel {
            name: upload.name.clone(),
        }
    }
    .text(locale);
    widgets::describe(&response, egui::WidgetType::Button, &label);
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
            failed: None,
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

    fn failed(op: u64, name: &str, why: &str) -> Upload {
        Upload {
            failed: Some(why.into()),
            ..upload(op, name, 10, 0)
        }
    }

    #[test]
    fn a_failed_upload_keeps_its_row_with_the_reason_and_a_button_to_put_it_away() {
        let list = [
            failed(1, "a.mkv", "File too large"),
            upload(2, "b.zip", 50, 10),
        ];
        let boxes = text_boxes(|ui| {
            strip(ui, &list, Locale::En);
        });
        let texts: Vec<&str> = boxes.iter().map(|b| b.text.as_str()).collect();
        for wanted in ["a.mkv", "File too large", "b.zip", "20%"] {
            assert!(texts.contains(&wanted), "missing {wanted:?} in {texts:?}");
        }
        // The failed one has no percentage or wait of its own, and the one after it is the one sending.
        assert!(
            !texts.contains(&"Waiting") && !texts.contains(&"0%"),
            "{texts:?}"
        );
        let names = accessible_names(|ui| {
            strip(ui, &list, Locale::En);
        });
        for wanted in [
            "Dismiss the failed upload of a.mkv",
            "Cancel upload of b.zip",
        ] {
            assert!(
                names
                    .iter()
                    .any(|(role, name)| role == "Button" && name == wanted),
                "missing {wanted:?} in {names:?}"
            );
        }
    }

    #[test]
    fn the_reasons_a_user_most_needs_are_shown_whole_in_a_failed_row_in_every_language() {
        for locale in [Locale::En, Locale::De, Locale::Es] {
            for reason in [
                Msg::PaymentRequired.text(locale),
                Msg::UploadNoSpace {
                    needed: "1023.9 GB".into(),
                    free: "1023.9 GB".into(),
                }
                .text(locale),
            ] {
                let list = [failed(1, "a.mkv", &reason)];
                let boxes = text_boxes(|ui| {
                    ui.allocate_ui(Vec2::new(512.0, 300.0), |ui| {
                        strip(ui, &list, locale);
                    });
                });
                assert!(
                    boxes.iter().any(|b| b.text == reason),
                    "{locale:?}: {reason:?} is cut in {:?}",
                    boxes.iter().map(|b| &b.text).collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn a_long_reason_and_name_stay_inside_a_failed_row_in_every_language() {
        let long = "An.Extremely.Long.File.Name.".repeat(8);
        let why = "A reason that goes on and on and on, far past what a row can hold. ".repeat(4);
        let list = [failed(1, &long, &why)];
        for locale in [Locale::En, Locale::De, Locale::Es] {
            let mut used = Rect::NOTHING;
            let boxes = text_boxes(|ui| {
                ui.allocate_ui(Vec2::new(512.0, 300.0), |ui| {
                    strip(ui, &list, locale);
                    used = ui.min_rect();
                });
            });
            assert!(used.width() <= 512.0 + 0.5, "{locale:?}: {used:?}");
            for text in &boxes {
                assert!(
                    text.full.right() <= used.right() - 14.0 - CANCEL_SIZE,
                    "{locale:?}: {:?} runs into the button ({:?})",
                    text.text,
                    text.full
                );
            }
        }
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
