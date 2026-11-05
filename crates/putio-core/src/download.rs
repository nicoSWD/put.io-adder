//! Which addresses the app may hand to the browser to download a file.
//!
//! put.io answers "where can I download this?" with a signed, IP-bound URL. That text comes from the
//! network, and it ends up on a command line (`open`, `xdg-open`, `explorer.exe`), so it is checked
//! first: plain `https`, no credentials in it, and a host that is exactly `put.io` or one of its
//! sub-domains. Anything else is refused rather than opened.
//!
//! The rule is deliberately narrow. If put.io ever serves downloads from another domain the download
//! fails with a clear message, instead of the app opening whatever a response says.

/// The longest address accepted (signed URLs are a few hundred characters).
pub const MAX_URL_LEN: usize = 4096;
const SCHEME: &str = "https://";
const DOMAIN: &str = "put.io";

/// Whether `url` may be opened as a download.
#[must_use]
pub fn is_download_url(url: &str) -> bool {
    if url.len() > MAX_URL_LEN || !url.bytes().all(|b| b.is_ascii_graphic()) {
        return false;
    }
    if url.bytes().any(|b| {
        matches!(
            b,
            b'\\' | b'"' | b'\'' | b'<' | b'>' | b'`' | b'^' | b'|' | b'{' | b'}'
        )
    }) {
        return false;
    }
    let Some(rest) = url.strip_prefix(SCHEME).or_else(|| {
        // Scheme names are case-insensitive.
        url.get(..SCHEME.len())
            .filter(|head| head.eq_ignore_ascii_case(SCHEME))
            .and(url.get(SCHEME.len()..))
    }) else {
        return false;
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let Some(authority) = rest.get(..authority_end) else {
        return false;
    };
    // `user:pass@host` and `put.io@evil.example` both contain an `@`.
    if authority.contains('@') {
        return false;
    }
    let host = match authority.split_once(':') {
        Some((host, "443")) => host,
        Some(_) => return false,
        None => authority,
    };
    is_put_io_host(host)
}

fn is_put_io_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    let well_formed = !host.is_empty()
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        });
    well_formed && (host == DOMAIN || host.ends_with(".put.io"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_io_and_its_subdomains_are_accepted() {
        for url in [
            "https://put.io/files/1",
            "https://s123.put.io/v2/files/9/download?token=abc&x=1%2F2",
            "https://a.b.put.io:443/x",
            "HTTPS://PUT.IO/x",
            "https://s1.PUT.io/x",
            "https://put.io",
            "https://put.io?x=1",
            "https://put.io#frag",
        ] {
            assert!(is_download_url(url), "{url}");
        }
    }

    #[test]
    fn other_hosts_are_refused() {
        for url in [
            "https://evil.example/",
            "https://evilput.io/",
            "https://put.io.evil.example/",
            "https://evil.example/.put.io",
            "https://evil.example/?u=https://put.io/",
            "https://notput.io",
            "https://xput.io",
            "https://put.io-evil.example/",
            "https://.put.io/",
            "https://a..put.io/",
            "https://-a.put.io/",
            "https://put.io./",
            "https://put.io.",
            "https://",
            "https:///x",
        ] {
            assert!(!is_download_url(url), "{url}");
        }
    }

    #[test]
    fn credentials_odd_ports_and_odd_schemes_are_refused() {
        for url in [
            "http://put.io/x",
            "ftp://put.io/x",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "//put.io/x",
            "put.io/x",
            "https://put.io@evil.example/",
            "https://user:pass@put.io/",
            "https://put.io:8443/x",
            "https://put.io:80/x",
            "https://put.io:/x",
            "https://put.io:443:443/x",
            "https://[::1]/x",
            "https:/put.io/x",
        ] {
            assert!(!is_download_url(url), "{url}");
        }
    }

    #[test]
    fn whitespace_control_characters_and_shell_syntax_are_refused() {
        for url in [
            "https://put.io/a b",
            "https://put.io/a\tb",
            "https://put.io/a\nb",
            "https://put.io/a\u{0}b",
            "https://put.io/\u{1b}[0m",
            "https://put.io/a\\b",
            "https://put.io\\@evil.example/",
            "https://put.io/\"x",
            "https://put.io/'x",
            "https://put.io/<x>",
            "https://put.io/`x`",
            "https://put.io/a|b",
            "https://put.io/{a}",
            "https://put.io/^a",
            " https://put.io/x",
            "https://put.io/x ",
        ] {
            assert!(!is_download_url(url), "{url:?}");
        }
    }

    #[test]
    fn non_ascii_hosts_and_paths_are_refused() {
        for url in [
            "https://pu\u{74}.io/\u{e9}",
            "https://put.io/日本",
            "https://рut.io/",
            "https://put.io\u{ff0f}evil.example/",
            "https://ｐｕｔ.io/",
            "https://put。io/",
        ] {
            assert!(!is_download_url(url), "{url:?}");
        }
    }

    #[test]
    fn very_long_addresses_are_refused() {
        let ok = format!("https://put.io/{}", "a".repeat(MAX_URL_LEN - 15));
        assert_eq!(ok.len(), MAX_URL_LEN);
        assert!(is_download_url(&ok));
        assert!(!is_download_url(&format!("{ok}a")));
    }

    #[test]
    fn nothing_panics_on_arbitrary_input() {
        for url in [
            "",
            "h",
            "https:",
            "https:/",
            "https://:",
            "https://:443",
            "https://@",
            "\u{10ffff}",
        ] {
            assert!(!is_download_url(url), "{url:?}");
        }
    }
}
