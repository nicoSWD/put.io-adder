//! The friends card: ask somebody to be a friend, answer the requests waiting, and see (and end) the
//! friendships there are.
//!
//! A modal like the other cards. These calls reach other people, so each is a button press: ending a
//! friendship asks first (put.io then takes the files shared with all friends out of that friend's folder),
//! and the card cannot be closed while a request is on its way. What put.io answers is shown as it is.

use eframe::egui::{self, Align, Id, Key, Layout, RichText};
use putio_api::{Friend, FriendChange};
use putio_core::i18n::{Locale, Msg};
use putio_core::state::{Event, Fetch, FriendAction, FriendsView, Problem};

use super::{prefs, theme, widgets};
use crate::app::App;

/// Width of the content inside the card (the window is 560 wide).
const WIDTH: f32 = 420.0;
/// Space the card needs besides the scrolling body: padding and header.
const CHROME_HEIGHT: f32 = 190.0;
/// How many characters a user name may have in the field.
const MAX_NAME_CHARS: usize = 64;
/// Height of a row of a friend.
const ROW_HEIGHT: f32 = 40.0;

/// What the user has typed and which removal is being asked about.
#[derive(Debug, Default)]
pub(crate) struct FriendsUi {
    name: String,
    /// The friend whose removal is being confirmed.
    confirm: Option<String>,
    /// The name field takes the keyboard when the card opens.
    focus: bool,
    /// The field that has the keyboard, so Escape can leave it before it closes the card.
    typing: Option<Id>,
    /// The field was emptied after the last request went through.
    cleared: bool,
}

/// Open the card.
pub(crate) fn open(app: &mut App, ctx: &egui::Context) {
    app.ui.friends_ui = FriendsUi {
        focus: true,
        ..FriendsUi::default()
    };
    app.dispatch(ctx, Event::FriendsCardOpened);
}

pub(crate) fn show(ctx: &egui::Context, app: &mut App) {
    let Some(view) = app.state.friends_card.clone() else {
        return;
    };
    let locale = app.state.locale;
    let busy = view.action == FriendAction::Working;
    let mut fui = std::mem::take(&mut app.ui.friends_ui);
    // A request that went through leaves the field empty for the next one.
    if matches!(view.action, FriendAction::Done(FriendChange::Request(_))) && !fui.cleared {
        fui.name.clear();
        fui.cleared = true;
    }
    let (frame, backdrop) = widgets::dialog_frame();
    let mut changes = Vec::new();
    let response = egui::Modal::new(Id::unique("friends"))
        .backdrop_color(backdrop)
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(WIDTH);
            let (_, close) = prefs::header(ui, &Msg::FriendsTitle.text(locale), false, locale);
            let height = (ui.ctx().content_rect().height() - CHROME_HEIGHT).max(140.0);
            prefs::scroll_region(ui, height, |ui| {
                changes = body(ui, &view, &mut fui, locale);
            });
            close
        });
    for change in changes {
        if matches!(change, FriendChange::Request(_)) {
            fui.cleared = false;
        }
        app.dispatch(ctx, Event::FriendChangeRequested(change));
    }
    // A click on the backdrop does not count: something may be typed.
    let escape = response.should_close() && !response.backdrop_response.clicked();
    let close = response.inner || escape;
    if close && !busy {
        if escape && let Some(field) = fui.typing.take() {
            // The first Escape leaves the field it was pressed in.
            ctx.memory_mut(|memory| memory.surrender_focus(field));
        } else {
            app.dispatch(ctx, Event::FriendsCardClosed);
            return;
        }
    }
    app.ui.friends_ui = fui;
}

