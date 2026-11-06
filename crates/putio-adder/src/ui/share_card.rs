//! The sharing card of a file: who it is shared with (each with a button to stop), and a choice of friends
//! to share it with.
//!
//! A modal like the other cards of the Files tab. Sharing reaches other people, so it is never a side
//! effect: friends are picked on purpose and shared with by the button. What put.io answers is shown as it
//! is, a refusal in its own words, and the card cannot be closed while a request is on its way.

use std::collections::HashSet;

use eframe::egui::{self, Align, Id, Layout, RichText};
use putio_api::{Friend, ShareWith};
use putio_core::i18n::{Locale, Msg};
use putio_core::state::{Event, Fetch, Problem, ShareAction, SharingView};

use super::{prefs, theme, widgets};
use crate::app::App;

/// Width of the content inside the card (the window is 560 wide).
const WIDTH: f32 = 420.0;
/// Space the card needs besides the scrolling body: padding, header, the name and the buttons.
const CHROME_HEIGHT: f32 = 230.0;

/// Who is ticked in the card.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Pick {
    /// Every friend. Excludes the single friends.
    everyone: bool,
    friends: HashSet<String>,
}

impl Pick {
    /// Who to share with, given the friends put.io listed: nobody listed is never picked.
    fn target(&self, friends: &[Friend]) -> Option<ShareWith> {
        if self.everyone {
            return Some(ShareWith::Everyone);
        }
        let names: Vec<String> = friends
            .iter()
            .filter(|friend| self.friends.contains(&friend.name))
            .map(|friend| friend.name.clone())
            .collect();
        (!names.is_empty()).then_some(ShareWith::Friends(names))
    }
}

/// Open the card for a file.
pub(crate) fn open(app: &mut App, ctx: &egui::Context, file_id: i64, name: String) {
    app.ui.share_pick = Pick::default();
    app.dispatch(ctx, Event::ShareOpened { file_id, name });
}

/// Debug builds only: some files marked as shared, and the card (`file-share`, `file-share-denied`,
/// `file-share-empty`) for a preview.
#[cfg(debug_assertions)]
pub(crate) fn demo(app: &mut App, ctx: &egui::Context, view: &str) {
    use putio_api::{ApiError, Share, SharedFile};
    let mark = |id: i64| SharedFile {
        id,
        name: String::new(),
    };
    app.dispatch(ctx, Event::SharedListed(Ok(vec![mark(11), mark(14)])));
    let Some(file) = app.state.files.items.iter().find(|f| f.id == 14).cloned() else {
        return;
    };
    let friend = |id: i64, name: &str| Friend {
        id,
        name: name.into(),
    };
    let (shares, friends) = match view {
        "file-share" => (
            Ok(vec![
                Share {
                    share_id: 1,
                    user_name: "jane".into(),
                },
                Share {
                    share_id: 2,
                    user_name: "john".into(),
                },
            ]),
            Ok(vec![
                friend(1, "jane"),
                friend(2, "john"),
                friend(3, "alex"),
                friend(4, "sam"),
            ]),
        ),
        "file-share-empty" => (Ok(vec![]), Ok(vec![])),
        "file-share-denied" => (Err(ApiError::Unauthorized), Err(ApiError::Unauthorized)),
        _ => return,
    };
    open(app, ctx, file.id, file.name.clone());
    app.dispatch(
        ctx,
        Event::SharedWithFetched {
            file_id: file.id,
            result: shares,
        },
    );
    app.dispatch(
        ctx,
        Event::FriendsFetched {
            file_id: file.id,
            result: friends,
        },
    );
}

pub(crate) fn show(ctx: &egui::Context, app: &mut App) {
    let Some(view) = app.state.sharing.clone() else {
        return;
    };
    let locale = app.state.locale;
    let busy = view.action == ShareAction::Working;
    let mut pick = std::mem::take(&mut app.ui.share_pick);
    let (frame, backdrop) = widgets::dialog_frame();
    let mut stop = None;
    let mut share = None;
    let response = egui::Modal::new(Id::unique("file-share"))
        .backdrop_color(backdrop)
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(WIDTH);
            let (_, close) = prefs::header(ui, &Msg::ShareTitle.text(locale), false, locale);
            name_line(ui, &view.file_name);
            let height = (ui.ctx().content_rect().height() - CHROME_HEIGHT).max(140.0);
            prefs::scroll_region(ui, height, |ui| {
                stop = body(ui, &view, &mut pick, locale);
            });
            ui.add_space(10.0);
            share = footer(ui, &view, &pick, locale);
            close
        });
    app.ui.share_pick = pick;
    if let Some(share_id) = stop {
        app.dispatch(ctx, Event::UnshareRequested(share_id));
    }
    if let Some(with) = share {
        app.dispatch(ctx, Event::ShareRequested(with));
    }
    if !busy && (response.inner || response.should_close()) {
        app.dispatch(ctx, Event::ShareClosed);
    }
}

/// The file the card is about, cut to the card.
fn name_line(ui: &mut egui::Ui, name: &str) {
    let font = theme::text(12.0);
    let text = widgets::fit_to_width(ui.painter(), name, &font, ui.available_width());
    ui.label(RichText::new(text).font(font).color(theme::pal().muted));
    ui.add_space(6.0);
}

