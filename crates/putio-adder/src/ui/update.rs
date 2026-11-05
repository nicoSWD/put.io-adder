//! How an available update is shown and what clicking it does.
//!
//! One pure function, [`look`], turns the core's update state into what to draw (a label, a tint, a
//! progress fraction, the click action). The toolbar pill, the "..." menu entry and the Preferences
//! section all use it, so they can never disagree about what state the update is in.

use eframe::egui::{self, Align2, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};
use putio_core::i18n::Msg;
use putio_core::state::{Event, Install, InstallTarget, State, UpdateStage};
use putio_core::update::UpdateStatus;

use super::icons::{self, MenuIcon};
use super::theme;

/// What clicking does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Click {
    /// Download and unpack the update.
    Install,
    /// Swap it in and restart.
    Restart,
    /// Open the release page (an update this copy cannot install itself).
    OpenRelease,
}

impl Click {
    pub(crate) fn event(self) -> Event {
        match self {
            Self::Install => Event::UpdateInstallRequested,
            Self::Restart => Event::UpdateRestartRequested,
            Self::OpenRelease => Event::OpenReleasePage,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tone {
    /// Something to do: yellow.
    Accent,
    /// Work in progress.
    Busy,
    /// Went wrong.
    Danger,
    /// Only a link out.
    Quiet,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Look {
    pub(crate) label: String,
    /// The longer text (what is available, why it cannot be installed, what went wrong).
    pub(crate) tooltip: String,
    pub(crate) tone: Tone,
    pub(crate) icon: MenuIcon,
    /// Overall progress while installing.
    pub(crate) progress: Option<f32>,
    /// `None` while there is nothing to click.
    pub(crate) click: Option<Click>,
}

/// What to show for the update, or `None` when there is nothing to say (up to date, or unknown).
pub(crate) fn look(state: &State) -> Option<Look> {
    let locale = state.locale;
    let UpdateStatus::Available { version, asset, .. } = &state.update else {
        return None;
    };
    let available = Msg::UpdateAvailable {
        version: version.clone(),
    }
    .text(locale);
    let installable = asset.is_some() && state.install_target == InstallTarget::Ready;
    // Why the release can only be opened, not installed.
    let why_manual = match &state.install_target {
        InstallTarget::Unavailable(reason) => reason.clone(),
        _ => String::new(),
    };
    let with_reason = |text: String, reason: &str| {
        if reason.is_empty() {
            text
        } else {
            format!("{text}. {reason}")
        }
    };

    Some(match &state.install {
        Install::Working { stage, fraction } => Look {
            label: match stage {
                UpdateStage::Downloading => Msg::UpdateDownloading {
                    percent: percent(*fraction),
                }
                .text(locale),
                UpdateStage::Unpacking => Msg::UpdateUnpacking.text(locale),
            },
            tooltip: available,
            tone: Tone::Busy,
            icon: MenuIcon::Download,
            progress: Some(*fraction),
            click: None,
        },
        Install::Ready { version } => Look {
            label: Msg::UpdateRestartNow.text(locale),
            tooltip: Msg::UpdateReady {
                version: version.clone(),
            }
            .text(locale),
            tone: Tone::Accent,
            icon: MenuIcon::Refresh,
            progress: None,
            click: Some(Click::Restart),
        },
        Install::Failed { message, .. } if installable => Look {
            label: Msg::UpdateTryAgain.text(locale),
            tooltip: Msg::UpdateFailed {
                reason: message.clone(),
            }
            .text(locale),
            tone: Tone::Danger,
            icon: MenuIcon::Refresh,
            progress: None,
            click: Some(Click::Install),
        },
        Install::Idle | Install::Failed { .. } if installable => Look {
            label: Msg::UpdateInstallNow.text(locale),
            tooltip: available,
            tone: Tone::Accent,
            icon: MenuIcon::Download,
            progress: None,
            click: Some(Click::Install),
        },
        _ => Look {
            label: Msg::UpdateOpenPage.text(locale),
            tooltip: with_reason(available, &why_manual),
            tone: Tone::Quiet,
            icon: MenuIcon::External,
            progress: None,
            click: Some(Click::OpenRelease),
        },
    })
}

fn percent(fraction: f32) -> u8 {
    // Clamped to 0..=100 first, so the cast loses nothing that matters.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let percent = (fraction.clamp(0.0, 1.0) * 100.0).round() as u8;
    percent
}

const PILL_HEIGHT: f32 = 26.0;
const PROGRESS_WIDTH: f32 = 132.0;

/// The pill in the toolbar. Painted at fixed positions: when the pointer arrives only the background
/// changes, so the label and the icon never move. Returns the response (clicks only count when
/// `look.click` is set).
pub(crate) fn pill(ui: &mut egui::Ui, look: &Look) -> egui::Response {
    let p = theme::pal();
    let font = theme::bold(11.0);
    let galley = ui
        .painter()
        .layout_no_wrap(look.label.clone(), font, egui::Color32::PLACEHOLDER);
    let width = if look.progress.is_some() {
        PROGRESS_WIDTH
    } else {
        10.0 + 16.0 + 6.0 + galley.size().x + 12.0
    };
    let sense = if look.click.is_some() {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, PILL_HEIGHT), sense);
    let response = if look.click.is_some() {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };
    let hovered = response.hovered() && look.click.is_some();
    let round = CornerRadius::same(13);
    let (fill, ink, border) = match look.tone {
        Tone::Accent => (
            if hovered {
                p.yellow_pressed
            } else {
                theme::YELLOW
            },
            p.on_yellow,
            None,
        ),
        Tone::Busy => (p.inset, p.text, Some(p.divider)),
        Tone::Danger => (
            if hovered {
                p.error_soft.gamma_multiply(0.85)
            } else {
                p.error_soft
            },
            p.error,
            Some(p.error),
        ),
        Tone::Quiet => (
            if hovered { p.card_hover } else { p.inset },
            p.text,
            Some(p.divider),
        ),
    };
    ui.painter().rect_filled(rect, round, fill);
    if let Some(fraction) = look.progress {
        // The label, and a thin bar along the bottom edge: the label stays readable on any background.
        ui.painter().text(
            rect.center() - Vec2::new(0.0, 1.5),
            Align2::CENTER_CENTER,
            &look.label,
            theme::bold(11.0),
            ink,
        );
        let bar = Rect::from_min_max(
            Pos2::new(rect.left() + 12.0, rect.bottom() - 6.0),
            Pos2::new(rect.right() - 12.0, rect.bottom() - 3.0),
        );
        icons::progress_bar(ui.painter(), bar, fraction, theme::YELLOW);
    } else {
        icons::menu_icon(
            ui.painter(),
            Pos2::new(rect.left() + 10.0 + 8.0, rect.center().y),
            look.icon,
            ink,
        );
        ui.painter().galley(
            Pos2::new(
                rect.left() + 10.0 + 16.0 + 6.0,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            ink,
        );
    }
    if let Some(border) = border {
        ui.painter().rect_stroke(
            rect,
            round,
            Stroke::new(1.0, border.gamma_multiply(0.6)),
            egui::StrokeKind::Inside,
        );
    }
    response.on_hover_text(&look.tooltip)
}

#[cfg(test)]
mod tests {
    use putio_core::i18n::Locale;
    use putio_core::settings::Settings;
    use putio_core::update::UpdateAsset;

    use super::*;
    use crate::ui::test_support::text_with_and_without_hover;

    fn asset() -> UpdateAsset {
        UpdateAsset {
            name: "putio-adder-v0.2.0-x.zip".into(),
            url: "https://github.com/nicoSWD/putio-adder-rs/releases/download/v0.2.0/putio-adder-v0.2.0-x.zip".into(),
            size: 1_000_000,
        }
    }

    fn state(with_asset: bool, target: InstallTarget, install: Install) -> State {
        let mut state = State::new(Settings::default(), Locale::En, "0.1.0");
        state.update = UpdateStatus::Available {
            version: "0.2.0".into(),
            url: "https://github.com/nicoSWD/putio-adder-rs/releases/tag/v0.2.0".into(),
            asset: with_asset.then(asset),
        };
        state.install_target = target;
        state.install = install;
        state
    }

    #[test]
    fn nothing_is_shown_without_an_update_on_offer() {
        let mut state = State::new(Settings::default(), Locale::En, "0.1.0");
        for status in [
            UpdateStatus::Unknown,
            UpdateStatus::Checking,
            UpdateStatus::UpToDate,
        ] {
            state.update = status;
            assert_eq!(look(&state), None);
        }
    }

    #[test]
    fn an_installable_update_offers_to_install() {
        let look = look(&state(true, InstallTarget::Ready, Install::Idle)).unwrap();
        assert_eq!(look.label, "Update now");
        assert_eq!(
            (look.tone, look.click, look.progress),
            (Tone::Accent, Some(Click::Install), None)
        );
        assert!(look.tooltip.contains("0.2.0"));
    }

    #[test]
    fn an_update_this_copy_cannot_install_only_opens_the_release_page() {
        for (with_asset, target) in [
            (false, InstallTarget::Ready),
            (true, InstallTarget::Unknown),
            (
                true,
                InstallTarget::Unavailable("A development build does not update itself.".into()),
            ),
        ] {
            let look = look(&state(with_asset, target.clone(), Install::Idle)).unwrap();
            assert_eq!(
                (look.tone, look.click),
                (Tone::Quiet, Some(Click::OpenRelease)),
                "{target:?}"
            );
            assert_eq!(look.label, "View release");
        }
        let blocked = look(&state(
            true,
            InstallTarget::Unavailable("Not writable.".into()),
            Install::Idle,
        ))
        .unwrap();
        assert!(
            blocked.tooltip.contains("Not writable."),
            "the reason is given: {}",
            blocked.tooltip
        );
    }

    #[test]
    fn while_installing_the_pill_shows_progress_and_cannot_be_clicked() {
        let working = Install::Working {
            stage: UpdateStage::Downloading,
            fraction: 0.46,
        };
        let look = look(&state(true, InstallTarget::Ready, working)).unwrap();
        assert_eq!(look.label, "Updating… 46%");
        assert_eq!(
            (look.tone, look.click, look.progress),
            (Tone::Busy, None, Some(0.46))
        );
        let unpacking = Install::Working {
            stage: UpdateStage::Unpacking,
            fraction: 0.95,
        };
        assert_eq!(
            super::look(&state(true, InstallTarget::Ready, unpacking))
                .unwrap()
                .label,
            "Preparing the update…"
        );
    }

    #[test]
    fn a_prepared_update_asks_for_the_restart() {
        let ready = Install::Ready {
            version: "0.2.0".into(),
        };
        let look = look(&state(true, InstallTarget::Ready, ready)).unwrap();
        assert_eq!(look.label, "Restart to finish");
        assert_eq!(
            (look.tone, look.click),
            (Tone::Accent, Some(Click::Restart))
        );
    }

    #[test]
    fn a_failed_install_says_why_and_offers_another_try() {
        let failed = Install::Failed {
            version: "0.2.0".into(),
            message: "The download is incomplete.".into(),
        };
        let look = look(&state(true, InstallTarget::Ready, failed.clone())).unwrap();
        assert_eq!(look.label, "Try again");
        assert_eq!(
            (look.tone, look.click),
            (Tone::Danger, Some(Click::Install))
        );
        assert!(look.tooltip.contains("The download is incomplete."));
        // Without an installable file there is nothing to retry: the release page is all that is left.
        let manual = super::look(&state(false, InstallTarget::Ready, failed)).unwrap();
        assert_eq!(manual.click, Some(Click::OpenRelease));
    }

    #[test]
    fn the_labels_follow_the_language() {
        let mut german = state(true, InstallTarget::Ready, Install::Idle);
        german.locale = Locale::De;
        assert_eq!(look(&german).unwrap().label, "Jetzt aktualisieren");
        german.locale = Locale::Es;
        assert_eq!(look(&german).unwrap().label, "Actualizar ahora");
    }

    #[test]
    fn each_click_maps_to_one_event() {
        assert!(matches!(
            Click::Install.event(),
            Event::UpdateInstallRequested
        ));
        assert!(matches!(
            Click::Restart.event(),
            Event::UpdateRestartRequested
        ));
        assert!(matches!(Click::OpenRelease.event(), Event::OpenReleasePage));
    }

    #[test]
    fn the_pill_does_not_move_its_text_when_hovered() {
        let working = Install::Working {
            stage: UpdateStage::Downloading,
            fraction: 0.3,
        };
        let failed = Install::Failed {
            version: "0.2.0".into(),
            message: "x".into(),
        };
        for (with_asset, target, install) in [
            (true, InstallTarget::Ready, Install::Idle),
            (
                true,
                InstallTarget::Ready,
                Install::Ready {
                    version: "0.2.0".into(),
                },
            ),
            (true, InstallTarget::Ready, failed),
            (false, InstallTarget::Ready, Install::Idle),
        ] {
            let look = look(&state(with_asset, target, install)).unwrap();
            let (resting, hovered) = text_with_and_without_hover(|ui| {
                let response = pill(ui, &look);
                (response.rect, response.hovered())
            });
            assert_eq!(resting, hovered, "{}", look.label);
        }
        // A pill with nothing to click is not hovered at all, so it is drawn the same either way.
        let busy = look(&state(true, InstallTarget::Ready, working)).unwrap();
        let mut ctx_rect = Rect::NOTHING;
        let ctx = egui::Context::default();
        theme::apply(&ctx, putio_core::settings::ThemeChoice::Light);
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ctx_rect = pill(ui, &busy).rect;
        });
        output.drop_without_applying_deltas();
        assert_eq!(
            ctx_rect.width(),
            PROGRESS_WIDTH,
            "the progress pill keeps one width"
        );
    }
}