/// Debug builds only: the card (`friends`, `friends-empty`, `friends-denied`, `friends-confirm`) for a
/// preview.
#[cfg(debug_assertions)]
pub(crate) fn demo(app: &mut App, ctx: &egui::Context, view: &str) {
    use putio_api::ApiError;
    let friend = |id: i64, name: &str| Friend {
        id,
        name: name.into(),
    };
    let (friends, requests) = match view {
        "friends" | "friends-confirm" => (
            Ok(vec![
                friend(1, "jane"),
                friend(2, "john"),
                friend(3, "alex"),
            ]),
            Ok(vec![friend(4, "sam"), friend(5, "kim")]),
        ),
        "friends-empty" => (Ok(vec![]), Ok(vec![])),
        "friends-denied" => (Err(ApiError::Unauthorized), Err(ApiError::Unauthorized)),
        _ => return,
    };
    open(app, ctx);
    app.dispatch(ctx, Event::FriendListFetched(friends));
    app.dispatch(ctx, Event::FriendRequestsFetched(requests));
    if view == "friends-confirm" {
        app.ui.friends_ui.confirm = Some("john".into());
    }
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

/// Everything in the card. Returns what the buttons asked for.
fn body(
    ui: &mut egui::Ui,
    view: &FriendsView,
    fui: &mut FriendsUi,
    locale: Locale,
) -> Vec<FriendChange> {
    ui.spacing_mut().item_spacing.y = 8.0;
    let busy = view.action == FriendAction::Working;
    let mut changes = Vec::new();

    widgets::section_title(ui, &Msg::FriendsAddTitle.text(locale));
    add_row(ui, fui, busy, locale, &mut changes);
    outcome(ui, &view.action, locale);

    match &view.requests {
        Fetch::Loading => {}
        Fetch::Failed(problem) => {
            widgets::section_title(ui, &Msg::FriendsRequestsTitle.text(locale));
            muted(ui, problem_text(problem, locale));
        }
        Fetch::Ready(list) if list.is_empty() => {}
        Fetch::Ready(list) => {
            widgets::section_title(ui, &Msg::FriendsRequestsTitle.text(locale));
            widgets::group(ui, |ui| {
                for (index, person) in list.iter().enumerate() {
                    if index > 0 {
                        widgets::divider(ui);
                    }
                    request_row(ui, person, busy, locale, &mut changes);
                }
            });
        }
    }

    widgets::section_title(ui, &Msg::FriendsListTitle.text(locale));
    match &view.friends {
        Fetch::Loading => muted(ui, Msg::FilesLoading.text(locale)),
        Fetch::Failed(problem) => muted(ui, problem_text(problem, locale)),
        Fetch::Ready(list) if list.is_empty() => muted(ui, Msg::FriendsNone.text(locale)),
        Fetch::Ready(list) => widgets::group(ui, |ui| {
            for (index, person) in list.iter().enumerate() {
                if index > 0 {
                    widgets::divider(ui);
                }
                friend_row(ui, person, fui, busy, locale, &mut changes);
            }
        }),
    }
    ui.add_space(4.0);
    changes
}

/// The field for a user name and the button that sends the request.
fn add_row(
    ui: &mut egui::Ui,
    fui: &mut FriendsUi,
    busy: bool,
    locale: Locale,
    changes: &mut Vec<FriendChange>,
) {
    let mut submit = false;
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // The button sits at the right end; the field takes what is left.
            if widgets::text_button(ui, &Msg::FriendsSendRequest.text(locale), true).clicked() {
                submit = true;
            }
            let field = ui
                .add_enabled_ui(!busy, |ui| {
                    widgets::text_field(
                        ui,
                        &mut fui.name,
                        &Msg::FriendsNameHint.text(locale),
                        None,
                        MAX_NAME_CHARS,
                    )
                })
                .inner;
            if fui.focus {
                field.request_focus();
                fui.focus = false;
            }
            fui.typing = field.has_focus().then_some(field.id);
            submit |= field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
        });
    });
    if submit && !busy && !fui.name.trim().is_empty() {
        changes.push(FriendChange::Request(fui.name.clone()));
    }
}