fn muted(ui: &mut egui::Ui, text: String) {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .font(theme::text(11.5))
                .color(theme::pal().muted),
        )
        .wrap(),
    );
}

fn problem_text(problem: &Problem, locale: Locale) -> String {
    match problem {
        Problem::NotAllowed => Msg::ShareNotAllowed.text(locale),
        Problem::Failed(reason) => Msg::FileActionFailed {
            reason: reason.clone(),
        }
        .text(locale),
    }
}

/// Who it is shared with, then who it can be shared with. Returns the share to end, when a button asked.
fn body(ui: &mut egui::Ui, view: &SharingView, pick: &mut Pick, locale: Locale) -> Option<i64> {
    ui.spacing_mut().item_spacing.y = 8.0;
    let busy = view.action == ShareAction::Working;
    let mut stop = None;

    widgets::section_title(ui, &Msg::ShareSharedWith.text(locale));
    match &view.shares {
        Fetch::Loading => muted(ui, Msg::FilesLoading.text(locale)),
        Fetch::Failed(problem) => muted(ui, problem_text(problem, locale)),
        Fetch::Ready(list) if list.is_empty() => muted(ui, Msg::ShareNobody.text(locale)),
        Fetch::Ready(list) => widgets::group(ui, |ui| {
            for (index, share) in list.iter().enumerate() {
                if index > 0 {
                    widgets::divider(ui);
                }
                widgets::row(ui, &share.user_name, None, |ui| {
                    if widgets::text_button(ui, &Msg::ShareStop.text(locale), false).clicked()
                        && !busy
                    {
                        stop = Some(share.share_id);
                    }
                });
            }
        }),
    }

    widgets::section_title(ui, &Msg::ShareWithTitle.text(locale));
    match &view.friends {
        Fetch::Loading => muted(ui, Msg::FilesLoading.text(locale)),
        Fetch::Failed(problem) => muted(ui, problem_text(problem, locale)),
        Fetch::Ready(list) if list.is_empty() => muted(ui, Msg::ShareNoFriends.text(locale)),
        Fetch::Ready(list) => widgets::group(ui, |ui| {
            let all = Msg::ShareAllFriends.text(locale);
            let mut everyone = pick.everyone;
            widgets::row(ui, &all, None, |ui| {
                widgets::switch(ui, &mut everyone, &all);
            });
            if everyone != pick.everyone {
                pick.everyone = everyone;
                if everyone {
                    pick.friends.clear();
                }
            }
            for friend in list {
                widgets::divider(ui);
                let mut on = pick.friends.contains(&friend.name);
                let before = on;
                widgets::row(ui, &friend.name, None, |ui| {
                    widgets::switch(ui, &mut on, &friend.name);
                });
                if on != before {
                    if on {
                        pick.friends.insert(friend.name.clone());
                        pick.everyone = false;
                    } else {
                        pick.friends.remove(&friend.name);
                    }
                }
            }
        }),
    }
    ui.add_space(4.0);
    stop
}

/// What went wrong with the last request, and the button that shares (when somebody is picked).
fn footer(ui: &mut egui::Ui, view: &SharingView, pick: &Pick, locale: Locale) -> Option<ShareWith> {
    if let ShareAction::Failed(problem) = &view.action {
        ui.add(
            egui::Label::new(
                RichText::new(problem_text(problem, locale))
                    .font(theme::text(11.0))
                    .color(theme::pal().error),
            )
            .wrap(),
        );
        ui.add_space(6.0);
    }
    let friends = match &view.friends {
        Fetch::Ready(list) => list.as_slice(),
        _ => &[],
    };
    let target = pick.target(friends);
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if view.action != ShareAction::Working
                && let Some(target) = target
                && widgets::text_button(ui, &Msg::ShareConfirm.text(locale), true).clicked()
            {
                chosen = Some(target);
            }
        });
    });
    chosen
}

#[cfg(test)]
mod tests {
    use putio_api::Share;

    use super::*;
    use crate::ui::test_support::{rendered_text, text_boxes};

    fn friend(id: i64, name: &str) -> Friend {
        Friend {
            id,
            name: name.into(),
        }
    }

    fn view(shares: Fetch<Vec<Share>>, friends: Fetch<Vec<Friend>>) -> SharingView {
        SharingView {
            file_id: 5,
            file_name: "Movies".into(),
            shares,
            friends,
            action: ShareAction::Idle,
        }
    }

    fn with_friends() -> SharingView {
        view(
            Fetch::Ready(vec![Share {
                share_id: 30,
                user_name: "jane".into(),
            }]),
            Fetch::Ready(vec![friend(1, "jane"), friend(2, "john")]),
        )
    }

    fn has(text: &[String], wanted: &str) -> bool {
        text.iter().any(|s| s == wanted)
    }

