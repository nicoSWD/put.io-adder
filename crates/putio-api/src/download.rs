//! Downloading a release file for the self-updater: HTTPS only, from GitHub's hosts only, on every hop.
//!
//! GitHub answers a release-asset address with a redirect to `release-assets.githubusercontent.com`.
//! The download follows redirects itself so that each hop can be checked, and it carries no
//! credentials (the put.io token never goes here). This does **not** prove the bytes are genuine; it
//! only keeps the request from being sent somewhere else (plain HTTP, another host, an internal
//! address) and bounds how much is read.

use std::io::Read;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(600);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_HOPS: usize = 3;
const CHUNK_BYTES: usize = 64 * 1024;

/// GETs `url`, following at most three allowed redirects, and returns the body, which must be at
/// most `max_bytes` long. `progress` is called with the bytes received so far after every chunk.
pub fn get(
    url: &str,
    max_bytes: u64,
    user_agent: &str,
    progress: &dyn Fn(u64),
) -> Result<Vec<u8>, String> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .http_status_as_error(false)
        .max_redirects(0)
        .max_redirects_will_error(false)
        .https_only(true)
        .user_agent(user_agent)
        .build();
    fetch(
        &ureq::Agent::new_with_config(config),
        url,
        max_bytes,
        &is_github_url,
        progress,
    )
}

fn fetch(
    agent: &ureq::Agent,
    url: &str,
    max_bytes: u64,
    allowed: &dyn Fn(&str) -> bool,
    progress: &dyn Fn(u64),
) -> Result<Vec<u8>, String> {
    let mut url = url.to_owned();
    for _ in 0..=MAX_HOPS {
        if !allowed(&url) {
            return Err("Download failed: unexpected download address.".to_owned());
        }
        let mut response = agent
            .get(&url)
            .call()
            .map_err(|error| format!("Download failed: {error}"))?;
        match response.status().as_u16() {
            200 => {
                let reader = response
                    .body_mut()
                    .with_config()
                    .limit(max_bytes.saturating_add(1))
                    .reader();
                return read_counting(reader, max_bytes, progress);
            }
            301 | 302 | 303 | 307 | 308 => {
                let location = response
                    .headers()
                    .get("location")
                    .and_then(|value| value.to_str().ok())
                    .ok_or("Download failed: redirect without a location.")?;
                url = resolve(&url, location)
                    .ok_or("Download failed: unexpected download address.")?;
            }
            status => return Err(format!("Download failed: HTTP {status}.")),
        }
    }
    Err("Download failed: too many redirects.".to_owned())
}

/// Reads `reader` to the end in chunks, reporting the running total, and fails past `max_bytes`.
fn read_counting(
    mut reader: impl Read,
    max_bytes: u64,
    progress: &dyn Fn(u64),
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut chunk = vec![0_u8; CHUNK_BYTES];
    loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|error| format!("Download failed: {error}"))?;
        if read == 0 {
            return Ok(bytes);
        }
        bytes.extend_from_slice(chunk.get(..read).unwrap_or_default());
        let received = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if received > max_bytes {
            return Err("Download failed: the file is larger than expected.".to_owned());
        }
        progress(received);
    }
}

/// `location` as an absolute address: already absolute, or a path on `base`'s own origin.
fn resolve(base: &str, location: &str) -> Option<String> {
    if location.starts_with("https://") || location.starts_with("http://") {
        return Some(location.to_owned());
    }
    if location.starts_with('/') && !location.starts_with("//") {
        let (scheme, rest) = base.split_once("://")?;
        let authority = rest.split(['/', '?', '#']).next()?;
        return Some(format!("{scheme}://{authority}{location}"));
    }
    None
}