/// How the last change went: said in the user's words, until the next one.
fn outcome(ui: &mut egui::Ui, action: &FriendAction, locale: Locale) {
    match action {
        FriendAction::Idle | FriendAction::Working => {}
        FriendAction::Done(change) => muted(
            ui,
            match change {
                FriendChange::Request(_) => Msg::FriendsDoneRequest,
                FriendChange::Approve(_) => Msg::FriendsDoneApprove,
                FriendChange::Deny(_) => Msg::FriendsDoneDeny,
                FriendChange::Unfriend(_) => Msg::FriendsDoneUnfriend,
            }
            .text(locale),
        ),
        FriendAction::Failed(problem) => {
            ui.add(
                egui::Label::new(
                    RichText::new(problem_text(problem, locale))
                        .font(theme::text(11.0))
                        .color(theme::pal().error),
                )
                .wrap(),
            );
        }
    }
}

/// A name on the left, cut to what the buttons leave; the buttons on the right.
fn person_row(ui: &mut egui::Ui, name: &str, buttons: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(ROW_HEIGHT);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            buttons(ui);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add(egui::Label::new(RichText::new(name).font(theme::bold(12.5))).truncate());
            });
        });
    });
}

fn request_row(
    ui: &mut egui::Ui,
    person: &Friend,
    busy: bool,
    locale: Locale,
    changes: &mut Vec<FriendChange>,
) {
    person_row(ui, &person.name, |ui| {
        // Right to left: the one that matters most is the one nearest the edge.
        if widgets::text_button(ui, &Msg::FriendsAccept.text(locale), true).clicked() && !busy {
            changes.push(FriendChange::Approve(person.name.clone()));
        }
        if widgets::text_button(ui, &Msg::FriendsDecline.text(locale), false).clicked() && !busy {
            changes.push(FriendChange::Deny(person.name.clone()));
        }
    });
}

fn friend_row(
    ui: &mut egui::Ui,
    person: &Friend,
    fui: &mut FriendsUi,
    busy: bool,
    locale: Locale,
    changes: &mut Vec<FriendChange>,
) {
    if fui.confirm.as_deref() == Some(person.name.as_str()) {
        // The question comes in place of the row, so nothing else moves.
        ui.add_space(8.0);
        ui.add(
            egui::Label::new(
                RichText::new(
                    Msg::FriendsRemoveAsk {
                        name: person.name.clone(),
                    }
                    .text(locale),
                )
                .font(theme::text(12.0)),
            )
            .wrap(),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::danger_button(ui, &Msg::FriendsRemoveConfirm.text(locale)).clicked()
                    && !busy
                {
                    changes.push(FriendChange::Unfriend(person.name.clone()));
                    fui.confirm = None;
                }
                if widgets::text_button(ui, &Msg::PrefsCancel.text(locale), false).clicked() {
                    fui.confirm = None;
                }
            });
        });
        ui.add_space(8.0);
        return;
    }
    person_row(ui, &person.name, |ui| {
        if widgets::text_button(ui, &Msg::FriendsRemove.text(locale), false).clicked() && !busy {
            fui.confirm = Some(person.name.clone());
        }
    });
}

#[cfg(test)]
mod tests {
    use eframe::egui::{Rect, Vec2};
    use putio_core::i18n::Locale;

    use super::*;
    use crate::ui::test_support::{rendered_text, text_boxes};

    fn friend(id: i64, name: &str) -> Friend {
        Friend {
            id,
            name: name.into(),
        }
    }

    fn view() -> FriendsView {
        FriendsView {
            friends: Fetch::Ready(vec![friend(1, "jane"), friend(2, "john")]),
            requests: Fetch::Ready(vec![friend(4, "sam")]),
            action: FriendAction::Idle,
        }
    }

    fn has(text: &[String], wanted: &str) -> bool {
        text.iter().any(|s| s == wanted)
    }

    #[test]
    fn the_card_lists_requests_and_friends_with_their_buttons() {
        let v = view();
        let text = rendered_text(|ui| {
            body(ui, &v, &mut FriendsUi::default(), Locale::En);
        });
        for expected in [
            "ADD A FRIEND",
            "Send request",
            "REQUESTS",
            "sam",
            "Accept",
            "Decline",
            "YOUR FRIENDS",
            "jane",
            "john",
            "Remove",
        ] {
            assert!(has(&text, expected), "missing {expected:?} in {text:?}");
        }
    }

