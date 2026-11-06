//! The window's size: what it is in each mode, how it eases between them, how the user may resize it,
//! and when a new size is worth remembering.
//!
//! Everything here is a pure function of the mode, the size the OS reports and the time, so it is tested
//! without a window. `app.rs` feeds it once per frame and sends out the commands it returns.

use std::time::{Duration, Instant};

use eframe::egui::{Vec2, ViewportCommand, vec2};
use putio_core::settings::{Settings, WindowSize};

use crate::ui::{STRIP_HEIGHT, TOP_HEIGHT, WIDTH};

/// Empty space under the status row when the list is closed, so the toggle does not sit on the edge.
pub(crate) const COLLAPSED_MARGIN: f32 = 14.0;
pub(crate) const COLLAPSED_HEIGHT: f32 = TOP_HEIGHT + STRIP_HEIGHT + COLLAPSED_MARGIN;
/// The sign-in panel needs room for the code.
pub(crate) const LOGIN_HEIGHT: f32 = 600.0;
/// Tall enough for every section of Preferences without scrolling.
pub(crate) const PREFS_HEIGHT: f32 = 760.0;
/// The height of the open list until the user picks another.
pub(crate) const DEFAULT_LIST_HEIGHT: f32 = 600.0;

/// How long the window takes to grow or shrink.
const ANIMATION: Duration = Duration::from_millis(240);
/// A user's resize must settle this long before it is written to the preferences.
const SAVE_DELAY: Duration = Duration::from_millis(800);
/// After an animation the OS may report a size a pixel or so off, or a frame late: not a resize.
const SETTLE: Duration = Duration::from_millis(350);
/// Sizes this close are the same size.
const TOLERANCE: f32 = 1.5;

const MIN_WIDTH: f32 = WIDTH;
const MAX_WIDTH: f32 = 1400.0;
const MIN_LIST_HEIGHT: f32 = 360.0;
const MAX_HEIGHT: f32 = 3000.0;

/// What the window is showing, which decides how tall it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Only the header: a fixed height.
    Collapsed,
    /// The sign-in panel: a fixed height.
    Login,
    /// Preferences: a fixed height.
    Prefs,
    /// The "..." menu is open over the collapsed window, which is too short to hold it. The flag says
    /// whether the menu has the new-version row.
    Menu { update_row: bool },
    /// The transfers list: the user's height, which they can change.
    List,
}

impl Mode {
    fn height(self, list_height: f32) -> f32 {
        match self {
            Self::Collapsed => COLLAPSED_HEIGHT,
            Self::Login => LOGIN_HEIGHT,
            Self::Prefs => PREFS_HEIGHT,
            Self::Menu { update_row } => crate::ui::menu::window_height(update_row),
            Self::List => list_height,
        }
    }

    /// The smallest and largest window this mode allows. Only the list can be resized in height.
    fn limits(self, list_height: f32) -> (Vec2, Vec2) {
        let (min_height, max_height) = match self {
            Self::List => (MIN_LIST_HEIGHT, MAX_HEIGHT),
            fixed => {
                let height = fixed.height(list_height);
                (height, height)
            }
        };
        (vec2(MIN_WIDTH, min_height), vec2(MAX_WIDTH, max_height))
    }
}

/// An ease-out curve: quick at first, settling gently.
fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

#[derive(Debug, Clone, Copy)]
struct Animation {
    from: f32,
    to: f32,
    started: Instant,
}

impl Animation {
    /// The height now, and whether the animation is over.
    fn at(&self, now: Instant) -> (f32, bool) {
        let elapsed = now.saturating_duration_since(self.started);
        if elapsed >= ANIMATION {
            return (self.to, true);
        }
        let t = elapsed.as_secs_f32() / ANIMATION.as_secs_f32();
        (self.from + (self.to - self.from) * ease_out(t), false)
    }
}

/// What to do after a frame.
#[derive(Debug, Default)]
pub(crate) struct Step {
    /// Window commands to send, in order.
    pub(crate) commands: Vec<ViewportCommand>,
    /// Another frame is needed straight away (an animation is running).
    pub(crate) animating: bool,
    /// A size the user settled on, to remember.
    pub(crate) save: Option<WindowSize>,
}

