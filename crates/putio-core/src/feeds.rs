//! RSS feeds: what the user types, what is shown of a feed, and the rules between the two.
//!
//! Everything here is a pure function over text, so the form's validation and the way a feed is
//! described are tested without a window. The put.io calls and the state machine around them are in
//! [`crate::state`].

use putio_api::{RssFeed, RssFeedSettings};

/// Longest feed address accepted (browsers and servers stop around here).
pub const MAX_URL_CHARS: usize = 2048;
/// Longest name kept for a feed.
pub const MAX_TITLE_CHARS: usize = 200;
/// Longest list of filter words kept.
pub const MAX_KEYWORDS_CHARS: usize = 500;

/// What is wrong with the form, in the order the fields are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedProblem {
    UrlMissing,
    /// Not a web address, or one that put.io could not use (no host, spaces, too long).
    UrlInvalid,
}

/// The contents of the add / edit form.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FeedForm {
    pub title: String,
    pub url: String,
    /// The folder new items are saved in (`0` is the top level).
    pub folder_id: i64,
    /// Comma-separated words, as typed.
    pub keyword: String,
    pub unwanted_keywords: String,
    pub delete_old_files: bool,
    /// Only when adding: ignore the items already in the feed.
    pub skip_existing: bool,
}

impl FeedForm {
    /// A new feed that saves into `folder_id`.
    #[must_use]
    pub fn new(folder_id: i64) -> Self {
        Self {
            folder_id,
            ..Self::default()
        }
    }

    /// The form for changing an existing feed.
    #[must_use]
    pub fn from_feed(feed: &RssFeed) -> Self {
        Self {
            title: feed.title.clone(),
            url: feed.rss_source_url.clone(),
            folder_id: feed.parent_dir_id.unwrap_or(0).max(0),
            keyword: feed.keyword.clone().unwrap_or_default(),
            unwanted_keywords: feed.unwanted_keywords.clone().unwrap_or_default(),
            delete_old_files: feed.delete_old_files,
            skip_existing: false,
        }
    }

    /// What to send to put.io, or the first thing wrong. The address is trimmed and checked, the name
    /// defaults to the feed's host (put.io requires one), and the filter words are tidied.
    pub fn settings(&self) -> Result<RssFeedSettings, FeedProblem> {
        let url = self.url.trim();
        if url.is_empty() {
            return Err(FeedProblem::UrlMissing);
        }
        let host = host_of(url).ok_or(FeedProblem::UrlInvalid)?;
        let title = self.title.trim();
        let title = if title.is_empty() {
            host
        } else {
            title.to_owned()
        };
        Ok(RssFeedSettings {
            title: clip(&title, MAX_TITLE_CHARS),
            url: url.to_owned(),
            parent_dir_id: self.folder_id.max(0),
            delete_old_files: self.delete_old_files,
            keyword: tidy_keywords(&self.keyword),
            unwanted_keywords: tidy_keywords(&self.unwanted_keywords),
            skip_existing: self.skip_existing,
        })
    }
}

/// The host of an `http` or `https` address, lowercase, without credentials, port, path or query: what
/// is safe and useful to show for a feed (the rest often carries a private key). `None` when the text
/// is not such an address.
#[must_use]
pub fn host_of(url: &str) -> Option<String> {
    let url = url.trim();
    if url.chars().count() > MAX_URL_CHARS
        || url.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return None;
    }
    let (scheme, rest) = url.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    // Credentials (`user:pass@`) are never shown.
    let host_port = authority.rsplit('@').next()?;
    let host = match host_port.strip_prefix('[') {
        // `[::1]:8080`
        Some(v6) => v6.split(']').next()?,
        None => host_port.split(':').next()?,
    };
    (!host.is_empty()
        && host
            .chars()
            .all(|c| !matches!(c, '@' | '\\' | '<' | '>' | '"')))
    .then(|| host.to_lowercase())
}