    #[test]
    fn ending_a_friendship_asks_first_and_names_the_consequence() {
        let v = view();
        let mut fui = FriendsUi {
            confirm: Some("john".into()),
            ..FriendsUi::default()
        };
        let text = rendered_text(|ui| {
            body(ui, &v, &mut fui, Locale::En);
        });
        assert!(
            has(&text, "Remove friend") && has(&text, "Cancel"),
            "{text:?}"
        );
        assert!(
            text.iter()
                .any(|t| t.starts_with("Remove john as a friend?")),
            "{text:?}"
        );
        // Only the friend asked about is asked about.
        assert!(
            !text.iter().any(|t| t.starts_with("Remove jane")),
            "{text:?}"
        );
    }

    #[test]
    fn every_state_of_a_list_has_its_words() {
        let mut v = view();
        v.friends = Fetch::Ready(vec![]);
        v.requests = Fetch::Ready(vec![]);
        let text = rendered_text(|ui| {
            body(ui, &v, &mut FriendsUi::default(), Locale::En);
        });
        assert!(has(&text, "No friends yet."), "{text:?}");
        assert!(!has(&text, "REQUESTS"), "no empty section: {text:?}");
        v.friends = Fetch::Failed(Problem::NotAllowed);
        v.requests = Fetch::Failed(Problem::Failed("Offline".into()));
        v.action = FriendAction::Failed(Problem::Failed("No such user".into()));
        let text = rendered_text(|ui| {
            body(ui, &v, &mut FriendsUi::default(), Locale::En);
        });
        for expected in [
            "put.io does not let this app do that.",
            "That did not work: Offline",
            "That did not work: No such user",
        ] {
            assert!(has(&text, expected), "missing {expected:?} in {text:?}");
        }
        v.action = FriendAction::Done(FriendChange::Unfriend("x".into()));
        let text = rendered_text(|ui| {
            body(ui, &v, &mut FriendsUi::default(), Locale::En);
        });
        assert!(has(&text, "Friend removed."), "{text:?}");
    }

    #[test]
    fn long_names_and_every_language_stay_inside_the_card() {
        let long = "An.Extremely.Long.User.Name.".repeat(6);
        let mut v = view();
        v.friends = Fetch::Ready(vec![friend(1, &long), friend(2, "john")]);
        v.requests = Fetch::Ready(vec![friend(4, &long)]);
        for locale in [Locale::En, Locale::De, Locale::Es] {
            for confirm in [None, Some(long.clone())] {
                let mut used = Rect::NOTHING;
                let mut fui = FriendsUi {
                    confirm,
                    name: "a rather long name that was typed".into(),
                    ..FriendsUi::default()
                };
                let boxes = text_boxes(|ui| {
                    ui.allocate_ui(Vec2::new(WIDTH, 900.0), |ui| {
                        body(ui, &v, &mut fui, locale);
                        used = ui.min_rect();
                    });
                });
                assert!(used.width() <= WIDTH + 0.5, "{locale:?}: {used:?}");
                for text in &boxes {
                    assert!(
                        text.visible.right() <= used.left() + WIDTH + 0.5,
                        "{locale:?}: {:?} sticks out ({:?})",
                        text.text,
                        text.visible
                    );
                }
            }
        }
    }

    #[test]
    fn a_name_never_runs_into_the_buttons_of_its_row() {
        let mut v = view();
        v.requests = Fetch::Ready(vec![friend(4, "A.Rather.Long.User.Name.Here.Indeed.Yes")]);
        for locale in [Locale::En, Locale::De, Locale::Es] {
            let boxes = text_boxes(|ui| {
                ui.allocate_ui(Vec2::new(WIDTH, 600.0), |ui| {
                    body(ui, &v, &mut FriendsUi::default(), locale);
                });
            });
            let name = boxes
                .iter()
                .find(|b| b.text.starts_with("A.Rather"))
                .unwrap();
            let decline = boxes
                .iter()
                .find(|b| b.text == Msg::FriendsDecline.text(locale))
                .unwrap();
            assert!(
                name.visible.right() <= decline.full.left(),
                "{locale:?}: {boxes:?}"
            );
        }
    }
}
