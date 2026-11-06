//! Text that must not end up in a log: a password typed by the user on its way to put.io.

use std::fmt;

/// A secret string. Its `Debug` output never shows the text, so a state machine, an event or an effect
/// that carries one can be printed (in a test failure, in a bug report) without leaking it.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// The text, for the one place that sends it.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(…)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_is_never_printed() {
        let secret = Secret::new("hunter2");
        for shown in [
            format!("{secret:?}"),
            format!("{secret:#?}"),
            format!("{:?}", Some(&secret)),
        ] {
            assert!(!shown.contains("hunter2"), "{shown}");
        }
        assert_eq!(secret.expose(), "hunter2");
        assert!(!secret.is_empty() && Secret::new("").is_empty());
    }
}
