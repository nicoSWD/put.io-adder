//! Whether put.io adder is the app the system opens magnet links and `.torrent` files with.
//!
//! Pure types and decisions only: asking the system (and changing its answer) is the platform
//! crate's job, because it differs per OS and talks to other processes.

/// Which kinds of link this app is currently the default for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    /// `magnet:` links.
    pub magnet: bool,
    /// `.torrent` files.
    pub torrent: bool,
}

impl Status {
    /// The app is the default for both.
    #[must_use]
    pub fn is_default(self) -> bool {
        self.magnet && self.torrent
    }
}

/// What looking at the system found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// This copy cannot be the default (a development build, an app not in a bundle, no `xdg-mime`).
    /// The text says why. Nothing is asked and nothing is offered.
    Unavailable(String),
    Known(Status),
}

/// Whether to ask the user now: the app is not the default yet, they have not said "don't ask
/// again", and they have not already been asked in this run.
#[must_use]
pub fn should_ask(check: &Check, ask_setting: bool, asked_this_run: bool) -> bool {
    ask_setting && !asked_this_run && matches!(check, Check::Known(status) if !status.is_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Status = Status {
        magnet: false,
        torrent: false,
    };
    const HALF: Status = Status {
        magnet: true,
        torrent: false,
    };
    const BOTH: Status = Status {
        magnet: true,
        torrent: true,
    };

    #[test]
    fn only_being_the_default_for_both_counts() {
        assert!(!NONE.is_default());
        assert!(!HALF.is_default());
        assert!(
            !Status {
                magnet: false,
                torrent: true
            }
            .is_default()
        );
        assert!(BOTH.is_default());
    }

    #[test]
    fn the_user_is_asked_when_the_app_is_not_yet_the_default() {
        assert!(should_ask(&Check::Known(NONE), true, false));
        assert!(
            should_ask(&Check::Known(HALF), true, false),
            "half is not enough"
        );
    }

    #[test]
    fn nobody_is_asked_when_it_is_already_done_declined_or_impossible() {
        assert!(
            !should_ask(&Check::Known(BOTH), true, false),
            "already the default"
        );
        assert!(
            !should_ask(&Check::Known(NONE), false, false),
            "said don't ask again"
        );
        assert!(
            !should_ask(&Check::Known(NONE), true, true),
            "already asked in this run"
        );
        assert!(!should_ask(
            &Check::Unavailable("a development build".into()),
            true,
            false
        ));
    }
}
