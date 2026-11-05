//! Validation of `magnet:` URIs received from the OS.
//!
//! Magnet links arrive from browsers, chat apps and other programs: they are untrusted. A link is
//! only accepted when it is bounded, free of control characters and whitespace, and names at least
//! one well-formed `BitTorrent` info-hash (`xt=urn:btih:` or `xt=urn:btmh:`).

/// Longest accepted URI (real links are a few hundred bytes; trackers can push them to a few KiB).
pub const MAX_LEN: usize = 8192;
const MAX_DISPLAY_NAME: usize = 120;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Magnet {
    /// The validated URI exactly as it will be sent to put.io.
    uri: String,
    display_name: Option<String>,
    info_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MagnetError {
    #[error("not a magnet link")]
    NotMagnet,
    #[error("magnet link is too long")]
    TooLong,
    #[error("magnet link contains control characters or whitespace")]
    BadCharacters,
    #[error("magnet link has bad percent-encoding")]
    BadEncoding,
    #[error("magnet link has no valid info-hash")]
    NoInfoHash,
}

impl Magnet {
    pub fn parse(input: &str) -> Result<Self, MagnetError> {
        let uri = input.trim();
        if uri.len() > MAX_LEN {
            return Err(MagnetError::TooLong);
        }
        let query = strip_scheme(uri).ok_or(MagnetError::NotMagnet)?;
        if uri.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(MagnetError::BadCharacters);
        }

        let mut info_hash = None;
        let mut display_name = None;
        for pair in query.split('&').filter(|pair| !pair.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            match key.to_ascii_lowercase().as_str() {
                "xt" => {
                    let value = decode(value)?;
                    if info_hash.is_none() {
                        info_hash = valid_hash(&value);
                    }
                }
                "dn" if display_name.is_none() => display_name = clean_name(&decode(value)?),
                _ => {}
            }
        }
        let info_hash = info_hash.ok_or(MagnetError::NoInfoHash)?;
        Ok(Self {
            uri: uri.to_owned(),
            display_name,
            info_hash,
        })
    }

    /// The URI to send to put.io.
    #[must_use]
    pub fn uri(&self) -> &str {
        &self.uri
    }

    /// Name for the status line: the `dn` parameter, or a short form of the info-hash.
    #[must_use]
    pub fn display_name(&self) -> String {
        match &self.display_name {
            Some(name) => name.clone(),
            None => format!(
                "magnet:{}…",
                self.info_hash.chars().take(8).collect::<String>()
            ),
        }
    }
}

/// The part after `magnet:?` (scheme matched case-insensitively), or `None`.
fn strip_scheme(uri: &str) -> Option<&str> {
    let (scheme, rest) = uri.split_at_checked("magnet:".len())?;
    if !scheme.eq_ignore_ascii_case("magnet:") {
        return None;
    }
    rest.strip_prefix('?')
}

/// Percent-decode; `+` is a space (as browsers encode `dn`). Rejects truncated or non-hex escapes
/// and anything that does not decode to UTF-8.
fn decode(value: &str) -> Result<String, MagnetError> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        match byte {
            b'%' => {
                let hex = bytes.get(i + 1..i + 3).ok_or(MagnetError::BadEncoding)?;
                let hex = std::str::from_utf8(hex).map_err(|_| MagnetError::BadEncoding)?;
                out.push(u8::from_str_radix(hex, 16).map_err(|_| MagnetError::BadEncoding)?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            _ => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| MagnetError::BadEncoding)
}

/// `urn:btih:` + 40 hex or 32 base32, or `urn:btmh:` + `1220` + 64 hex (SHA-256 multihash).
fn valid_hash(xt: &str) -> Option<String> {
    let lower = xt.to_ascii_lowercase();
    if let Some(hash) = lower.strip_prefix("urn:btih:") {
        let hex = hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit());
        let base32 =
            hash.len() == 32 && hash.bytes().all(|b| matches!(b, b'a'..=b'z' | b'2'..=b'7'));
        return (hex || base32).then(|| hash.to_owned());
    }
    let hash = lower.strip_prefix("urn:btmh:1220")?;
    (hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())).then(|| hash.to_owned())
}

