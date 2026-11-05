//! The transfers list.

use eframe::egui::{self, Align, Layout, RichText, Sense};
use egui_extras::{Column, TableBuilder};
use putio_core::format;
use putio_core::i18n::Msg;
use putio_core::state::Event;
use putio_core::transfers::{self, SortKey};
use putio_core::update::UpdateStatus;

use super::theme;
use crate::app::App;

const ROW_HEIGHT: f32 = 20.0;
const HEADER_HEIGHT: f32 = 20.0;

#[allow(clippy::too_many_lines)] // immediate-mode drawing reads best as one top-to-bottom function
pub(crate) fn show(ui: &mut egui::Ui, app: &mut App) {
    let ctx = ui.ctx().clone();
    let locale = app.state.locale;

    if let UpdateStatus::Available { version, .. } = app.state.update.clone() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(Msg::UpdateAvailable { version }.text(locale)).strong());
            if ui.link(Msg::UpdateOpenPage.text(locale)).clicked() {
                app.dispatch(&ctx, Event::OpenReleasePage);
            }
        });
    }

    // A sorted copy for display: the core keeps its own (default) order.
    let mut rows = app.state.transfers.clone();
    transfers::sort(&mut rows, app.ui.sort, app.ui.ascending);

    let mut action: Option<Event> = None;
    let mut sort_click: Option<SortKey> = None;

    TableBuilder::new(ui)
        .striped(true)
        .sense(Sense::click())
        .cell_layout(Layout::left_to_right(Align::Center))
        .column(Column::remainder().at_least(120.0).clip(true))
        .column(Column::exact(90.0))
        .column(Column::exact(70.0))
        .column(Column::exact(64.0))
        .column(Column::exact(68.0))
        .header(HEADER_HEIGHT, |mut header| {
            let columns = [
                (Msg::ColName, SortKey::Name),
                (Msg::ColProgress, SortKey::Progress),
                (Msg::ColSpeed, SortKey::Speed),
                (Msg::ColEta, SortKey::Eta),
                (Msg::ColSize, SortKey::Size),
            ];
            for (title, key) in columns {
                header.col(|ui| {
                    let arrow = if app.ui.sort == key {
                        if app.ui.ascending { " ▲" } else { " ▼" }
                    } else {
                        ""
                    };
                    let text = RichText::new(format!("{}{arrow}", title.text(locale)))
                        .strong()
                        .color(theme::MUTED);
                    if ui
                        .add(egui::Label::new(text).sense(Sense::click()))
                        .clicked()
                    {
                        sort_click = Some(key);
                    }
                });
            }
        })
        .body(|body| {
            body.rows(ROW_HEIGHT, rows.len(), |mut row| {
                let Some(transfer) = rows.get(row.index()) else {
                    return;
                };
                row.col(|ui| {
                    ui.label(&transfer.name);
                });
                row.col(|ui| {
                    ui.add(
                        egui::ProgressBar::new(f32::from(transfer.percent()) / 100.0)
                            .desired_width(80.0)
                            .desired_height(10.0),
                    );
                });
                row.col(|ui| {
                    let speed = if transfer.status == putio_api::TransferStatus::Downloading {
                        format::speed(transfer.down_speed)
                    } else {
                        "–".into()
                    };
                    ui.label(RichText::new(speed).small());
                });
                row.col(|ui| {
                    ui.label(
                        RichText::new(format::eta(
                            transfer.status,
                            transfer.estimated_time,
                            locale,
                        ))
                        .small(),
                    );
                });
                row.col(|ui| {
                    ui.label(RichText::new(format::bytes(transfer.size)).small());
                });

                let response = row.response();
                if response.double_clicked() {
                    action = Some(Event::OpenTransferOnPutio(transfer.id));
                }
                response.context_menu(|ui| {
                    // Everything is keyed by transfer *id*, never by row position: the list can change
                    // between opening the menu and clicking an item.
                    if ui.button(Msg::MenuOpenOnPutio.text(locale)).clicked() {
                        action = Some(Event::OpenTransferOnPutio(transfer.id));
                        ui.close();
                    }
                    if transfer.file_id.is_some()
                        && ui
                            .button(
                                Msg::MenuStream {
                                    name: transfer.name.clone(),
                                }
                                .text(locale),
                            )
                            .clicked()
                    {
                        action = Some(Event::StreamRequested(transfer.id));
                        ui.close();
                    }
                    if !transfer.status.is_finished()
                        && ui
                            .button(
                                Msg::MenuCancelTransfer {
                                    name: transfer.name.clone(),
                                }
                                .text(locale),
                            )
                            .clicked()
                    {
                        action = Some(Event::CancelRequested(transfer.id));
                        ui.close();
                    }
                });
            });
        });

    if let Some(key) = sort_click {
        if app.ui.sort == key {
            app.ui.ascending = !app.ui.ascending;
        } else {
            app.ui.sort = key;
            app.ui.ascending = true;
        }
    }
    if let Some(event) = action {
        app.dispatch(&ctx, event);
    }
}
