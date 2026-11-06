//! The details panel: what put.io knows about one transfer (peers, ratio, tracker message, …).
//!
//! A dialog like Preferences: a header card with the transfer's state, the facts in titled groups,
//! put.io's and the tracker's messages as callouts, and the actions that make sense next. The rows come
//! from [`putio_core::format::details`], which leaves out anything the server did not send, so the panel
//! never shows empty or made-up values. While it is open the core fetches the transfer on its own every few
//! seconds (see `putio_core::state::TransferDetail`), so the numbers keep moving, and the panel closes
//! itself when put.io no longer knows the transfer. Two facts need a second look-up: the name of the folder
//! it saves into (asked of put.io once and remembered) and the title of the RSS feed that added it (known
//! once the Feeds tab has been looked at).

use eframe::egui::{self, Align2, CornerRadius, Margin, Pos2, Rect, RichText, Sense, Vec2};
use putio_api::{Transfer, TransferStatus};
use putio_core::format::{self, DetailGroup, DetailRow};
use putio_core::i18n::{Locale, Msg};
use putio_core::settings::Tab;
use putio_core::state::Event;

use super::icons::{self, MenuIcon};
use super::{prefs, theme, widgets};
use crate::app::App;
use crate::worker::Job;

/// Width of the content inside the card (the window is 560 wide).
const CONTENT_WIDTH: f32 = 456.0;
const ROW_HEIGHT: f32 = 29.0;
/// The least space between a fact's label and its value.
const VALUE_GAP: f32 = 16.0;
/// Space the dialog needs besides the scrolling body: padding, header and the buttons.
const CHROME_HEIGHT: f32 = 190.0;

/// What the buttons at the bottom ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Download,
    Retry,
    ShowInFiles,
    OpenOnPutio,
}

/// What the panel knows about a transfer beyond the transfer itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Place {
    /// The name of the folder it saves into.
    saved_in: Option<String>,
    /// The folder as the Files tab can show it (number and name), when both are known.
    folder: Option<(i64, String)>,
    /// The title of the RSS feed that added it (or a generic word when the feed is not known).
    feed: Option<String>,
}

fn place(app: &mut App, transfer: &Transfer, locale: Locale) -> Place {
    let mut place = Place::default();
    if let Some(parent) = transfer.save_parent_id.filter(|id| *id >= 0) {
        let name = if parent == 0 {
            Some(Msg::FilesRoot.text(locale))
        } else {
            let known = app.ui.folder_names.get(&parent).cloned();
            if known.is_none() {
                // Asked once; the answer (or `None` for "could not say") is remembered.
                app.ui.folder_names.insert(parent, None);
                app.send(Job::FolderName { id: parent });
            }
            known.flatten()
        };
        place.folder = name.clone().map(|name| (parent, name));
        place.saved_in = name;
    }
    if let Some(feed) = transfer.subscription_id {
        let title = app
            .state
            .feeds
            .items
            .iter()
            .find(|item| item.id == feed)
            .map(|item| item.title.trim())
            .filter(|title| !title.is_empty());
        place.feed =
            Some(title.map_or_else(|| Msg::DetailFromFeedUnknown.text(locale), str::to_owned));
    }
    place
}

pub(crate) fn show(ctx: &egui::Context, app: &mut App) {
    let Some(id) = app.state.detail.as_ref().map(|detail| detail.id) else {
        return;
    };
    // Gone from put.io (or from the list, before the first look-up came back): nothing to show.
    let Some(transfer) = app.state.detail_transfer().cloned() else {
        app.dispatch(ctx, Event::TransferDetailsClosed);
        return;
    };
    let locale = app.state.locale;
    let place = place(app, &transfer, locale);
    let (frame, backdrop) = widgets::dialog_frame();
    let mut action = None;
    let response = egui::Modal::new(egui::Id::unique("transfer-details"))
        .backdrop_color(backdrop)
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(CONTENT_WIDTH);
            let (_, close) = prefs::header(ui, &Msg::DetailsTitle.text(locale), false, locale);
            let height = (ui.ctx().content_rect().height() - CHROME_HEIGHT).max(160.0);
            prefs::scroll_region(ui, height, |ui| body(ui, &transfer, &place, locale));
            ui.add_space(10.0);
            action = footer(ui, &transfer, place.folder.is_some(), locale);
            close
        });
    // The status line is behind the dialog, so an action closes it to let the result show.
    if let Some(action) = action {
        app.dispatch(ctx, Event::TransferDetailsClosed);
        match action {
            Action::Download => app.dispatch(ctx, Event::DownloadRequested(id)),
            Action::Retry => app.dispatch(ctx, Event::RetryRequested(id)),
            Action::OpenOnPutio => app.dispatch(ctx, Event::OpenTransferOnPutio(id)),
            Action::ShowInFiles => {
                if let Some((folder, name)) = place.folder {
                    app.dispatch(ctx, Event::SelectTab(Tab::Files));
                    app.dispatch(ctx, Event::FilesReveal { folder, name });
                    app.ui.selected_file = transfer.file_id;
                }
            }
        }
    } else if response.inner || response.should_close() {
        app.dispatch(ctx, Event::TransferDetailsClosed);
    }
}

