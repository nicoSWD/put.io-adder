//! A link the user pastes to have put.io fetch: a magnet link, or the address of a file on the web.
//!
//! Everything pasted is untrusted text. What is accepted is a `magnet:` link (checked by the magnet parser)
//! or an `http` / `https` address with a host, no spaces and no control characters, of a sane length. An
//! address often carries a key or a password, so only its host is ever shown or logged.

use crate::feeds::host_of;
use crate::magnet::Magnet;

/// The longest address that is accepted.
pub const MAX_LINK_CHARS: usize = 2048;

/// A link that can be given to put.io.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// What is sent to put.io.
    pub uri: String,
    /// What may be shown of it: a magnet's name, or the host of a web address.
    pub label: String,
}

/// Why a pasted text is not a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkError {
    Empty,
    TooLong,
    /// Not a magnet link and not a web address.
    NotALink,
}

/// Check a pasted text.
///
/// # Errors
/// [`LinkError`] says what is wrong with it.
pub fn parse(raw: &str) -> Result<Link, LinkError> {
    let text = raw.trim();
    if text.is_empty() {
        return Err(LinkError::Empty);
    }
    if text.chars().count() > MAX_LINK_CHARS {
        return Err(LinkError::TooLong);
    }
    if text.len() >= 7 && text.is_char_boundary(7) && text[..7].eq_ignore_ascii_case("magnet:") {
        let magnet = Magnet::parse(text).map_err(|_| LinkError::NotALink)?;
        return Ok(Link {
            uri: magnet.uri().to_owned(),
            label: magnet.display_name(),
        });
    }
    let host = host_of(text).ok_or(LinkError::NotALink)?;
    Ok(Link {
        uri: text.to_owned(),
        label: host,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn a_magnet_link_and_a_web_address_are_links() {
        let magnet = parse(&format!("  magnet:?xt=urn:btih:{HASH}&dn=Sintel \n")).unwrap();
        assert_eq!(magnet.label, "Sintel");
        assert!(magnet.uri.starts_with("magnet:?xt=urn:btih:"));
        let web = parse("https://User:Secret@Example.com:8080/files/a.mkv?key=abc").unwrap();
        assert_eq!(
            web.label, "example.com",
            "never the credentials, the path or the key"
        );
        assert_eq!(
            web.uri,
            "https://User:Secret@Example.com:8080/files/a.mkv?key=abc"
        );
        assert!(parse("http://example.com/a.torrent").is_ok());
    }

    #[test]
    fn everything_else_is_refused_and_says_why() {
        assert_eq!(parse(""), Err(LinkError::Empty));
        assert_eq!(parse("   \n"), Err(LinkError::Empty));
        for bad in [
            "example.com/a.mkv",
            "ftp://example.com/a.mkv",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://",
            "https:///path",
            "http://exa mple.com/a",
            "https://example.com/a\nb",
            "https://example.com/\u{7}",
            "magnet:?dn=no-hash",
            "magnet:",
            "just some words",
        ] {
            assert_eq!(parse(bad), Err(LinkError::NotALink), "{bad:?}");
        }
        let long = format!("https://example.com/{}", "a".repeat(MAX_LINK_CHARS));
        assert_eq!(parse(&long), Err(LinkError::TooLong));
    }
}
