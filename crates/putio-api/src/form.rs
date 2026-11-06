//! Form and multipart encoding, written out by hand to keep the dependency tree small.

use std::fmt::Write as _;

/// Percent-encode everything except the RFC 3986 unreserved characters.
#[must_use]
pub fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// `k=v&k=v` with both sides percent-encoded.
#[must_use]
pub fn urlencoded(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| format!("{}={}", percent_encode(key), percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Standard base64 (with padding), as the resumable upload's metadata wants it.
#[must_use]
pub fn base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let [a, b, c] = [
            chunk.first().copied().unwrap_or(0),
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let group = (u32::from(a) << 16) | (u32::from(b) << 8) | u32::from(c);
        for shift in [18, 12, 6, 0] {
            let index = usize::try_from((group >> shift) & 0x3f).unwrap_or(0);
            out.push(char::from(ALPHABET.get(index).copied().unwrap_or(b'=')));
        }
        // The last group of one or two bytes carries two or three letters, then padding.
        let padding = 3usize.saturating_sub(chunk.len());
        out.truncate(out.len() - padding);
        out.push_str(&"=".repeat(padding));
    }
    out
}

/// A built `multipart/form-data` body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Multipart {
    pub content_type: String,
    pub bytes: Vec<u8>,
}

/// One part of a multipart body.
#[derive(Debug, Clone, Copy)]
pub enum Part<'a> {
    Text {
        name: &'a str,
        value: &'a str,
    },
    File {
        name: &'a str,
        filename: &'a str,
        content_type: &'a str,
        bytes: &'a [u8],
    },
}

/// Replace characters that could break out of a quoted header parameter.
fn header_safe(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_control() || c == '"' || c == '\\' {
                '_'
            } else {
                c
            }
        })
        .collect()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// Build a multipart body. The boundary is derived deterministically and extended until it occurs
/// nowhere in the payload, so no randomness is needed and the output is reproducible in tests.
#[must_use]
pub fn multipart(parts: &[Part<'_>]) -> Multipart {
    let payload_has = |needle: &[u8]| {
        parts.iter().any(|part| match part {
            Part::Text { value, .. } => contains(value.as_bytes(), needle),
            Part::File { bytes, .. } => contains(bytes, needle),
        })
    };
    let mut boundary = String::from("----putio-adder-boundary");
    while payload_has(boundary.as_bytes()) {
        boundary.push('x');
    }

    let mut body = Vec::new();
    for part in parts {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        match part {
            Part::Text { name, value } => {
                body.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{}\"\r\n\r\n",
                        header_safe(name)
                    )
                    .as_bytes(),
                );
                body.extend_from_slice(value.as_bytes());
            }
            Part::File {
                name,
                filename,
                content_type,
                bytes,
            } => {
                body.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",
                        header_safe(name),
                        header_safe(filename),
                        header_safe(content_type),
                    )
                    .as_bytes(),
                );
                body.extend_from_slice(bytes);
            }
        }
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Multipart {
        content_type: format!("multipart/form-data; boundary={boundary}"),
        bytes: body,
    }
}

/// The two ends of a multipart body whose last part is a file that is not in memory: what comes before the
/// file's bytes and what comes after them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Streamed {
    pub content_type: String,
    pub head: Vec<u8>,
    pub tail: Vec<u8>,
}

/// A boundary nobody can guess or collide with by accident: random per process and per call. (A file's
/// bytes are not searched for it, so it has to be long enough that a match is not a thing that happens.)
fn random_boundary() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)),
    );
    let one = hasher.finish();
    let two = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish();
    format!("----putio-adder-{one:016x}{two:016x}")
}