/// The state card, then the groups of facts, then the messages.
fn body(ui: &mut egui::Ui, transfer: &Transfer, place: &Place, locale: Locale) {
    ui.spacing_mut().item_spacing.y = 8.0;
    hero(ui, transfer, locale);

    let mut rows = format::details(transfer, locale);
    for (label, value) in [
        (Msg::DetailSavedIn, &place.saved_in),
        (Msg::DetailFromFeed, &place.feed),
    ] {
        if let Some(value) = value {
            rows.push(DetailRow {
                group: DetailGroup::Transfer,
                label,
                value: value.clone(),
            });
        }
    }
    for (group, title) in [
        (DetailGroup::Transfer, Msg::DetailGroupTransfer),
        (DetailGroup::Sharing, Msg::DetailGroupSharing),
        (DetailGroup::Dates, Msg::DetailGroupDates),
    ] {
        let facts: Vec<&DetailRow> = rows.iter().filter(|row| row.group == group).collect();
        if facts.is_empty() {
            continue;
        }
        widgets::section_title(ui, &title.text(locale));
        widgets::group(ui, |ui| {
            for (index, row) in facts.iter().enumerate() {
                if index > 0 {
                    widgets::divider(ui);
                }
                fact(ui, row, locale);
            }
        });
    }
    for row in rows.iter().filter(|row| row.group == DetailGroup::Messages) {
        callout(ui, row, locale);
    }
    ui.add_space(4.0);
}

/// The transfer's badge and name, what it is doing, and its progress while it is running.
fn hero(ui: &mut egui::Ui, transfer: &Transfer, locale: Locale) {
    let p = theme::pal();
    egui::Frame::new()
        .fill(p.inset)
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                let (badge, _) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::hover());
                icons::status_badge(ui.painter(), badge.center(), transfer.status);
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    let name = if transfer.name.is_empty() {
                        "…"
                    } else {
                        transfer.name.as_str()
                    };
                    ui.add(
                        egui::Label::new(RichText::new(name).font(theme::bold(14.0)).color(p.text))
                            .wrap_mode(egui::TextWrapMode::Wrap),
                    );
                    let failed = transfer.status == TransferStatus::Error;
                    ui.label(
                        RichText::new(format::headline(transfer, locale))
                            .font(theme::text(11.5))
                            .color(if failed { p.error } else { p.muted }),
                    );
                });
            });
            if transfer.status.is_pending() {
                progress(ui, transfer);
            }
        });
}

/// A full-width bar with the percentage at its right end.
fn progress(ui: &mut egui::Ui, transfer: &Transfer) {
    let p = theme::pal();
    ui.add_space(10.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 14.0), Sense::hover());
    let percent = f32::from(transfer.percent());
    let bar = Rect::from_min_max(
        Pos2::new(rect.left(), rect.center().y - 3.0),
        Pos2::new(rect.right() - 44.0, rect.center().y + 3.0),
    );
    icons::progress_bar(ui.painter(), bar, percent / 100.0, theme::YELLOW);
    ui.painter().text(
        Pos2::new(rect.right(), rect.center().y),
        Align2::RIGHT_CENTER,
        format!("{percent:.0}%"),
        theme::bold(12.0),
        p.accent_text,
    );
}