/// A display name safe to put in the UI: no control characters, trimmed, bounded.
fn clean_name(name: &str) -> Option<String> {
    let cleaned: String = name.chars().filter(|c| !c.is_control()).collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return None;
    }
    let mut out: String = cleaned.chars().take(MAX_DISPLAY_NAME).collect();
    if cleaned.chars().count() > MAX_DISPLAY_NAME {
        out.push('…');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

    fn link(rest: &str) -> String {
        format!("magnet:?{rest}")
    }

    #[test]
    fn accepts_a_typical_link() {
        let magnet = Magnet::parse(&link(&format!(
            "xt=urn:btih:{HASH}&dn=Big+Buck%20Bunny&tr=udp%3A%2F%2Ft.example%3A80"
        )))
        .unwrap();
        assert_eq!(magnet.display_name(), "Big Buck Bunny");
        assert!(magnet.uri().starts_with("magnet:?xt=urn:btih:"));
    }

    #[test]
    fn accepts_base32_and_v2_hashes_and_any_scheme_case() {
        assert!(Magnet::parse(&link("xt=urn:btih:MFRGGZDFMZTWQ2LKNNWG23TPOBYXE43U")).is_ok());
        assert!(Magnet::parse(&format!("MAGNET:?xt=urn:btmh:1220{}", "ab".repeat(32))).is_ok());
        assert!(Magnet::parse(&link(&format!("XT=URN:BTIH:{}", HASH.to_uppercase()))).is_ok());
    }

    #[test]
    fn trims_outer_whitespace_only() {
        assert!(
            Magnet::parse(&format!("  \n{}\r\n", link(&format!("xt=urn:btih:{HASH}")))).is_ok()
        );
        assert_eq!(
            Magnet::parse(&link(&format!("xt=urn:btih:{HASH}&dn=a b"))).unwrap_err(),
            MagnetError::BadCharacters
        );
    }

    #[test]
    fn falls_back_to_a_short_hash_when_there_is_no_name() {
        assert_eq!(
            Magnet::parse(&link(&format!("xt=urn:btih:{HASH}")))
                .unwrap()
                .display_name(),
            "magnet:01234567…"
        );
        assert_eq!(
            Magnet::parse(&link(&format!("xt=urn:btih:{HASH}&dn=%00%07")))
                .unwrap()
                .display_name(),
            "magnet:01234567…"
        );
    }

    #[test]
    fn rejects_non_magnets_and_bad_hashes() {
        assert_eq!(
            Magnet::parse("https://example.com").unwrap_err(),
            MagnetError::NotMagnet
        );
        assert_eq!(
            Magnet::parse("magnet:xt=urn:btih:abc").unwrap_err(),
            MagnetError::NotMagnet
        );
        assert_eq!(Magnet::parse("").unwrap_err(), MagnetError::NotMagnet);
        assert_eq!(
            Magnet::parse("magnet:?").unwrap_err(),
            MagnetError::NoInfoHash
        );
        assert_eq!(
            Magnet::parse(&link("dn=x")).unwrap_err(),
            MagnetError::NoInfoHash
        );
        assert_eq!(
            Magnet::parse(&link("xt=urn:btih:xyz")).unwrap_err(),
            MagnetError::NoInfoHash
        );
        assert_eq!(
            Magnet::parse(&link("xt=urn:sha1:0123")).unwrap_err(),
            MagnetError::NoInfoHash
        );
        assert_eq!(
            Magnet::parse(&link(&format!("xt=urn:btih:{}", &HASH[..39]))).unwrap_err(),
            MagnetError::NoInfoHash
        );
        assert_eq!(
            Magnet::parse(&link("xt=urn:btmh:1221aa")).unwrap_err(),
            MagnetError::NoInfoHash
        );
    }

    #[test]
    fn rejects_control_characters_and_bad_encoding() {
        assert_eq!(
            Magnet::parse(&link(&format!("xt=urn:btih:{HASH}\u{0}"))).unwrap_err(),
            MagnetError::BadCharacters
        );
        assert_eq!(
            Magnet::parse(&link(&format!("xt=urn:btih:{HASH}&dn=%zz"))).unwrap_err(),
            MagnetError::BadEncoding
        );
        assert_eq!(
            Magnet::parse(&link(&format!("xt=urn:btih:{HASH}&dn=%4"))).unwrap_err(),
            MagnetError::BadEncoding
        );
        assert_eq!(
            Magnet::parse(&link(&format!("xt=urn:btih:{HASH}&dn=%ff%fe"))).unwrap_err(),
            MagnetError::BadEncoding
        );
    }

    #[test]
    fn rejects_oversized_links() {
        let long = link(&format!("xt=urn:btih:{HASH}&tr={}", "a".repeat(MAX_LEN)));
        assert_eq!(Magnet::parse(&long).unwrap_err(), MagnetError::TooLong);
    }

    #[test]
    fn display_name_is_sanitised_and_bounded() {
        let magnet = Magnet::parse(&link(&format!(
            "xt=urn:btih:{HASH}&dn=a%0Ab%1B%5B31m{}",
            "z".repeat(300)
        )))
        .unwrap();
        let name = magnet.display_name();
        assert!(!name.chars().any(char::is_control));
        assert!(name.starts_with("ab[31mzzz"));
        assert_eq!(name.chars().count(), MAX_DISPLAY_NAME + 1);
        assert!(name.ends_with('…'));
    }

    #[test]
    fn first_valid_hash_wins_even_after_an_invalid_xt() {
        assert!(Magnet::parse(&link(&format!("xt=urn:btih:nope&xt=urn:btih:{HASH}"))).is_ok());
    }

    #[test]
    fn multibyte_input_never_panics_on_scheme_split() {
        assert_eq!(
            Magnet::parse("mägnet:?xt=1").unwrap_err(),
            MagnetError::NotMagnet
        );
        assert_eq!(
            Magnet::parse("日本語日本語日本語").unwrap_err(),
            MagnetError::NotMagnet
        );
    }

    mod props {
        use proptest::prelude::*;

        use super::super::*;

        proptest! {
            #[test]
            fn parse_never_panics_on_arbitrary_input(input in ".{0,300}") {
                let _ = Magnet::parse(&input);
            }

            #[test]
            fn accepted_links_are_clean(input in "magnet:\\?xt=urn:btih:[0-9a-f]{40}(&[a-z]{1,3}=[A-Za-z0-9%+.:/_-]{0,40}){0,5}") {
                if let Ok(magnet) = Magnet::parse(&input) {
                    prop_assert!(magnet.uri().len() <= MAX_LEN);
                    prop_assert!(!magnet.uri().chars().any(|c| c.is_control() || c.is_whitespace()));
                    prop_assert!(!magnet.display_name().chars().any(char::is_control));
                }
            }
        }
    }
}