#[derive(Debug)]
pub(crate) struct Window {
    mode: Mode,
    /// The height of the list: what the user last chose.
    list_height: f32,
    width: f32,
    /// The height last asked for (or last seen, after the user resized).
    current: f32,
    animation: Option<Animation>,
    /// Min and max last sent, so they are only sent when they change.
    limits: Option<(Vec2, Vec2)>,
    /// Ignore the OS-reported size until then (see [`SETTLE`]).
    quiet_until: Option<Instant>,
    /// A new size and when it has been stable long enough to save.
    pending_save: Option<(WindowSize, Instant)>,
}

impl Window {
    /// The window as `main` creates it: the saved size, in the mode the app starts in.
    pub(crate) fn new(settings: &Settings, mode: Mode) -> Self {
        let saved = settings.window_size.map(WindowSize::clamped);
        let list_height = saved.map_or(DEFAULT_LIST_HEIGHT, |size| to_f32(size.height));
        let width = saved.map_or(MIN_WIDTH, |size| to_f32(size.width));
        Self {
            mode,
            list_height,
            width,
            current: mode.height(list_height),
            animation: None,
            limits: None,
            quiet_until: None,
            pending_save: None,
        }
    }

    /// The size to create the window with.
    pub(crate) fn initial_size(&self) -> Vec2 {
        vec2(self.width, self.current)
    }

    pub(crate) fn initial_limits(&self) -> (Vec2, Vec2) {
        self.mode.limits(self.list_height)
    }

    /// One frame. `actual` is the inner size the OS reports (`None` before it has said).
    pub(crate) fn step(&mut self, mode: Mode, actual: Option<Vec2>, now: Instant) -> Step {
        let mut step = Step::default();
        let mut resized_by_user = false;

        if mode != self.mode {
            self.mode = mode;
            let target = mode.height(self.list_height);
            if (target - self.current).abs() > TOLERANCE {
                self.animation = Some(Animation {
                    from: self.current,
                    to: target,
                    started: now,
                });
            } else {
                self.current = target;
            }
        }

        if let Some(animation) = self.animation {
            let (height, done) = animation.at(now);
            self.current = height;
            step.commands
                .push(ViewportCommand::InnerSize(vec2(self.width, height)));
            if done {
                self.animation = None;
                self.quiet_until = Some(now + SETTLE);
            } else {
                step.animating = true;
            }
        } else if let Some(actual) = actual {
            let settled = self.quiet_until.is_none_or(|until| now >= until);
            if settled {
                // Compared after clamping: a size the window cannot take is the size it already has.
                let width = actual.x.clamp(MIN_WIDTH, MAX_WIDTH);
                if (width - self.width).abs() > TOLERANCE {
                    self.width = width;
                    resized_by_user = true;
                }
                let height = actual.y.clamp(MIN_LIST_HEIGHT, MAX_HEIGHT);
                if self.mode == Mode::List && (height - self.current).abs() > TOLERANCE {
                    self.list_height = height;
                    self.current = height;
                    resized_by_user = true;
                }
            }
        }

        // While moving, nothing may hold the window back from where it is going.
        let wanted = if step.animating {
            (
                vec2(MIN_WIDTH, COLLAPSED_HEIGHT),
                vec2(MAX_WIDTH, MAX_HEIGHT),
            )
        } else {
            self.mode.limits(self.list_height)
        };
        if self.limits != Some(wanted) {
            self.limits = Some(wanted);
            step.commands.push(ViewportCommand::MinInnerSize(wanted.0));
            step.commands.push(ViewportCommand::MaxInnerSize(wanted.1));
        }

        if resized_by_user {
            self.pending_save = Some((self.size_to_save(), now + SAVE_DELAY));
        }
        if let Some((size, due)) = self.pending_save
            && now >= due
        {
            self.pending_save = None;
            step.save = Some(size);
        }
        step
    }