/// A label on the left and its value on the right, both painted at fixed positions.
fn fact(ui: &mut egui::Ui, row: &DetailRow, locale: Locale) {
    let p = theme::pal();
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_HEIGHT), Sense::hover());
    let label = ui
        .painter()
        .layout_no_wrap(row.label.text(locale), theme::text(12.0), p.muted);
    // A long value (a folder's name) is cut so that it never runs into its label.
    let room = (rect.width() - label.size().x - VALUE_GAP).max(0.0);
    let value = widgets::fit_to_width(ui.painter(), &row.value, &theme::bold(12.5), room);
    ui.painter().galley(
        Pos2::new(rect.left(), rect.center().y - label.size().y / 2.0),
        label,
        p.muted,
    );
    ui.painter().text(
        Pos2::new(rect.right(), rect.center().y),
        Align2::RIGHT_CENTER,
        value,
        theme::bold(12.5),
        p.text,
    );
}

/// Free text (an error, the tracker's message) in a tinted box: red for an error, quiet otherwise.
fn callout(ui: &mut egui::Ui, row: &DetailRow, locale: Locale) {
    let p = theme::pal();
    let is_error = row.label == Msg::DetailError;
    let (fill, title_color) = if is_error {
        (p.error_soft, p.error)
    } else {
        (p.inset, p.muted)
    };
    egui::Frame::new()
        .fill(fill)
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(14, 11))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.label(
                RichText::new(row.label.text(locale).to_uppercase())
                    .font(theme::bold(10.0))
                    .color(title_color),
            );
            ui.add(
                egui::Label::new(
                    RichText::new(&row.value)
                        .font(theme::text(12.5))
                        .color(p.text),
                )
                .wrap_mode(egui::TextWrapMode::Wrap),
            );
        });
}