/// The words of a comma-separated list, trimmed, without empty ones and without repeats, joined the
/// way put.io shows them.
#[must_use]
pub fn tidy_keywords(text: &str) -> String {
    let mut words: Vec<&str> = Vec::new();
    for word in text.split(',').map(str::trim).filter(|w| !w.is_empty()) {
        if !words.iter().any(|seen| seen.eq_ignore_ascii_case(word)) {
            words.push(word);
        }
    }
    clip(&words.join(", "), MAX_KEYWORDS_CHARS)
}

fn clip(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

/// What to call a feed in the list: its name, else its host, else its address.
#[must_use]
pub fn display_title(feed: &RssFeed) -> String {
    let title: String = feed
        .title
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_owned();
    if !title.is_empty() {
        return title;
    }
    host_of(&feed.rss_source_url).unwrap_or_else(|| clip(feed.rss_source_url.trim(), 80))
}

/// How a feed is doing, for the badge on its row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedStatus {
    /// Being checked for new items.
    Active,
    Paused,
    /// put.io could not read it the last time it looked.
    Problem,
}

#[must_use]
pub fn status_of(feed: &RssFeed) -> FeedStatus {
    if feed.paused {
        FeedStatus::Paused
    } else if feed
        .last_error
        .as_deref()
        .is_some_and(|e| !e.trim().is_empty())
    {
        FeedStatus::Problem
    } else {
        FeedStatus::Active
    }
}