    #[test]
    fn the_card_says_who_it_is_shared_with_and_offers_the_friends() {
        let v = with_friends();
        let text = rendered_text(|ui| {
            body(ui, &v, &mut Pick::default(), Locale::En);
        });
        for expected in [
            "SHARED WITH",
            "jane",
            "Stop sharing",
            "SHARE WITH",
            "All friends",
            "john",
        ] {
            assert!(has(&text, expected), "missing {expected:?} in {text:?}");
        }
    }

    #[test]
    fn every_state_of_a_list_has_its_words() {
        let cases = [
            (view(Fetch::Loading, Fetch::Loading), vec!["Loading files…"]),
            (
                view(Fetch::Ready(vec![]), Fetch::Ready(vec![])),
                vec![
                    "Not shared with anyone yet.",
                    "You have no friends on put.io yet.",
                ],
            ),
            (
                view(
                    Fetch::Failed(Problem::NotAllowed),
                    Fetch::Failed(Problem::Failed("Offline".into())),
                ),
                vec![
                    "put.io does not let this app do that.",
                    "That did not work: Offline",
                ],
            ),
        ];
        for (v, expected) in cases {
            let text = rendered_text(|ui| {
                body(ui, &v, &mut Pick::default(), Locale::En);
            });
            for wanted in expected {
                assert!(has(&text, wanted), "missing {wanted:?} in {text:?}");
            }
        }
    }

    #[test]
    fn who_is_picked_follows_the_friends_put_io_listed() {
        let friends = [friend(1, "jane"), friend(2, "john")];
        assert_eq!(Pick::default().target(&friends), None);
        let one = Pick {
            everyone: false,
            friends: ["john".to_owned()].into(),
        };
        assert_eq!(
            one.target(&friends),
            Some(ShareWith::Friends(vec!["john".into()]))
        );
        // A name that is not in the list (a friend who went away) is never sent.
        let gone = Pick {
            everyone: false,
            friends: ["mallory".to_owned()].into(),
        };
        assert_eq!(gone.target(&friends), None);
        let all = Pick {
            everyone: true,
            friends: HashSet::new(),
        };
        assert_eq!(all.target(&friends), Some(ShareWith::Everyone));
    }

    #[test]
    fn the_share_button_shows_only_when_somebody_is_picked_and_nothing_is_on_its_way() {
        let mut v = with_friends();
        let none = rendered_text(|ui| {
            footer(ui, &v, &Pick::default(), Locale::En);
        });
        assert!(!has(&none, "Share"), "{none:?}");
        let pick = Pick {
            everyone: true,
            friends: HashSet::new(),
        };
        let some = rendered_text(|ui| {
            footer(ui, &v, &pick, Locale::En);
        });
        assert!(has(&some, "Share"), "{some:?}");
        v.action = ShareAction::Working;
        let busy = rendered_text(|ui| {
            footer(ui, &v, &pick, Locale::En);
        });
        assert!(!has(&busy, "Share"), "{busy:?}");
        v.action = ShareAction::Failed(Problem::NotAllowed);
        let failed = rendered_text(|ui| {
            footer(ui, &v, &pick, Locale::En);
        });
        assert!(
            has(&failed, "put.io does not let this app do that."),
            "{failed:?}"
        );
    }

    #[test]
    fn long_names_and_every_language_stay_inside_the_card() {
        let long = "An.Extremely.Long.Friend.Name.".repeat(6);
        let mut v = with_friends();
        v.shares = Fetch::Ready(vec![Share {
            share_id: 1,
            user_name: long.clone(),
        }]);
        v.friends = Fetch::Ready(vec![friend(1, &long), friend(2, "john")]);
        for locale in [Locale::En, Locale::De, Locale::Es] {
            let mut used = egui::Rect::NOTHING;
            let boxes = text_boxes(|ui| {
                ui.allocate_ui(egui::Vec2::new(WIDTH, 800.0), |ui| {
                    body(ui, &v, &mut Pick::default(), locale);
                    used = ui.min_rect();
                });
            });
            assert!(used.width() <= WIDTH + 0.5, "{locale:?}: {used:?}");
            for text in &boxes {
                assert!(
                    text.full.right() <= used.left() + WIDTH + 0.5,
                    "{locale:?}: {:?} sticks out ({:?})",
                    text.text,
                    text.full
                );
            }
        }
    }

    #[test]
    fn the_stop_button_and_the_name_do_not_overlap_in_any_language() {
        let mut v = with_friends();
        v.shares = Fetch::Ready(vec![Share {
            share_id: 1,
            user_name: "A.Rather.Long.User.Name.Here".into(),
        }]);
        for locale in [Locale::En, Locale::De, Locale::Es] {
            let boxes = text_boxes(|ui| {
                ui.allocate_ui(egui::Vec2::new(WIDTH, 400.0), |ui| {
                    body(ui, &v, &mut Pick::default(), locale);
                });
            });
            let name = boxes
                .iter()
                .find(|b| b.text.starts_with("A.Rather"))
                .unwrap();
            let stop = boxes
                .iter()
                .find(|b| b.text == Msg::ShareStop.text(locale))
                .unwrap();
            assert!(
                name.full.right() <= stop.full.left(),
                "{locale:?}: {boxes:?}"
            );
        }
    }
}
