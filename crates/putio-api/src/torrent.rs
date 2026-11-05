//! Cheap sanity checks on `.torrent` bytes before they are uploaded.

/// put.io's limit is not documented; real `.torrent` files are well below this.
pub const MAX_TORRENT_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidTorrent {
    #[error("the file is empty")]
    Empty,
    #[error("the file is larger than {} MiB", MAX_TORRENT_BYTES / 1024 / 1024)]
    TooLarge,
    #[error("the file is not a torrent (no bencoded dictionary with an info section)")]
    NotATorrent,
}

/// A `.torrent` is a bencoded dictionary (`d…e`) with an `info` key. This does not parse the whole
/// structure (put.io does that); it only stops obviously wrong or oversized files from being sent.
pub fn validate(bytes: &[u8]) -> Result<(), InvalidTorrent> {
    if bytes.is_empty() {
        return Err(InvalidTorrent::Empty);
    }
    if bytes.len() > MAX_TORRENT_BYTES {
        return Err(InvalidTorrent::TooLarge);
    }
    let has_info = bytes.windows(6).any(|window| window == b"4:info");
    if bytes.first() == Some(&b'd') && bytes.last() == Some(&b'e') && has_info {
        Ok(())
    } else {
        Err(InvalidTorrent::NotATorrent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_minimal_torrent() {
        assert_eq!(validate(b"d8:announce3:url4:infod4:name1:aee"), Ok(()));
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(validate(b""), Err(InvalidTorrent::Empty));
        assert_eq!(validate(b"<html>"), Err(InvalidTorrent::NotATorrent));
        assert_eq!(
            validate(b"d8:announce3:urle"),
            Err(InvalidTorrent::NotATorrent)
        );
        assert_eq!(validate(b"d4:infod"), Err(InvalidTorrent::NotATorrent));
        assert_eq!(
            validate(&vec![b'd'; MAX_TORRENT_BYTES + 1]),
            Err(InvalidTorrent::TooLarge)
        );
    }
}