/// Newest first, which is the order put.io creates them in: a feed just added is at the top.
pub fn sort_feeds(feeds: &mut [RssFeed]) {
    feeds.sort_by(|a, b| b.id.cmp(&a.id));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(json: &str) -> RssFeed {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn only_web_addresses_have_a_host() {
        for (url, host) in [
            ("https://Example.COM/feed.xml?key=SECRET", "example.com"),
            ("http://example.com", "example.com"),
            ("HTTPS://example.com:8443/a#b", "example.com"),
            ("https://user:pa55@example.com/rss", "example.com"),
            ("https://[2001:db8::1]:8080/rss", "2001:db8::1"),
            ("  https://example.com/feed  ", "example.com"),
        ] {
            assert_eq!(host_of(url).as_deref(), Some(host), "{url}");
        }
        for url in [
            "",
            "example.com/feed.xml",
            "ftp://example.com/feed",
            "magnet:?xt=urn:btih:abc",
            "https://",
            "https:///feed",
            "https://exa mple.com/feed",
            "https://example.com/\nfeed",
            "javascript://example.com",
            "https://@/feed",
        ] {
            assert_eq!(host_of(url), None, "{url:?}");
        }
        let long = format!("https://example.com/{}", "a".repeat(MAX_URL_CHARS));
        assert_eq!(host_of(&long), None);
    }

    #[test]
    fn credentials_and_keys_never_reach_the_host() {
        let host = host_of("https://me:secret@feeds.example.com/rss?apikey=abc123").unwrap();
        assert_eq!(host, "feeds.example.com");
        assert!(!host.contains("secret") && !host.contains("abc123"));
    }

    #[test]
    fn the_form_is_checked_before_anything_is_sent() {
        let mut form = FeedForm::new(7);
        assert_eq!(form.settings(), Err(FeedProblem::UrlMissing));
        form.url = "   ".into();
        assert_eq!(form.settings(), Err(FeedProblem::UrlMissing));
        form.url = "example.com/feed".into();
        assert_eq!(form.settings(), Err(FeedProblem::UrlInvalid));
        form.url = "  https://Example.com/feed.xml ".into();
        let settings = form.settings().unwrap();
        assert_eq!(settings.url, "https://Example.com/feed.xml");
        assert_eq!(settings.parent_dir_id, 7);
        assert!(!settings.skip_existing && !settings.delete_old_files);
    }

    #[test]
    fn a_feed_without_a_name_is_named_after_its_site() {
        let mut form = FeedForm::new(0);
        form.url = "https://feeds.example.com/rss".into();
        assert_eq!(form.settings().unwrap().title, "feeds.example.com");
        form.title = "  My shows  ".into();
        assert_eq!(form.settings().unwrap().title, "My shows");
        form.title = "x".repeat(MAX_TITLE_CHARS + 50);
        assert_eq!(
            form.settings().unwrap().title.chars().count(),
            MAX_TITLE_CHARS
        );
    }

    #[test]
    fn filter_words_are_tidied_and_bounded() {
        assert_eq!(
            tidy_keywords(" 1080p ,, 2160p,1080P , x265 "),
            "1080p, 2160p, x265"
        );
        assert_eq!(tidy_keywords(""), "");
        assert_eq!(tidy_keywords(" , , "), "");
        let long = (0..500)
            .map(|n| format!("w{n}"))
            .collect::<Vec<_>>()
            .join(",");
        assert!(tidy_keywords(&long).chars().count() <= MAX_KEYWORDS_CHARS);
        let mut form = FeedForm::new(0);
        form.url = "https://example.com/f".into();
        form.keyword = " a,b ".into();
        form.unwanted_keywords = "cam".into();
        let s = form.settings().unwrap();
        assert_eq!(
            (s.keyword.as_str(), s.unwanted_keywords.as_str()),
            ("a, b", "cam")
        );
    }

    #[test]
    fn editing_starts_from_the_feed_as_it_is() {
        let f = feed(
            r#"{"id":5,"title":"T","rss_source_url":"https://e.com/f","parent_dir_id":12,
                "delete_old_files":true,"keyword":"hd","unwanted_keywords":null}"#,
        );
        let form = FeedForm::from_feed(&f);
        assert_eq!(form.folder_id, 12);
        assert_eq!(form.keyword, "hd");
        assert_eq!(form.unwanted_keywords, "");
        assert!(form.delete_old_files);
        // Whatever the server sends for the folder, the form holds a usable one.
        assert_eq!(
            FeedForm::from_feed(&feed(r#"{"id":1,"parent_dir_id":-4}"#)).folder_id,
            0
        );
        assert_eq!(FeedForm::from_feed(&feed(r#"{"id":1}"#)).folder_id, 0);
        // Round trip: unchanged form, same settings.
        let again = form.settings().unwrap();
        assert_eq!((again.title.as_str(), again.parent_dir_id), ("T", 12));
    }

    #[test]
    fn a_feed_is_called_by_its_name_then_its_site() {
        assert_eq!(
            display_title(&feed(
                r#"{"id":1,"title":" Shows\u0007 ","rss_source_url":"https://e.com/x"}"#
            )),
            "Shows"
        );
        assert_eq!(
            display_title(&feed(
                r#"{"id":1,"title":"","rss_source_url":"https://Feeds.E.com/x?k=1"}"#
            )),
            "feeds.e.com"
        );
        assert_eq!(
            display_title(&feed(r#"{"id":1,"rss_source_url":"not a url"}"#)),
            "not a url"
        );
        assert_eq!(display_title(&feed(r#"{"id":1}"#)), "");
    }

    #[test]
    fn a_paused_feed_is_paused_whatever_its_last_error_was() {
        assert_eq!(status_of(&feed(r#"{"id":1}"#)), FeedStatus::Active);
        assert_eq!(
            status_of(&feed(r#"{"id":1,"paused":true}"#)),
            FeedStatus::Paused
        );
        assert_eq!(
            status_of(&feed(r#"{"id":1,"last_error":"FEED_CANNOT_FETCHED"}"#)),
            FeedStatus::Problem
        );
        assert_eq!(
            status_of(&feed(r#"{"id":1,"last_error":"  "}"#)),
            FeedStatus::Active
        );
        assert_eq!(
            status_of(&feed(r#"{"id":1,"paused":true,"last_error":"x"}"#)),
            FeedStatus::Paused
        );
    }

    #[test]
    fn the_newest_feed_comes_first() {
        let mut feeds = vec![
            feed(r#"{"id":2}"#),
            feed(r#"{"id":9}"#),
            feed(r#"{"id":4}"#),
        ];
        sort_feeds(&mut feeds);
        assert_eq!(feeds.iter().map(|f| f.id).collect::<Vec<_>>(), [9, 4, 2]);
    }
}