    /// How long until the next frame is needed for a pending save (so it is written even if nothing
    /// else wakes the UI).
    pub(crate) fn save_due_in(&self, now: Instant) -> Option<Duration> {
        self.pending_save
            .map(|(_, due)| due.saturating_duration_since(now))
    }

    fn size_to_save(&self) -> WindowSize {
        WindowSize {
            width: to_u32(self.width),
            height: to_u32(self.list_height),
        }
        .clamped()
    }
}

// Sizes are tiny compared with what the conversions can represent.
#[allow(clippy::cast_precision_loss)]
fn to_f32(value: u32) -> f32 {
    value as f32
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn to_u32(value: f32) -> u32 {
    value.round().max(0.0) as u32
}

#[cfg(test)]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)] // tiny test numbers
mod tests {
    use super::*;

    fn at(start: Instant, millis: u64) -> Instant {
        start + Duration::from_millis(millis)
    }

    fn window(mode: Mode) -> Window {
        Window::new(&Settings::default(), mode)
    }

    fn inner_sizes(step: &Step) -> Vec<Vec2> {
        step.commands
            .iter()
            .filter_map(|c| match c {
                ViewportCommand::InnerSize(size) => Some(*size),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_collapsed_window_keeps_a_margin_below_the_status_row() {
        assert_eq!(
            COLLAPSED_HEIGHT,
            TOP_HEIGHT + STRIP_HEIGHT + COLLAPSED_MARGIN
        );
        // Enough room that the toggle does not sit on the edge.
        const { assert!(COLLAPSED_MARGIN >= 10.0) };
        assert_eq!(Mode::Collapsed.height(500.0), COLLAPSED_HEIGHT);
    }

    #[test]
    fn the_minimum_width_is_the_designed_width() {
        assert_eq!(WIDTH, to_f32(WindowSize::MIN_WIDTH));
        assert_eq!(MAX_WIDTH, to_f32(WindowSize::MAX_WIDTH));
        assert_eq!(MIN_LIST_HEIGHT, to_f32(WindowSize::MIN_HEIGHT));
        assert_eq!(MAX_HEIGHT, to_f32(WindowSize::MAX_HEIGHT));
    }

    #[test]
    fn the_easing_starts_fast_ends_at_one_and_never_overshoots() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert!(ease_out(0.5) > 0.5, "ease-out is ahead of linear early on");
        let mut previous = 0.0;
        for i in 0..=100 {
            let value = ease_out(i as f32 / 100.0);
            assert!((0.0..=1.0).contains(&value));
            assert!(value >= previous);
            previous = value;
        }
        assert_eq!(ease_out(-3.0), 0.0);
        assert_eq!(ease_out(9.0), 1.0);
    }

    #[test]
    fn an_animation_runs_between_its_ends_and_then_stops() {
        let start = Instant::now();
        let animation = Animation {
            from: 100.0,
            to: 600.0,
            started: start,
        };
        assert_eq!(animation.at(start), (100.0, false));
        let (mid, done) = animation.at(at(start, 120));
        assert!(mid > 100.0 && mid < 600.0 && !done);
        assert_eq!(animation.at(at(start, 240)), (600.0, true));
        assert_eq!(animation.at(at(start, 5_000)), (600.0, true));
        let shrinking = Animation {
            from: 600.0,
            to: 110.0,
            started: start,
        };
        let (mid, _) = shrinking.at(at(start, 100));
        assert!(mid < 600.0 && mid > 110.0);
    }

    #[test]
    fn opening_the_list_grows_the_window_step_by_step_to_the_users_height() {
        let start = Instant::now();
        let mut w = window(Mode::Collapsed);
        assert_eq!(w.initial_size(), vec2(WIDTH, COLLAPSED_HEIGHT));
        // Nothing to do while nothing changes.
        assert_eq!(
            inner_sizes(&w.step(Mode::Collapsed, Some(vec2(WIDTH, COLLAPSED_HEIGHT)), start)),
            Vec::<Vec2>::new()
        );

        let first = w.step(
            Mode::List,
            Some(vec2(WIDTH, COLLAPSED_HEIGHT)),
            at(start, 10),
        );
        assert!(first.animating);
        let mut heights = inner_sizes(&first).iter().map(|s| s.y).collect::<Vec<_>>();
        for ms in [60, 110, 160, 210] {
            let step = w.step(Mode::List, None, at(start, 10 + ms));
            assert!(step.animating, "{ms}");
            heights.extend(inner_sizes(&step).iter().map(|s| s.y));
        }
        let last = w.step(Mode::List, None, at(start, 10 + 240));
        assert!(!last.animating);
        heights.extend(inner_sizes(&last).iter().map(|s| s.y));
        assert!(
            heights.windows(2).all(|pair| pair[0] <= pair[1]),
            "grows monotonically: {heights:?}"
        );
        assert_eq!(heights.last().copied(), Some(DEFAULT_LIST_HEIGHT));
        assert!(heights.len() >= 6, "several steps, not one jump");
        assert!(
            heights
                .iter()
                .all(|h| *h >= COLLAPSED_HEIGHT && *h <= DEFAULT_LIST_HEIGHT)
        );
    }

    #[test]
    fn closing_the_list_shrinks_it_back_to_the_collapsed_height() {
        let start = Instant::now();
        let mut w = window(Mode::List);
        w.step(
            Mode::Collapsed,
            Some(vec2(WIDTH, DEFAULT_LIST_HEIGHT)),
            start,
        );
        let end = w.step(Mode::Collapsed, None, at(start, 300));
        assert!(!end.animating);
        assert_eq!(
            inner_sizes(&end).last().map(|s| s.y),
            Some(COLLAPSED_HEIGHT)
        );
    }

    #[test]
    fn the_size_limits_never_hold_the_window_back_while_it_moves_and_apply_once_it_stops() {
        let start = Instant::now();
        let mut w = window(Mode::Collapsed);
        let limits = |step: &Step| {
            let min = step.commands.iter().find_map(|c| match c {
                ViewportCommand::MinInnerSize(v) => Some(*v),
                _ => None,
            });
            let max = step.commands.iter().find_map(|c| match c {
                ViewportCommand::MaxInnerSize(v) => Some(*v),
                _ => None,
            });
            (min, max)
        };
        // At the start the fixed collapsed height is both the minimum and the maximum.
        let (min, max) = limits(&w.step(Mode::Collapsed, None, start));
        assert_eq!(
            (min.unwrap().y, max.unwrap().y),
            (COLLAPSED_HEIGHT, COLLAPSED_HEIGHT)
        );
        // Moving: the minimum must allow the small end and the maximum the big end.
        let (min, max) = limits(&w.step(Mode::List, None, at(start, 20)));
        assert_eq!(min.unwrap().y, COLLAPSED_HEIGHT);
        assert_eq!(max.unwrap().y, MAX_HEIGHT);
        // Nothing is sent again while the limits stay as they are.
        assert_eq!(
            limits(&w.step(Mode::List, None, at(start, 60))),
            (None, None)
        );
        // Stopped: the list may be resized between its own bounds.
        let (min, max) = limits(&w.step(Mode::List, None, at(start, 400)));
        assert_eq!(
            (min.unwrap().y, max.unwrap().y),
            (MIN_LIST_HEIGHT, MAX_HEIGHT)
        );
        assert_eq!((min.unwrap().x, max.unwrap().x), (MIN_WIDTH, MAX_WIDTH));
    }

    #[test]
    fn the_collapsed_window_grows_to_hold_the_menu_and_shrinks_back() {
        let start = Instant::now();
        let mut w = window(Mode::Collapsed);
        for update_row in [false, true] {
            let menu = Mode::Menu { update_row };
            let held = crate::ui::menu::window_height(update_row);
            assert!(
                held > COLLAPSED_HEIGHT,
                "the menu does not fit the collapsed window"
            );
            w.step(menu, Some(vec2(WIDTH, COLLAPSED_HEIGHT)), start);
            let open = w.step(menu, None, at(start, 400));
            assert_eq!(inner_sizes(&open).last().map(|s| s.y), Some(held));
            w.step(Mode::Collapsed, Some(vec2(WIDTH, held)), at(start, 1_000));
            let closed = w.step(Mode::Collapsed, None, at(start, 1_400));
            assert_eq!(
                inner_sizes(&closed).last().map(|s| s.y),
                Some(COLLAPSED_HEIGHT)
            );
        }
        // The menu needs less room than the shortest the list can be, so opening it there changes nothing.
        assert!(crate::ui::menu::window_height(true) <= MIN_LIST_HEIGHT);
    }

    #[test]
    fn only_the_list_can_be_resized_in_height() {
        for mode in [
            Mode::Collapsed,
            Mode::Login,
            Mode::Prefs,
            Mode::Menu { update_row: true },
        ] {
            let (min, max) = mode.limits(700.0);
            assert_eq!(min.y, max.y, "{mode:?} has one height");
            assert!(max.x > min.x, "{mode:?} can still be made wider");
        }
        let (min, max) = Mode::List.limits(700.0);
        assert!(min.y < max.y);
    }

    #[test]
    fn a_resize_by_the_user_is_remembered_after_it_settles() {
        let start = Instant::now();
        let mut w = window(Mode::List);
        w.step(Mode::List, Some(vec2(WIDTH, DEFAULT_LIST_HEIGHT)), start);
        // The user drags: the size changes frame after frame, and nothing is saved while it moves.
        for (i, (width, height)) in [(600.0, 650.0), (640.0, 700.0), (700.0, 740.0)]
            .iter()
            .enumerate()
        {
            let step = w.step(
                Mode::List,
                Some(vec2(*width, *height)),
                at(start, 100 + 100 * i as u64),
            );
            assert!(
                step.save.is_none()
                    && step
                        .commands
                        .iter()
                        .all(|c| !matches!(c, ViewportCommand::InnerSize(_)))
            );
        }
        assert!(w.save_due_in(at(start, 300)).is_some());
        // Not yet.
        assert!(
            w.step(Mode::List, Some(vec2(700.0, 740.0)), at(start, 900))
                .save
                .is_none()
        );
        // Settled.
        let saved = w
            .step(Mode::List, Some(vec2(700.0, 740.0)), at(start, 1_200))
            .save;
        assert_eq!(
            saved,
            Some(WindowSize {
                width: 700,
                height: 740
            })
        );
        // Said once.
        assert!(
            w.step(Mode::List, Some(vec2(700.0, 740.0)), at(start, 1_300))
                .save
                .is_none()
        );
        assert_eq!(w.save_due_in(at(start, 1_300)), None);
    }

    #[test]
    fn the_height_the_user_chose_comes_back_after_collapsing_and_after_preferences() {
        let start = Instant::now();
        let mut w = window(Mode::List);
        w.step(Mode::List, Some(vec2(WIDTH, 700.0)), start);
        // A drag to 700.
        w.step(Mode::List, Some(vec2(WIDTH, 700.0)), at(start, 50));
        for (mode, fixed) in [
            (Mode::Collapsed, COLLAPSED_HEIGHT),
            (Mode::Prefs, PREFS_HEIGHT),
        ] {
            let t = at(start, 1_000);
            w.step(mode, Some(vec2(WIDTH, 700.0)), t);
            let done = w.step(mode, None, at(start, 1_400));
            assert_eq!(inner_sizes(&done).last().map(|s| s.y), Some(fixed));
            w.step(Mode::List, Some(vec2(WIDTH, fixed)), at(start, 1_500));
            let back = w.step(Mode::List, None, at(start, 1_800));
            assert_eq!(
                inner_sizes(&back).last().map(|s| s.y),
                Some(700.0),
                "{mode:?}"
            );
            // (Pretend the OS has caught up before the next round.)
            w.step(Mode::List, Some(vec2(WIDTH, 700.0)), at(start, 2_400));
            // Let the clock move on for the next round.
            let _ = start;
        }
    }

    #[test]
    fn the_os_catching_up_after_an_animation_is_not_taken_for_a_resize() {
        let start = Instant::now();
        let mut w = window(Mode::Collapsed);
        w.step(Mode::List, Some(vec2(WIDTH, COLLAPSED_HEIGHT)), start);
        let done = w.step(Mode::List, None, at(start, 300));
        assert!(!done.animating);
        // The OS still reports the old, small height for a frame or two.
        for ms in [310, 400, 500] {
            let step = w.step(
                Mode::List,
                Some(vec2(WIDTH, COLLAPSED_HEIGHT)),
                at(start, ms),
            );
            assert!(step.save.is_none(), "{ms}");
        }
        assert!(
            w.save_due_in(at(start, 500)).is_none(),
            "no save was scheduled"
        );
        // And the remembered height is still the list's own.
        assert_eq!(
            w.size_to_save().height,
            WindowSize::MIN_HEIGHT.max(DEFAULT_LIST_HEIGHT as u32)
        );
    }

    #[test]
    fn sizes_outside_the_supported_range_are_clamped() {
        let start = Instant::now();
        let mut w = window(Mode::List);
        w.step(Mode::List, Some(vec2(WIDTH, DEFAULT_LIST_HEIGHT)), start);
        w.step(Mode::List, Some(vec2(50.0, 50.0)), at(start, 100));
        let saved = w
            .step(Mode::List, Some(vec2(50.0, 50.0)), at(start, 1_000))
            .save
            .unwrap();
        assert_eq!(
            (saved.width, saved.height),
            (WindowSize::MIN_WIDTH, WindowSize::MIN_HEIGHT)
        );
        w.step(Mode::List, Some(vec2(99_999.0, 99_999.0)), at(start, 1_100));
        let saved = w
            .step(Mode::List, Some(vec2(99_999.0, 99_999.0)), at(start, 2_000))
            .save
            .unwrap();
        assert_eq!(
            (saved.width, saved.height),
            (WindowSize::MAX_WIDTH, WindowSize::MAX_HEIGHT)
        );
    }

    #[test]
    fn a_saved_size_is_used_at_start_and_a_hostile_one_is_not_trusted() {
        let settings = Settings {
            window_size: Some(WindowSize {
                width: 800,
                height: 900,
            }),
            ..Settings::default()
        };
        assert_eq!(
            Window::new(&settings, Mode::List).initial_size(),
            vec2(800.0, 900.0)
        );
        // Collapsed keeps the saved width but its own height.
        assert_eq!(
            Window::new(&settings, Mode::Collapsed).initial_size(),
            vec2(800.0, COLLAPSED_HEIGHT)
        );
        let hostile = Settings {
            window_size: Some(WindowSize {
                width: 0,
                height: u32::MAX,
            }),
            ..Settings::default()
        };
        let size = Window::new(&hostile, Mode::List).initial_size();
        assert_eq!(size, vec2(MIN_WIDTH, MAX_HEIGHT));
        assert_eq!(
            Window::new(&Settings::default(), Mode::List).initial_size(),
            vec2(WIDTH, DEFAULT_LIST_HEIGHT)
        );
    }

    #[test]
    fn a_width_change_alone_is_remembered_too() {
        let start = Instant::now();
        let mut w = window(Mode::Collapsed);
        w.step(Mode::Collapsed, Some(vec2(WIDTH, COLLAPSED_HEIGHT)), start);
        w.step(
            Mode::Collapsed,
            Some(vec2(900.0, COLLAPSED_HEIGHT)),
            at(start, 100),
        );
        let saved = w
            .step(
                Mode::Collapsed,
                Some(vec2(900.0, COLLAPSED_HEIGHT)),
                at(start, 1_000),
            )
            .save;
        assert_eq!(saved.map(|s| s.width), Some(900));
        // The list height is kept as it was.
        assert_eq!(saved.map(|s| s.height), Some(DEFAULT_LIST_HEIGHT as u32));
    }
}