/// The buttons at the bottom: download (when there is something to download), retry (for a failed
/// transfer) and open on put.io.
fn footer(
    ui: &mut egui::Ui,
    transfer: &Transfer,
    can_show_in_files: bool,
    locale: Locale,
) -> Option<Action> {
    let mut action = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        // A failed transfer has nothing worth downloading or showing: retrying comes first.
        let has_files = transfer.file_id.is_some() && transfer.status != TransferStatus::Error;
        if has_files
            && widgets::action_button(
                ui,
                MenuIcon::Download,
                &Msg::MenuDownload.text(locale),
                true,
            )
            .clicked()
        {
            action = Some(Action::Download);
        }
        if transfer.status == TransferStatus::Error
            && widgets::action_button(ui, MenuIcon::Refresh, &Msg::MenuRetry.text(locale), true)
                .clicked()
        {
            action = Some(Action::Retry);
        }
        if has_files
            && can_show_in_files
            && widgets::action_button(
                ui,
                MenuIcon::Folder,
                &Msg::MenuShowInFiles.text(locale),
                false,
            )
            .clicked()
        {
            action = Some(Action::ShowInFiles);
        }
        if widgets::action_button(
            ui,
            MenuIcon::External,
            &Msg::MenuOpenOnPutio.text(locale),
            false,
        )
        .clicked()
        {
            action = Some(Action::OpenOnPutio);
        }
    });
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::test_support::{rendered_text, text_with_and_without_hover};
    use putio_api::TransferKind;

    fn seeding() -> Transfer {
        Transfer {
            id: 1,
            name: "Spring.2019.Blender.Open.Movie".into(),
            status: TransferStatus::Seeding,
            kind: TransferKind::Torrent,
            size: 4 * 1024 * 1024 * 1024,
            file_id: Some(5),
            peers: Some(14),
            availability: Some(100),
            current_ratio: Some(1.5),
            uploaded: Some(6 * 1024 * 1024 * 1024),
            tracker_message: Some("Announce OK".into()),
            ..Transfer::default()
        }
    }

    fn has(text: &[String], wanted: &str) -> bool {
        text.iter().any(|s| s == wanted)
    }

    #[test]
    fn the_panel_shows_peers_ratio_and_the_tracker_message() {
        let text = rendered_text(|ui| body(ui, &seeding(), &Place::default(), Locale::En));
        for expected in [
            "Spring.2019.Blender.Open.Movie",
            "Done · seeding",
            "SHARING",
            "Peers",
            "14",
            "Share ratio",
            "1.50",
            "TRACKER MESSAGE",
            "Announce OK",
        ] {
            assert!(has(&text, expected), "missing {expected:?} in {text:?}");
        }
    }

    #[test]
    fn facts_are_grouped_under_their_titles_in_order() {
        let text = rendered_text(|ui| body(ui, &seeding(), &Place::default(), Locale::En));
        let at = |wanted: &str| text.iter().position(|s| s == wanted).unwrap();
        assert!(at("TRANSFER") < at("Size"));
        assert!(at("Size") < at("SHARING"));
        assert!(at("SHARING") < at("Peers"));
        assert!(at("Peers") < at("TRACKER MESSAGE"));
    }

    #[test]
    fn what_the_server_did_not_send_is_not_shown() {
        let t = Transfer {
            id: 2,
            name: "Plain".into(),
            status: TransferStatus::Waiting,
            ..Transfer::default()
        };
        let text = rendered_text(|ui| body(ui, &t, &Place::default(), Locale::En));
        for absent in [
            "Peers",
            "Share ratio",
            "SHARING",
            "TRANSFER",
            "DATES",
            "TRACKER MESSAGE",
        ] {
            assert!(!has(&text, absent), "{absent:?} should be absent: {text:?}");
        }
        assert!(has(&text, "Plain") && has(&text, "Waiting…"));
    }

    #[test]
    fn a_running_transfer_shows_its_percentage_and_a_failed_one_its_reason() {
        let running = Transfer {
            id: 3,
            name: "Running".into(),
            status: TransferStatus::Downloading,
            percent_done: 42,
            ..Transfer::default()
        };
        assert!(has(
            &rendered_text(|ui| body(ui, &running, &Place::default(), Locale::En)),
            "42%"
        ));
        let mut finished = seeding();
        finished.availability = None;
        assert!(!has(
            &rendered_text(|ui| body(ui, &finished, &Place::default(), Locale::En)),
            "100%"
        ));
        let failed = Transfer {
            id: 4,
            name: "Broken".into(),
            status: TransferStatus::Error,
            error_message: Some("Tracker unreachable".into()),
            ..Transfer::default()
        };
        let text = rendered_text(|ui| body(ui, &failed, &Place::default(), Locale::En));
        assert!(has(&text, "Failed") && has(&text, "ERROR") && has(&text, "Tracker unreachable"));
    }

    #[test]
    fn the_panel_is_translated() {
        let text = rendered_text(|ui| body(ui, &seeding(), &Place::default(), Locale::De));
        assert!(has(&text, "Verhältnis"), "{text:?}");
        assert!(has(&text, "TRACKER-MELDUNG"), "{text:?}");
        assert!(has(&text, "TEILEN"), "{text:?}");
    }

    #[test]
    fn download_is_offered_only_when_there_is_something_to_download() {
        let with_file = rendered_text(|ui| {
            footer(ui, &seeding(), false, Locale::En);
        });
        assert!(has(&with_file, "Download") && has(&with_file, "Open on put.io"));
        let mut running = seeding();
        running.file_id = None;
        let without = rendered_text(|ui| {
            footer(ui, &running, false, Locale::En);
        });
        assert!(!has(&without, "Download") && has(&without, "Open on put.io"));
    }

    #[test]
    fn a_failed_transfer_can_be_retried_from_the_panel() {
        let failed = Transfer {
            status: TransferStatus::Error,
            ..Transfer::default()
        };
        let text = rendered_text(|ui| {
            footer(ui, &failed, false, Locale::En);
        });
        assert!(has(&text, "Retry") && !has(&text, "Download"));
        let fine = rendered_text(|ui| {
            footer(ui, &seeding(), false, Locale::En);
        });
        assert!(!has(&fine, "Retry"));
    }

    #[test]
    fn a_very_long_name_without_spaces_does_not_break_the_layout() {
        let mut t = seeding();
        t.name = "Some.Extremely.Long.Release.Name.".repeat(12);
        let text = rendered_text(|ui| body(ui, &t, &Place::default(), Locale::En));
        assert!(has(&text, &t.name));
    }

    fn saved_to_a_folder_from_a_feed() -> Place {
        Place {
            saved_in: Some("TV Shows".into()),
            folder: Some((4, "TV Shows".into())),
            feed: Some("Weekly Releases".into()),
        }
    }

    #[test]
    fn where_it_saves_and_which_feed_added_it_are_listed_with_the_transfer_facts() {
        let text =
            rendered_text(|ui| body(ui, &seeding(), &saved_to_a_folder_from_a_feed(), Locale::En));
        let at = |wanted: &str| text.iter().position(|s| s == wanted).unwrap();
        for expected in ["Saved in", "TV Shows", "Added by", "Weekly Releases"] {
            assert!(has(&text, expected), "missing {expected:?} in {text:?}");
        }
        // They belong to the Transfer group, before the sharing facts.
        assert!(
            at("Size") < at("Saved in") && at("Added by") < at("SHARING"),
            "{text:?}"
        );
        // Nothing is said when nothing is known.
        let plain = rendered_text(|ui| body(ui, &seeding(), &Place::default(), Locale::En));
        assert!(
            !has(&plain, "Saved in") && !has(&plain, "Added by"),
            "{plain:?}"
        );
    }

    #[test]
    fn a_very_long_folder_name_is_cut_before_it_reaches_its_label() {
        use crate::ui::test_support::text_boxes;
        let long = "An.Extremely.Long.Folder.Name.".repeat(10);
        for locale in [Locale::En, Locale::De, Locale::Es] {
            let row = DetailRow {
                group: DetailGroup::Transfer,
                label: Msg::DetailSavedIn,
                value: long.clone(),
            };
            let boxes = text_boxes(|ui| {
                ui.allocate_ui(Vec2::new(CONTENT_WIDTH, 100.0), |ui| fact(ui, &row, locale));
            });
            let label = boxes
                .iter()
                .find(|b| b.text == Msg::DetailSavedIn.text(locale))
                .unwrap();
            let value = boxes
                .iter()
                .find(|b| b.text.ends_with('…'))
                .unwrap_or_else(|| panic!("{boxes:?}"));
            assert!(
                label.full.right() + VALUE_GAP - 0.5 <= value.full.left(),
                "{locale:?}: {boxes:?}"
            );
            assert!(
                value.full.right() <= label.full.left() + CONTENT_WIDTH + 2.0,
                "{locale:?}: {boxes:?}"
            );
        }
    }

    #[test]
    fn every_button_fits_in_the_footer_in_every_language() {
        // The most buttons that can show at once: download, show in files and open on put.io, or retry
        // and open on put.io.
        let mut failed = seeding();
        failed.status = TransferStatus::Error;
        failed.file_id = Some(5);
        for locale in [Locale::En, Locale::De, Locale::Es] {
            for (transfer, buttons) in [(seeding(), 3), (failed.clone(), 2)] {
                let mut used = Rect::NOTHING;
                let boxes = crate::ui::test_support::text_boxes(|ui| {
                    ui.allocate_ui(Vec2::new(CONTENT_WIDTH, 100.0), |ui| {
                        footer(ui, &transfer, true, locale);
                        used = ui.min_rect();
                    });
                });
                assert_eq!(boxes.len(), buttons, "{locale:?}: {boxes:?}");
                assert!(
                    used.width() <= CONTENT_WIDTH,
                    "{locale:?}: {used:?} {boxes:?}"
                );
            }
        }
    }

    #[test]
    fn a_failed_transfer_offers_a_retry_rather_than_a_download() {
        let failed = Transfer {
            status: TransferStatus::Error,
            file_id: Some(5),
            ..Transfer::default()
        };
        let text = rendered_text(|ui| {
            footer(ui, &failed, false, Locale::En);
        });
        assert!(has(&text, "Retry") && !has(&text, "Download"), "{text:?}");
    }

    #[test]
    fn showing_the_folder_in_files_is_offered_only_when_it_is_known() {
        let without = rendered_text(|ui| {
            footer(ui, &seeding(), false, Locale::En);
        });
        assert!(!has(&without, "Show in Files"));
        let with = rendered_text(|ui| {
            footer(ui, &seeding(), true, Locale::En);
        });
        assert!(has(&with, "Show in Files"));
    }

    #[test]
    fn a_fact_does_not_move_its_text_when_hovered() {
        let row = DetailRow {
            group: DetailGroup::Sharing,
            label: Msg::DetailPeers,
            value: "14".into(),
        };
        let (resting, hovered) = text_with_and_without_hover(|ui| {
            let origin = ui.cursor().min;
            fact(ui, &row, Locale::En);
            let rect = Rect::from_min_size(origin, Vec2::new(ui.available_width(), ROW_HEIGHT));
            (rect, ui.rect_contains_pointer(rect))
        });
        assert_eq!(resting, hovered);
    }
}