/// `https://github.com/…` or `https://*.githubusercontent.com/…`: no credentials, default port,
/// plain ASCII and nothing a command line or a log could mistake for something else.
#[must_use]
pub fn is_github_url(url: &str) -> bool {
    if url.len() > 2048 || !url.bytes().all(|b| b.is_ascii_graphic()) || url.contains('\\') {
        return false;
    }
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return false;
    }
    let host = match authority.split_once(':') {
        Some((host, "443")) => host,
        Some(_) => return false,
        None => authority,
    };
    let host = host.to_ascii_lowercase();
    let labels_ok = !host.is_empty()
        && host.split('.').all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        });
    labels_ok && (host == "github.com" || host.ends_with(".githubusercontent.com"))
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::net::TcpListener;

    use super::*;

    #[test]
    fn only_github_over_https_is_allowed() {
        for url in [
            "https://github.com/nicoSWD/put.io-adder/releases/download/v1/a.zip",
            "https://release-assets.githubusercontent.com/x?y=1",
            "https://objects.githubusercontent.com/x",
            "https://GitHub.com/x",
            "https://github.com:443/x",
        ] {
            assert!(is_github_url(url), "{url}");
        }
        for url in [
            "http://github.com/x",
            "https://github.com.evil.example/x",
            "https://evilgithub.com/x",
            "https://notgithubusercontent.com/x",
            "https://githubusercontent.com/x",
            "https://github.com@evil.example/x",
            "https://user:pw@github.com/x",
            "https://github.com:8443/x",
            "https://github.com:/x",
            "https://github.com./x",
            "https://-a.githubusercontent.com/x",
            "https://evil.example/github.com/x",
            "https://evil.example/?u=https://github.com/",
            "https://github.com/a b",
            "https://github.com/a\\b",
            "https://github.com/\u{e9}",
            "https://127.0.0.1/x",
            "ftp://github.com/x",
            "//github.com/x",
            "",
        ] {
            assert!(!is_github_url(url), "{url}");
        }
    }

    #[test]
    fn redirect_locations_resolve_on_their_own_origin_only() {
        assert_eq!(
            resolve("https://github.com/a/b", "/c/d?e=1").as_deref(),
            Some("https://github.com/c/d?e=1")
        );
        assert_eq!(
            resolve(
                "https://github.com/a",
                "https://objects.githubusercontent.com/x"
            )
            .as_deref(),
            Some("https://objects.githubusercontent.com/x")
        );
        // A scheme-relative or relative location is not followed.
        assert_eq!(resolve("https://github.com/a", "//evil.example/x"), None);
        assert_eq!(resolve("https://github.com/a", "x/y"), None);
        assert_eq!(resolve("https://github.com/a", "javascript:alert(1)"), None);
    }

    /// Serve each canned response once, in order, on a loopback port; returns the port.
    fn serve(responses: Vec<Vec<u8>>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for response in responses {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut request = [0_u8; 2048];
                let _ = std::io::Read::read(&mut stream, &mut request);
                let _ = stream.write_all(&response);
            }
        });
        port
    }

    fn ok(body: &[u8]) -> Vec<u8> {
        let mut out = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        out.extend_from_slice(body);
        out
    }

    fn redirect(to: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .into_bytes()
    }

    fn local_agent() -> ureq::Agent {
        ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(5)))
                .http_status_as_error(false)
                .max_redirects(0)
                .max_redirects_will_error(false)
                .build(),
        )
    }

    fn get_local(url: &str, max: u64, progress: &dyn Fn(u64)) -> Result<Vec<u8>, String> {
        fetch(
            &local_agent(),
            url,
            max,
            &|url| url.starts_with("http://127.0.0.1:"),
            progress,
        )
    }

    #[test]
    fn a_body_is_returned_and_progress_is_reported() {
        let port = serve(vec![ok(&[7_u8; 200_000])]);
        let last = std::cell::Cell::new(0_u64);
        let body = get_local(&format!("http://127.0.0.1:{port}/a"), 1_000_000, &|n| {
            last.set(n);
        })
        .unwrap();
        assert_eq!(body.len(), 200_000);
        assert_eq!(last.get(), 200_000);
    }

    #[test]
    fn a_body_over_the_limit_is_refused() {
        let port = serve(vec![ok(&[1_u8; 5_000])]);
        let error = get_local(&format!("http://127.0.0.1:{port}/a"), 4_999, &|_| {}).unwrap_err();
        assert!(error.contains("larger than expected"), "{error}");
    }

    #[test]
    fn an_allowed_redirect_is_followed_and_a_foreign_one_is_not() {
        let target = serve(vec![ok(b"payload")]);
        let start = serve(vec![redirect(&format!("http://127.0.0.1:{target}/b"))]);
        assert_eq!(
            get_local(&format!("http://127.0.0.1:{start}/a"), 100, &|_| {}).unwrap(),
            b"payload"
        );

        let start = serve(vec![redirect("https://evil.example/b")]);
        let error = get_local(&format!("http://127.0.0.1:{start}/a"), 100, &|_| {}).unwrap_err();
        assert!(error.contains("unexpected download address"), "{error}");
    }

    #[test]
    fn redirect_loops_and_error_statuses_end_the_download() {
        let port = serve(vec![]);
        let looping = (0..8)
            .map(|_| redirect(&format!("http://127.0.0.1:{port}/a")))
            .collect();
        let loop_port = serve(looping);
        let url = format!("http://127.0.0.1:{loop_port}/a");
        // Every hop points back at the first port; none of them answer, so the loop ends in an error.
        assert!(get_local(&url, 100, &|_| {}).is_err());

        let missing = serve(vec![
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        ]);
        let error = get_local(&format!("http://127.0.0.1:{missing}/a"), 100, &|_| {}).unwrap_err();
        assert!(error.contains("HTTP 404"), "{error}");
        let no_location = serve(vec![
            b"HTTP/1.1 302 Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        ]);
        let error =
            get_local(&format!("http://127.0.0.1:{no_location}/a"), 100, &|_| {}).unwrap_err();
        assert!(error.contains("without a location"), "{error}");
    }

    #[test]
    fn the_real_entry_point_refuses_other_hosts_and_plain_http_before_connecting() {
        for url in [
            "http://github.com/x",
            "https://evil.example/x",
            "https://127.0.0.1/x",
        ] {
            let error = get(url, 10, "test", &|_| {}).unwrap_err();
            assert!(
                error.contains("unexpected download address"),
                "{url}: {error}"
            );
        }
    }
}