/// `fields` as text parts, then a file part named `name` for `filename`: the head and tail to put around
/// the file's bytes.
#[must_use]
pub fn streamed(
    fields: &[(&str, &str)],
    name: &str,
    filename: &str,
    content_type: &str,
) -> Streamed {
    let boundary = random_boundary();
    let mut head = Vec::new();
    for (field, value) in fields {
        head.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{}\"\r\n\r\n{value}\r\n",
                header_safe(field)
            )
            .as_bytes(),
        );
    }
    head.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",
            header_safe(name),
            header_safe(filename),
            header_safe(content_type),
        )
        .as_bytes(),
    );
    Streamed {
        content_type: format!("multipart/form-data; boundary={boundary}"),
        head,
        tail: format!("\r\n--{boundary}--\r\n").into_bytes(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_streamed_body_has_the_same_shape_as_a_built_one() {
        let s = streamed(
            &[("parent_id", "7")],
            "file",
            "a \"b\".bin",
            "application/octet-stream",
        );
        let boundary = s
            .content_type
            .strip_prefix("multipart/form-data; boundary=")
            .unwrap();
        let whole = [s.head.as_slice(), b"DATA", s.tail.as_slice()].concat();
        let text = String::from_utf8(whole).unwrap();
        assert_eq!(
            text,
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"parent_id\"\r\n\r\n7\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a _b_.bin\"\r\nContent-Type: application/octet-stream\r\n\r\nDATA\r\n--{boundary}--\r\n"
            )
        );
        // A name cannot break out of its quotes, and two bodies do not share a boundary.
        assert!(boundary.len() >= 40);
        assert_ne!(
            streamed(&[], "f", "x", "t").content_type,
            streamed(&[], "f", "x", "t").content_type
        );
    }

    #[test]
    fn base64_matches_the_standard_test_vectors() {
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
            ("parent_id", "cGFyZW50X2lk"),
            ("true", "dHJ1ZQ=="),
        ] {
            assert_eq!(base64(plain.as_bytes()), encoded, "{plain:?}");
        }
        // Bytes outside ASCII, and every byte value, round through the same table.
        assert_eq!(base64("é".as_bytes()), "w6k=");
        assert_eq!(base64(&[0xfb, 0xff, 0xbf]), "+/+/");
    }

    #[test]
    fn percent_encoding_keeps_only_unreserved() {
        assert_eq!(percent_encode("aZ09-._~"), "aZ09-._~");
        assert_eq!(percent_encode("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
        assert_eq!(percent_encode(""), "");
    }

    #[test]
    fn magnet_links_survive_form_encoding() {
        let magnet = "magnet:?xt=urn:btih:abc&dn=A+B&tr=http%3A%2F%2Fx";
        assert_eq!(
            urlencoded(&[("url", magnet)]),
            "url=magnet%3A%3Fxt%3Durn%3Abtih%3Aabc%26dn%3DA%2BB%26tr%3Dhttp%253A%252F%252Fx"
        );
    }

    #[test]
    fn multipart_shape() {
        let built = multipart(&[
            Part::Text {
                name: "parent_id",
                value: "7",
            },
            Part::File {
                name: "file",
                filename: "a.torrent",
                content_type: "application/x-bittorrent",
                bytes: b"d4:infoe",
            },
        ]);
        let text = String::from_utf8(built.bytes).unwrap();
        let boundary = built
            .content_type
            .strip_prefix("multipart/form-data; boundary=")
            .unwrap();
        assert!(text.starts_with(&format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"parent_id\"\r\n\r\n7\r\n"
        )));
        assert!(text.contains("name=\"file\"; filename=\"a.torrent\"\r\nContent-Type: application/x-bittorrent\r\n\r\nd4:infoe\r\n"));
        assert!(text.ends_with(&format!("--{boundary}--\r\n")));
    }

    #[test]
    fn boundary_never_appears_in_the_payload() {
        let tricky = b"----putio-adder-boundary and ----putio-adder-boundaryx";
        let built = multipart(&[Part::File {
            name: "file",
            filename: "f",
            content_type: "x/y",
            bytes: tricky,
        }]);
        let boundary = built
            .content_type
            .strip_prefix("multipart/form-data; boundary=")
            .unwrap();
        assert!(!contains(tricky, boundary.as_bytes()));
    }

    #[test]
    fn header_parameters_cannot_be_broken_out_of() {
        let built = multipart(&[Part::File {
            name: "file",
            filename: "a\"\r\nX-Evil: 1.torrent",
            content_type: "x",
            bytes: b"",
        }]);
        let text = String::from_utf8(built.bytes).unwrap();
        assert!(!text.contains("X-Evil: 1.torrent\r\n\r\n"));
        assert!(text.contains("filename=\"a___X-Evil: 1.torrent\""));
    }
}
