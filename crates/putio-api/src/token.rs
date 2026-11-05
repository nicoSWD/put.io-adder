//! The OAuth access token.

use std::fmt;

use zeroize::Zeroizing;

/// Longest token accepted. put.io tokens are ~20 characters; this only bounds hostile input.
const MAX_LEN: usize = 512;

/// An OAuth access token.
///
/// The value is wiped from memory on drop, `Debug` never prints it, and there is deliberately no
/// `Display` or `Serialize` implementation: the only way out is [`Token::expose`].
#[derive(Clone)]
pub struct Token(Zeroizing<String>);

/// Why a string was rejected as a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidToken {
    #[error("token is empty")]
    Empty,
    #[error("token is too long")]
    TooLong,
    #[error("token contains characters that are not allowed in an HTTP header")]
    BadCharacters,
}

impl Token {
    /// Validate and wrap a token. Only visible ASCII is accepted, which also rules out header
    /// injection (no CR, LF or spaces).
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidToken> {
        let value = Zeroizing::new(value.into());
        if value.is_empty() {
            return Err(InvalidToken::Empty);
        }
        if value.len() > MAX_LEN {
            return Err(InvalidToken::TooLong);
        }
        if !value.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(InvalidToken::BadCharacters);
        }
        Ok(Self(value))
    }

    /// The raw token. Handle with care: never log it or put it in a URL or argv.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

impl PartialEq for Token {
    /// Constant-time-ish comparison is not needed (tokens are never compared against attacker input),
    /// but equality is useful in tests and for "token changed" checks.
    fn eq(&self, other: &Self) -> bool {
        self.0.as_str() == other.0.as_str()
    }
}

impl Eq for Token {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_tokens() {
        assert!(Token::new("ABCDEF123456").is_ok());
    }

    #[test]
    fn rejects_empty_long_and_unsafe() {
        assert_eq!(Token::new("").unwrap_err(), InvalidToken::Empty);
        assert_eq!(
            Token::new("a".repeat(513)).unwrap_err(),
            InvalidToken::TooLong
        );
        for bad in ["a b", "a\nb", "a\r\nX: y", "tök", "a\u{0}b", "a\tb"] {
            assert_eq!(
                Token::new(bad).unwrap_err(),
                InvalidToken::BadCharacters,
                "{bad:?}"
            );
        }
    }

    #[test]
    fn debug_never_prints_the_secret() {
        let token = Token::new("super-secret-value").unwrap();
        assert!(!format!("{token:?}").contains("super-secret"));
        assert_eq!(token.expose(), "super-secret-value");
    }
}
