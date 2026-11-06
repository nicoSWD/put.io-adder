//! The typed put.io client: one method per endpoint the app uses.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::{ApiError, ApiFailure, retry_delay};
use crate::form::{self, Part};
use crate::models::{
    Account, AccountSettingChange, DeviceCode, File, FileType, Release, RssFeed, RssFeedSettings,
    Transfer, UploadOutcome, ZipStatus,
};
use crate::token::Token;
use crate::torrent;
use crate::transport::{Body, Host, Method, Request, Response, Transport};

const JSON: &str = "application/json";
const GITHUB_JSON: &str = "application/vnd.github+json";
/// GitHub project the update check reads releases from.
const GITHUB_REPO: &str = "nicoSWD/put.io-adder";
/// Page size for file listings (spec maximum is 1000).
const PER_PAGE: &str = "1000";
/// Upper bound on pages followed for one listing, so a misbehaving cursor cannot loop forever.
const MAX_PAGES: usize = 50;

/// The answer to a file search: the first results, and how many there are in all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSearch {
    pub files: Vec<File>,
    /// How many files match (more than `files` holds when there are many).
    pub total: usize,
}

/// How many search results are asked for: enough to find a file, few enough to answer quickly.
const SEARCH_RESULTS: &str = "100";

/// One page of a file listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileListing {
    /// The folder that was listed.
    pub parent: Option<File>,
    pub files: Vec<File>,
}

#[derive(Debug)]
pub struct Client<T> {
    transport: T,
    token: Option<Token>,
}

impl<T: Transport> Client<T> {
    #[must_use]
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            token: None,
        }
    }

    #[must_use]
    pub fn with_token(transport: T, token: Token) -> Self {
        Self {
            transport,
            token: Some(token),
        }
    }

    pub fn set_token(&mut self, token: Option<Token>) {
        self.token = token;
    }

    #[must_use]
    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }

    // ---- plumbing -------------------------------------------------------------------------

    fn request(
        &self,
        method: Method,
        host: Host,
        path: String,
        body: Body,
        accept: &'static str,
        authenticated: bool,
    ) -> Request {
        Request {
            method,
            host,
            path,
            accept,
            bearer: if authenticated {
                self.token.clone()
            } else {
                None
            },
            body,
        }
    }

    fn send(&self, request: &Request) -> Result<Response, ApiError> {
        let response = self.transport.execute(request)?;
        match response.status {
            200..=299 => Ok(response),
            401 => Err(ApiError::Unauthorized),
            429 => {
                let number = |name: &str| {
                    response
                        .header(name)
                        .and_then(|value| value.trim().parse::<u64>().ok())
                };
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                Err(ApiError::RateLimited {
                    retry_after: retry_delay(
                        number("x-ratelimit-reset"),
                        number("retry-after"),
                        now,
                    ),
                })
            }
            status => Err(ApiError::Api(failure_from(status, &response.body))),
        }
    }

    /// Send and decode a JSON body. A 2xx body of `{"status": "ERROR", …}` is still an error.
    fn json<D: DeserializeOwned>(&self, request: &Request) -> Result<D, ApiError> {
        let response = self.send(request)?;
        let value: Value =
            serde_json::from_slice(&response.body).map_err(|e| ApiError::Decode(e.to_string()))?;
        if value.get("status").and_then(Value::as_str) == Some("ERROR") {
            return Err(ApiError::Api(failure_from_value(response.status, &value)));
        }
        serde_json::from_value(value).map_err(|e| ApiError::Decode(e.to_string()))
    }

    /// For endpoints whose 200 has no documented body (e.g. cancel): success unless the body, when
    /// there is one, says `"status": "ERROR"`.
    fn ack(&self, request: &Request) -> Result<(), ApiError> {
        let response = self.send(request)?;
        if let Ok(value) = serde_json::from_slice::<Value>(&response.body)
            && value.get("status").and_then(Value::as_str) == Some("ERROR")
        {
            return Err(ApiError::Api(failure_from_value(response.status, &value)));
        }
        Ok(())
    }

    fn get(&self, path: String) -> Request {
        self.request(Method::Get, Host::Api, path, Body::None, JSON, true)
    }

    fn post_form(&self, path: &str, pairs: &[(&str, &str)]) -> Request {
        self.request(
            Method::Post,
            Host::Api,
            path.to_owned(),
            Body::Form(form::urlencoded(pairs)),
            JSON,
            true,
        )
    }

    // ---- authentication (device / OOB flow) ------------------------------------------------

    /// Step 1: ask for a code to show the user. They enter it at <https://put.io/link>.
    pub fn oob_code(&self, app_id: u32, client_name: &str) -> Result<DeviceCode, ApiError> {
        let query = form::urlencoded(&[
            ("app_id", &app_id.to_string()),
            ("client_name", client_name),
        ]);
        let request = self.request(
            Method::Get,
            Host::Api,
            format!("/oauth2/oob/code?{query}"),
            Body::None,
            JSON,
            false,
        );
        self.json(&request)
    }

    /// Step 2: poll until the user has linked the code. `Ok(None)` means "not yet".
    pub fn oob_poll(&self, code: &str) -> Result<Option<Token>, ApiError> {
        #[derive(serde::Deserialize)]
        struct Poll {
            #[serde(default)]
            oauth_token: Option<String>,
        }
        // The code ends up in a path segment: allow nothing that could change the path.
        if code.is_empty() || code.len() > 64 || !code.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(ApiError::InvalidInput(
                "device code must be 1-64 alphanumeric characters",
            ));
        }
        let request = self.request(
            Method::Get,
            Host::Api,
            format!("/oauth2/oob/code/{code}"),
            Body::None,
            JSON,
            false,
        );
        let poll: Poll = self.json(&request)?;
        match poll.oauth_token.filter(|token| !token.is_empty()) {
            None => Ok(None),
            Some(raw) => Token::new(raw)
                .map(Some)
                .map_err(|e| ApiError::Decode(format!("token from put.io rejected: {e}"))),
        }
    }

    // ---- account & transfers -------------------------------------------------------------

    pub fn account(&self) -> Result<Account, ApiError> {
        #[derive(serde::Deserialize)]
        struct Reply {
            info: Account,
        }
        Ok(self.json::<Reply>(&self.get("/account/info".into()))?.info)
    }

    /// Change one of the account's settings. The reply is only checked for an error: what the account looks
    /// like afterwards comes with the next `/account/info`.
    pub fn save_account_setting(&self, change: AccountSettingChange) -> Result<(), ApiError> {
        let request = self.request(
            Method::Post,
            Host::Api,
            "/account/settings".into(),
            Body::Raw {
                content_type: "application/json".into(),
                bytes: change.to_json().into_bytes(),
            },
            JSON,
            true,
        );
        self.ack(&request)
    }

    pub fn transfers(&self) -> Result<Vec<Transfer>, ApiError> {
        #[derive(serde::Deserialize)]
        struct Reply {
            #[serde(default)]
            transfers: Vec<Transfer>,
        }
        Ok(self
            .json::<Reply>(&self.get("/transfers/list".into()))?
            .transfers)
    }

    /// One transfer, with everything put.io knows about it right now. A transfer that is gone (cleared or
    /// cancelled) answers with an [`ApiError::Api`] whose status is 404.
    pub fn transfer(&self, id: i64) -> Result<Transfer, ApiError> {
        #[derive(serde::Deserialize)]
        struct Reply {
            transfer: Transfer,
        }
        Ok(self
            .json::<Reply>(&self.get(format!("/transfers/{id}")))?
            .transfer)
    }

    /// Add a magnet link or HTTP(S) URL. `save_parent_id` is the destination folder (account default if `None`).
    pub fn add_transfer(
        &self,
        url: &str,
        save_parent_id: Option<i64>,
    ) -> Result<Transfer, ApiError> {
        #[derive(serde::Deserialize)]
        struct Reply {
            transfer: Transfer,
        }
        let parent = save_parent_id.map(|id| id.to_string());
        let mut pairs = vec![("url", url)];
        if let Some(parent) = &parent {
            pairs.push(("save_parent_id", parent));
        }
        Ok(self
            .json::<Reply>(&self.post_form("/transfers/add", &pairs))?
            .transfer)
    }

    /// Cancel transfers by id (ids, not list positions).
    pub fn cancel_transfers(&self, ids: &[i64]) -> Result<(), ApiError> {
        if ids.is_empty() {
            return Err(ApiError::InvalidInput("no transfer ids"));
        }
        let joined = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        self.ack(&self.post_form("/transfers/cancel", &[("transfer_ids", &joined)]))
    }

    /// Retry a transfer that failed.
    pub fn retry_transfer(&self, id: i64) -> Result<(), ApiError> {
        // The id is a query parameter in the spec (`POST /transfers/retry?id=`), with no body.
        let request = self.request(
            Method::Post,
            Host::Api,
            format!("/transfers/retry?id={id}"),
            Body::None,
            JSON,
            true,
        );
        self.ack(&request)
    }

    /// Remove finished transfers from the list. Only the entries go: the files stay on put.io.
    pub fn clean_transfers(&self, ids: &[i64]) -> Result<(), ApiError> {
        if ids.is_empty() {
            return Err(ApiError::InvalidInput("no transfer ids"));
        }
        let joined = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        self.ack(&self.post_form("/transfers/clean", &[("transfer_ids", &joined)]))
    }

    // ---- rss ------------------------------------------------------------------------------

    /// The RSS feeds put.io watches for the account.
    pub fn rss_feeds(&self) -> Result<Vec<RssFeed>, ApiError> {
        #[derive(serde::Deserialize)]
        struct Reply {
            #[serde(default)]
            feeds: Vec<RssFeed>,
        }
        Ok(self.json::<Reply>(&self.get("/rss/list".into()))?.feeds)
    }

    /// Start watching a feed. put.io checks the address and answers with an error when it is not a feed it
    /// can read, so a bad address comes back as an [`ApiError::Api`] with a message for the user.
    pub fn create_rss_feed(&self, feed: &RssFeedSettings) -> Result<(), ApiError> {
        let parent = feed.parent_dir_id.to_string();
        let mut pairs = rss_fields(feed, &parent)?;
        pairs.push((
            "dont_process_whole_feed",
            if feed.skip_existing { "true" } else { "false" },
        ));
        // The create form takes the checkbox the way a browser sends it: present (`on`) or absent.
        if feed.delete_old_files {
            pairs.push(("delete_old_files", "on"));
        }
        self.ack(&self.post_form("/rss/create", &pairs))
    }

    /// Change a feed's address, destination, filters or housekeeping.
    pub fn update_rss_feed(&self, id: i64, feed: &RssFeedSettings) -> Result<(), ApiError> {
        let parent = feed.parent_dir_id.to_string();
        let mut pairs = rss_fields(feed, &parent)?;
        pairs.push((
            "delete_old_files",
            if feed.delete_old_files {
                "true"
            } else {
                "false"
            },
        ));
        self.ack(&self.post_form(&format!("/rss/{id}"), &pairs))
    }

    /// Stop (or start again) checking a feed for new items.
    pub fn set_rss_feed_paused(&self, id: i64, paused: bool) -> Result<(), ApiError> {
        let action = if paused { "pause" } else { "resume" };
        self.ack(&self.rss_action(&format!("/rss/{id}/{action}")))
    }

    /// Stop watching a feed. What it already brought in stays in the account.
    pub fn delete_rss_feed(&self, id: i64) -> Result<(), ApiError> {
        self.ack(&self.rss_action(&format!("/rss/{id}/delete")))
    }

    /// The feed endpoints that take no parameters: a `POST` with an empty body.
    fn rss_action(&self, path: &str) -> Request {
        self.request(
            Method::Post,
            Host::Api,
            path.to_owned(),
            Body::None,
            JSON,
            true,
        )
    }

    // ---- files ----------------------------------------------------------------------------

    /// Upload a `.torrent`. `filename` is only a label; `parent_id` is the destination folder.
    pub fn upload_torrent(
        &self,
        filename: &str,
        bytes: &[u8],
        parent_id: Option<i64>,
    ) -> Result<UploadOutcome, ApiError> {
        #[derive(serde::Deserialize)]
        struct Reply {
            #[serde(default)]
            transfer: Option<Transfer>,
            #[serde(default)]
            file: Option<File>,
        }
        torrent::validate(bytes).map_err(|e| {
            ApiError::InvalidInput(match e {
                torrent::InvalidTorrent::Empty => "torrent file is empty",
                torrent::InvalidTorrent::TooLarge => "torrent file is too large",
                torrent::InvalidTorrent::NotATorrent => "file is not a torrent",
            })
        })?;
        let parent = parent_id.map(|id| id.to_string());
        let mut parts = vec![
            Part::File {
                name: "file",
                filename,
                content_type: "application/x-bittorrent",
                bytes,
            },
            Part::Text {
                name: "filename",
                value: filename,
            },
        ];
        if let Some(parent) = &parent {
            parts.push(Part::Text {
                name: "parent_id",
                value: parent,
            });
        }
        let built = form::multipart(&parts);
        let request = self.request(
            Method::Post,
            Host::Upload,
            "/files/upload".into(),
            Body::Raw {
                content_type: built.content_type,
                bytes: built.bytes,
            },
            JSON,
            true,
        );

        let reply: Reply = self.json(&request)?;
        match (reply.transfer, reply.file) {
            (Some(transfer), _) => Ok(UploadOutcome::Transfer(Box::new(transfer))),
            (None, Some(file)) => Ok(UploadOutcome::File(file)),
            (None, None) => Err(ApiError::Decode(
                "upload reply has neither `transfer` nor `file`".into(),
            )),
        }
    }

    /// List a folder (`0` is the root), following pagination cursors. `only` filters by type.
    pub fn list_files(
        &self,
        parent_id: i64,
        only: Option<FileType>,
    ) -> Result<FileListing, ApiError> {
        #[derive(serde::Deserialize)]
        struct Page {
            #[serde(default)]
            files: Vec<File>,
            #[serde(default)]
            parent: Option<File>,
            #[serde(default)]
            cursor: Option<String>,
        }
        let id = parent_id.to_string();
        let mut pairs = vec![("parent_id", id.as_str()), ("per_page", PER_PAGE)];
        if let Some(kind) = only {
            pairs.push(("file_type", kind.as_query()));
        }
        let mut page: Page =
            self.json(&self.get(format!("/files/list?{}", form::urlencoded(&pairs))))?;
        let parent = page.parent.take();
        let mut files = std::mem::take(&mut page.files);

        for _ in 1..MAX_PAGES {
            let Some(cursor) = page.cursor.take().filter(|c| !c.is_empty()) else {
                break;
            };
            page = self.json(&self.post_form(
                "/files/list/continue",
                &[("cursor", &cursor), ("per_page", PER_PAGE)],
            ))?;
            files.append(&mut page.files);
        }
        Ok(FileListing { parent, files })
    }

    /// Search the account's files (and what friends shared) by name. Only the first page is read: a
    /// search that needs more than that needs a better question.
    pub fn search_files(&self, query: &str) -> Result<FileSearch, ApiError> {
        #[derive(serde::Deserialize)]
        struct Reply {
            #[serde(default)]
            files: Vec<File>,
            #[serde(default)]
            total: Option<usize>,
        }
        let query = query.trim();
        if query.is_empty() {
            return Err(ApiError::InvalidInput("search is empty"));
        }
        let pairs = [("query", query), ("per_page", SEARCH_RESULTS)];
        let reply: Reply =
            self.json(&self.get(format!("/files/search?{}", form::urlencoded(&pairs))))?;
        let total = reply
            .total
            .unwrap_or(reply.files.len())
            .max(reply.files.len());
        Ok(FileSearch {
            files: reply.files,
            total,
        })
    }

    /// Give a file or folder another name.
    pub fn rename_file(&self, id: i64, name: &str) -> Result<(), ApiError> {
        if name.trim().is_empty() {
            return Err(ApiError::InvalidInput("name is empty"));
        }
        let id = id.to_string();
        self.ack(&self.post_form("/files/rename", &[("file_id", &id), ("name", name)]))
    }

    /// Move files and folders into the folder `parent_id` (`0` is the top level).
    pub fn move_files(&self, ids: &[i64], parent_id: i64) -> Result<(), ApiError> {
        if ids.is_empty() {
            return Err(ApiError::InvalidInput("no file ids"));
        }
        let joined = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        let parent = parent_id.to_string();
        self.ack(&self.post_form(
            "/files/move",
            &[("file_ids", &joined), ("parent_id", &parent)],
        ))
    }

    /// Delete files and folders (a folder with everything in it). put.io has no trash: this cannot be undone.
    pub fn delete_files(&self, ids: &[i64]) -> Result<(), ApiError> {
        if ids.is_empty() {
            return Err(ApiError::InvalidInput("no file ids"));
        }
        let joined = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        self.ack(&self.post_form("/files/delete", &[("file_ids", &joined)]))
    }

    /// Make a folder inside `parent_id` (`0` is the top level).
    pub fn create_folder(&self, name: &str, parent_id: i64) -> Result<(), ApiError> {
        if name.trim().is_empty() {
            return Err(ApiError::InvalidInput("name is empty"));
        }
        let parent = parent_id.to_string();
        self.ack(&self.post_form(
            "/files/create-folder",
            &[("name", name), ("parent_id", &parent)],
        ))
    }

    /// One file or folder: its name, mostly (the feed form shows which folder a feed saves into).
    pub fn file(&self, id: i64) -> Result<File, ApiError> {
        #[derive(serde::Deserialize)]
        struct Reply {
            file: File,
        }
        Ok(self.json::<Reply>(&self.get(format!("/files/{id}")))?.file)
    }

    /// A signed, IP-bound URL for a file. It carries its own credentials, so the account token never
    /// has to be handed to a player. Always `https`.
    pub fn file_url(&self, file_id: i64) -> Result<String, ApiError> {
        #[derive(serde::Deserialize)]
        struct Reply {
            url: String,
        }
        let reply: Reply = self.json(&self.get(format!("/files/{file_id}/url")))?;
        if reply.url.starts_with("https://") {
            Ok(reply.url)
        } else {
            Err(ApiError::Decode("file URL is not https".into()))
        }
    }

    /// Ask put.io to build a ZIP of `file_ids` (a folder, say). It is built in the background: poll
    /// [`Client::zip_status`] with the returned id.
    pub fn create_zip(&self, file_ids: &[i64]) -> Result<i64, ApiError> {
        #[derive(serde::Deserialize)]
        struct Reply {
            zip_id: i64,
        }
        if file_ids.is_empty() {
            return Err(ApiError::InvalidInput("no file ids"));
        }
        let joined = file_ids
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let reply: Reply = self.json(&self.post_form("/zips/create", &[("file_ids", &joined)]))?;
        Ok(reply.zip_id)
    }

    /// How far a ZIP is. Its `url` appears once the state is `Done`.
    pub fn zip_status(&self, zip_id: i64) -> Result<ZipStatus, ApiError> {
        self.json(&self.get(format!("/zips/{zip_id}")))
    }

    // ---- updates --------------------------------------------------------------------------

    /// Latest published (non-draft, non-prerelease) release of this app on GitHub.
    pub fn latest_release(&self) -> Result<Release, ApiError> {
        let request = self.request(
            Method::Get,
            Host::GitHub,
            format!("/repos/{GITHUB_REPO}/releases/latest"),
            Body::None,
            GITHUB_JSON,
            false,
        );
        let release: Release = self.json(&request)?;
        if !release.html_url.starts_with("https://github.com/") {
            return Err(ApiError::Decode("release URL is not on github.com".into()));
        }
        Ok(release)
    }
}

/// The fields both `rss/create` and `rss/{id}` take. A feed needs an address and a title (put.io refuses
/// both when empty), so that is checked here, before anything is sent.
fn rss_fields<'a>(
    feed: &'a RssFeedSettings,
    parent: &'a str,
) -> Result<Vec<(&'static str, &'a str)>, ApiError> {
    if feed.url.trim().is_empty() {
        return Err(ApiError::InvalidInput("feed address is empty"));
    }
    if feed.title.trim().is_empty() {
        return Err(ApiError::InvalidInput("feed title is empty"));
    }
    Ok(vec![
        ("title", feed.title.as_str()),
        ("rss_source_url", feed.url.as_str()),
        ("parent_dir_id", parent),
        ("keyword", feed.keyword.as_str()),
        ("unwanted_keywords", feed.unwanted_keywords.as_str()),
    ])
}

fn failure_from(status: u16, body: &[u8]) -> ApiFailure {
    match serde_json::from_slice::<Value>(body) {
        Ok(value) => failure_from_value(status, &value),
        Err(_) => ApiFailure {
            status,
            error_type: None,
            message: format!("HTTP {status}"),
        },
    }
}

fn failure_from_value(status: u16, value: &Value) -> ApiFailure {
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
    ApiFailure {
        status,
        error_type: text("error_type"),
        message: text("error_message").unwrap_or_else(|| format!("HTTP {status}")),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::models::TransferStatus;
    use crate::models::ZipState;
    use crate::testing::FakeTransport;
    use crate::transport::TransportError;

    fn token() -> Token {
        Token::new("tok-123").unwrap()
    }

    fn client(transport: &FakeTransport) -> Client<&FakeTransport> {
        Client::with_token(transport, token())
    }

    #[test]
    fn account_with_and_without_disk() {
        let fake = FakeTransport::new();
        fake.push_json(200, include_str!("../tests/fixtures/account_info.json"));
        fake.push_json(
            200,
            include_str!("../tests/fixtures/account_info_with_disk.json"),
        );
        let c = client(&fake);
        assert_eq!(c.account().unwrap().disk, None);
        assert_eq!(
            c.account().unwrap().disk.unwrap().used_fraction(),
            Some(0.25)
        );
        assert_eq!(fake.requests()[0].describe(), "GET /account/info");
    }

    #[test]
    fn the_accounts_settings_are_read_from_its_info_and_an_odd_one_is_dropped() {
        let fake = FakeTransport::new();
        fake.push_json(
            200,
            r#"{"status":"OK","info":{"user_id":1,"username":"u","settings":{"default_download_folder":7,"trash_enabled":false,"theme":"dark"}}}"#,
        );
        fake.push_json(
            200,
            r#"{"status":"OK","info":{"user_id":1,"username":"u","settings":{"default_download_folder":"x","trash_enabled":true}}}"#,
        );
        fake.push_json(
            200,
            r#"{"status":"OK","info":{"user_id":1,"username":"u","settings":"nonsense"}}"#,
        );
        fake.push_json(
            200,
            r#"{"status":"OK","info":{"user_id":1,"username":"u"}}"#,
        );
        let c = client(&fake);
        let settings = c.account().unwrap().settings.unwrap();
        assert_eq!(settings.default_download_folder, Some(7));
        assert_eq!(settings.trash_enabled, Some(false));
        let settings = c.account().unwrap().settings.unwrap();
        assert_eq!(settings.default_download_folder, None);
        assert_eq!(settings.trash_enabled, Some(true));
        // Nothing here ever makes the account unreadable.
        assert_eq!(c.account().unwrap().settings, None);
        assert_eq!(c.account().unwrap().settings, None);
    }

    #[test]
    fn one_account_setting_is_sent_as_a_json_object_with_one_key() {
        let fake = FakeTransport::new();
        fake.push_json(200, r#"{"status":"OK"}"#);
        fake.push_json(200, r#"{"status":"OK"}"#);
        fake.push_json(
            400,
            r#"{"status":"ERROR","error_type":"INVALID_VALUE","error_message":"nope"}"#,
        );
        let c = client(&fake);
        c.save_account_setting(AccountSettingChange::TrashEnabled(false))
            .unwrap();
        let request = fake.last_request().unwrap();
        assert_eq!(request.describe(), "POST /account/settings");
        assert_eq!(
            request.body,
            Body::Raw {
                content_type: "application/json".into(),
                bytes: br#"{"trash_enabled":false}"#.to_vec(),
            }
        );
        c.save_account_setting(AccountSettingChange::DefaultDownloadFolder(42))
            .unwrap();
        assert_eq!(
            fake.last_request().unwrap().body,
            Body::Raw {
                content_type: "application/json".into(),
                bytes: br#"{"default_download_folder":42}"#.to_vec(),
            }
        );
        // What put.io refuses comes back as its error.
        let Err(ApiError::Api(failure)) =
            c.save_account_setting(AccountSettingChange::DefaultDownloadFolder(-1))
        else {
            panic!("expected an API error")
        };
        assert_eq!(failure.error_type.as_deref(), Some("INVALID_VALUE"));
    }

    #[test]
    fn the_token_is_only_ever_sent_as_a_bearer() {
        let fake = FakeTransport::new();
        fake.push_json(200, include_str!("../tests/fixtures/transfers_list.json"));
        fake.push_json(200, include_str!("../tests/fixtures/transfer_added.json"));
        fake.push_json(200, r#"{"status":"OK"}"#);
        fake.push_json(200, r#"{"url":"https://example.invalid/f"}"#);
        let c = client(&fake);
        c.transfers().unwrap();
        c.add_transfer("magnet:?xt=urn:btih:abc", None).unwrap();
        c.cancel_transfers(&[1]).unwrap();
        c.file_url(5).unwrap();
        for request in fake.requests() {
            assert_eq!(request.bearer.as_ref().map(Token::expose), Some("tok-123"));
            assert!(!request.path.contains("tok-123"), "{}", request.path);
            if let Body::Form(form) = &request.body {
                assert!(!form.contains("tok-123"));
            }
        }
    }

    #[test]
    fn unauthenticated_calls_carry_no_token() {
        let fake = FakeTransport::new();
        fake.push_json(200, r#"{"code":"ABCD1234"}"#);
        let c = client(&fake);
        assert_eq!(c.oob_code(711, "put.io adder").unwrap().code, "ABCD1234");
        let request = fake.last_request().unwrap();
        assert!(request.bearer.is_none());
        assert_eq!(
            request.path,
            "/oauth2/oob/code?app_id=711&client_name=put.io%20adder"
        );
    }

    #[test]
    fn transfers_list_maps_every_status_including_unknown() {
        let fake = FakeTransport::new();
        fake.push_json(200, include_str!("../tests/fixtures/transfers_list.json"));
        let transfers = client(&fake).transfers().unwrap();
        let statuses: Vec<_> = transfers.iter().map(|t| t.status).collect();
        assert_eq!(
            statuses,
            [
                TransferStatus::Downloading,
                TransferStatus::Completed,
                TransferStatus::Error,
                TransferStatus::Unknown
            ]
        );
        assert_eq!(transfers[0].file_id, None);
        assert_eq!(transfers[1].file_id, Some(987_654));
        assert_eq!(
            transfers[2].error_message.as_deref(),
            Some("Tracker unreachable")
        );
    }

    #[test]
    fn add_transfer_encodes_url_and_destination_folder() {
        let fake = FakeTransport::new();
        fake.push_json(200, include_str!("../tests/fixtures/transfer_added.json"));
        let added = client(&fake)
            .add_transfer("magnet:?xt=urn:btih:abc&dn=x y", Some(42))
            .unwrap();
        assert_eq!(added.id, 77);
        let request = fake.last_request().unwrap();
        assert_eq!(request.describe(), "POST /transfers/add");
        assert_eq!(
            request.body,
            Body::Form(
                "url=magnet%3A%3Fxt%3Durn%3Abtih%3Aabc%26dn%3Dx%20y&save_parent_id=42".into()
            )
        );
    }

    #[test]
    fn cancel_uses_ids_and_rejects_empty() {
        let fake = FakeTransport::new();
        fake.push_json(200, r#"{"status":"OK"}"#);
        let c = client(&fake);
        c.cancel_transfers(&[3, 9]).unwrap();
        assert_eq!(
            fake.last_request().unwrap().body,
            Body::Form("transfer_ids=3%2C9".into())
        );
        assert_eq!(
            c.cancel_transfers(&[]).unwrap_err(),
            ApiError::InvalidInput("no transfer ids")
        );
    }

    #[test]
    fn cancel_accepts_an_empty_200_as_the_spec_documents() {
        let fake = FakeTransport::new();
        fake.push_json(200, "");
        fake.push_json(
            200,
            r#"{"status":"ERROR","error_type":"NotFound","error_message":"x"}"#,
        );
        let c = client(&fake);
        assert_eq!(c.cancel_transfers(&[1]), Ok(()));
        assert_eq!(
            c.cancel_transfers(&[1]).unwrap_err().error_type(),
            Some("NotFound")
        );
    }

    #[test]
    fn retry_posts_the_id_as_a_query_parameter_without_a_body() {
        let fake = FakeTransport::new();
        fake.push_json(200, "");
        fake.push_json(
            404,
            r#"{"error_type":"NotFound","error_message":"no such transfer"}"#,
        );
        let c = client(&fake);
        c.retry_transfer(42).unwrap();
        let request = fake.last_request().unwrap();
        assert_eq!(request.describe(), "POST /transfers/retry?id=42");
        assert_eq!(request.body, Body::None);
        assert_eq!(
            c.retry_transfer(1).unwrap_err().error_type(),
            Some("NotFound")
        );
    }

    fn feed() -> RssFeedSettings {
        RssFeedSettings {
            title: "Spring & Co".into(),
            url: "https://example.invalid/feed.xml?a=1&b=2".into(),
            parent_dir_id: 42,
            delete_old_files: false,
            keyword: "1080p, 2160p".into(),
            unwanted_keywords: String::new(),
            skip_existing: false,
        }
    }

    #[test]
    fn one_transfer_is_read_with_where_it_saves_and_which_feed_added_it() {
        let fake = FakeTransport::new();
        fake.push_json(
            200,
            r#"{"status":"OK","transfer":{"id":77,"name":"Show S01E01","status":"SEEDING","type":"TORRENT",
                "peers":12,"current_ratio":1.5,"save_parent_id":42,"subscription_id":5,"file_id":900}}"#,
        );
        let transfer = client(&fake).transfer(77).unwrap();
        assert_eq!(fake.last_request().unwrap().describe(), "GET /transfers/77");
        assert_eq!(transfer.id, 77);
        assert_eq!(transfer.peers, Some(12));
        assert_eq!(transfer.save_parent_id, Some(42));
        assert_eq!(transfer.subscription_id, Some(5));
        assert_eq!(transfer.file_id, Some(900));
        // Neither is sent for a transfer that was not added by a feed, and odd values are dropped.
        fake.push_json(
            200,
            r#"{"transfer":{"id":78,"save_parent_id":"x","subscription_id":null}}"#,
        );
        let plain = client(&fake).transfer(78).unwrap();
        assert_eq!((plain.save_parent_id, plain.subscription_id), (None, None));
        // A transfer that is gone is a 404, and a reply without the transfer is an error.
        fake.push_json(404, r#"{"error_type":"NotFound","error_message":"gone"}"#);
        let gone = client(&fake).transfer(79).unwrap_err();
        assert!(
            matches!(&gone, ApiError::Api(f) if f.status == 404),
            "{gone:?}"
        );
        fake.push_json(200, r#"{"status":"OK"}"#);
        assert!(matches!(
            client(&fake).transfer(80),
            Err(ApiError::Decode(_))
        ));
    }

    #[test]
    fn a_search_asks_for_the_first_page_and_says_how_many_there_are() {
        let fake = FakeTransport::new();
        fake.push_json(
            200,
            r#"{"status":"OK","total":250,"files":[{"id":1,"name":"a.mkv","file_type":"VIDEO","size":5,"created_at":"2024-05-01T10:00:00"},{"id":2,"name":"Show","file_type":"FOLDER"}]}"#,
        );
        let found = client(&fake).search_files("  spring & co ").unwrap();
        assert_eq!(
            fake.last_request().unwrap().describe(),
            "GET /files/search?query=spring%20%26%20co&per_page=100"
        );
        assert_eq!(found.files.len(), 2);
        assert_eq!(found.total, 250);
        assert_eq!(
            found.files[0].created_at.as_deref(),
            Some("2024-05-01T10:00:00")
        );
        // A reply without a total counts what it holds; a total below that cannot be right.
        fake.push_json(200, r#"{"files":[{"id":1},{"id":2}],"total":1}"#);
        assert_eq!(client(&fake).search_files("x").unwrap().total, 2);
        // Nothing is sent for an empty question.
        let before = fake.requests().len();
        assert_eq!(
            client(&fake).search_files("   ").unwrap_err(),
            ApiError::InvalidInput("search is empty")
        );
        assert_eq!(fake.requests().len(), before);
    }

    #[test]
    fn renaming_moving_deleting_and_making_folders_send_their_fields() {
        let fake = FakeTransport::new();
        for _ in 0..4 {
            fake.push_json(200, r#"{"status":"OK"}"#);
        }
        let c = client(&fake);
        c.rename_file(7, "Season 1 & 2").unwrap();
        c.move_files(&[3, 9], 42).unwrap();
        c.delete_files(&[3, 9]).unwrap();
        c.create_folder("New / folder", 0).unwrap();
        let sent: Vec<(String, Body)> = fake
            .requests()
            .into_iter()
            .map(|r| (r.describe(), r.body))
            .collect();
        assert_eq!(
            sent,
            [
                (
                    "POST /files/rename".to_owned(),
                    Body::Form("file_id=7&name=Season%201%20%26%202".into())
                ),
                (
                    "POST /files/move".to_owned(),
                    Body::Form("file_ids=3%2C9&parent_id=42".into())
                ),
                (
                    "POST /files/delete".to_owned(),
                    Body::Form("file_ids=3%2C9".into())
                ),
                (
                    "POST /files/create-folder".to_owned(),
                    Body::Form("name=New%20%2F%20folder&parent_id=0".into())
                ),
            ]
        );
    }

    #[test]
    fn file_changes_without_a_target_or_a_name_are_refused_before_anything_is_sent() {
        let fake = FakeTransport::new();
        let c = client(&fake);
        assert_eq!(
            c.rename_file(1, " ").unwrap_err(),
            ApiError::InvalidInput("name is empty")
        );
        assert_eq!(
            c.create_folder("", 0).unwrap_err(),
            ApiError::InvalidInput("name is empty")
        );
        assert_eq!(
            c.move_files(&[], 0).unwrap_err(),
            ApiError::InvalidInput("no file ids")
        );
        assert_eq!(
            c.delete_files(&[]).unwrap_err(),
            ApiError::InvalidInput("no file ids")
        );
        assert_eq!(fake.requests().len(), 0);
    }

    #[test]
    fn a_refused_file_change_comes_back_with_its_reason() {
        let fake = FakeTransport::new();
        fake.push_json(
            400,
            r#"{"status":"ERROR","error_type":"NAME_ALREADY_EXISTS","error_message":"exists"}"#,
        );
        let error = client(&fake).rename_file(1, "a").unwrap_err();
        assert_eq!(error.error_type(), Some("NAME_ALREADY_EXISTS"));
        fake.push_json(401, r#"{"error_type":"invalid_scope"}"#);
        assert_eq!(
            client(&fake).delete_files(&[1]).unwrap_err(),
            ApiError::Unauthorized
        );
    }

    #[test]
    fn a_folders_name_is_read_from_its_properties() {
        let fake = FakeTransport::new();
        fake.push_json(
            200,
            r#"{"status":"OK","file":{"id":42,"name":"TV Shows","file_type":"FOLDER","parent_id":0}}"#,
        );
        let folder = client(&fake).file(42).unwrap();
        assert_eq!(fake.last_request().unwrap().describe(), "GET /files/42");
        assert_eq!((folder.id, folder.name.as_str()), (42, "TV Shows"));
        // A reply without the file is an error, not an empty folder.
        fake.push_json(200, r#"{"status":"OK"}"#);
        assert!(matches!(client(&fake).file(42), Err(ApiError::Decode(_))));
    }

    #[test]
    fn the_feed_list_is_read_with_everything_put_io_sends() {
        let fake = FakeTransport::new();
        fake.push_json(200, include_str!("../tests/fixtures/rss_list.json"));
        let feeds = client(&fake).rss_feeds().unwrap();
        assert_eq!(fake.last_request().unwrap().describe(), "GET /rss/list");
        assert_eq!(feeds.len(), 2);
        let first = &feeds[0];
        assert_eq!(first.id, 5);
        assert_eq!(first.title, "Example releases");
        assert_eq!(first.rss_source_url, "https://example.invalid/releases.xml");
        assert_eq!(first.parent_dir_id, Some(123));
        assert_eq!(first.keyword.as_deref(), Some("1080p, 2160p"));
        assert_eq!(first.unwanted_keywords.as_deref(), Some("CAM, HDTS"));
        assert!(!first.paused && !first.delete_old_files);
        assert_eq!(first.last_fetch.as_deref(), Some("2024-05-03T08:30:00"));
        assert_eq!(first.last_error, None);
        let second = &feeds[1];
        assert!(second.paused && second.delete_old_files);
        assert_eq!(second.keyword, None);
        assert_eq!(second.last_error.as_deref(), Some("FEED_CANNOT_FETCHED"));
    }

    #[test]
    fn an_odd_value_in_a_feed_never_loses_the_list() {
        let fake = FakeTransport::new();
        fake.push_json(
            200,
            r#"{"feeds":[
                {"id":1,"title":null,"rss_source_url":7,"parent_dir_id":"x","paused":"yes","keyword":["a"],"brand_new":{}},
                {"id":2,"title":"fine"}
            ]}"#,
        );
        let feeds = client(&fake).rss_feeds().unwrap();
        assert_eq!(feeds.len(), 2);
        assert_eq!(feeds[0].title, "");
        assert_eq!(feeds[0].rss_source_url, "");
        assert_eq!(feeds[0].parent_dir_id, None);
        assert!(!feeds[0].paused);
        assert_eq!(feeds[0].keyword, None);
        assert_eq!(feeds[1].title, "fine");
        // No feeds at all is an empty list, not an error.
        fake.push_json(200, r#"{"status":"OK"}"#);
        assert_eq!(client(&fake).rss_feeds().unwrap(), []);
    }

    #[test]
    fn creating_a_feed_sends_its_fields_encoded() {
        let fake = FakeTransport::new();
        fake.push_json(200, r#"{"status":"OK","feed":{"id":9}}"#);
        client(&fake).create_rss_feed(&feed()).unwrap();
        let request = fake.last_request().unwrap();
        assert_eq!(request.describe(), "POST /rss/create");
        assert_eq!(
            request.body,
            Body::Form(
                "title=Spring%20%26%20Co&rss_source_url=https%3A%2F%2Fexample.invalid%2Ffeed.xml%3Fa%3D1%26b%3D2\
                 &parent_dir_id=42&keyword=1080p%2C%202160p&unwanted_keywords=\
                 &dont_process_whole_feed=false"
                    .into()
            )
        );
        // The checkbox is sent the way a browser sends it, and only when it is on.
        let mut more = feed();
        more.delete_old_files = true;
        more.skip_existing = true;
        fake.push_json(200, r#"{"status":"OK"}"#);
        client(&fake).create_rss_feed(&more).unwrap();
        let Body::Form(body) = fake.last_request().unwrap().body else {
            panic!("a form");
        };
        assert!(
            body.ends_with("&dont_process_whole_feed=true&delete_old_files=on"),
            "{body}"
        );
    }

    #[test]
    fn updating_a_feed_names_it_and_says_yes_or_no_for_the_housekeeping() {
        let fake = FakeTransport::new();
        fake.push_json(200, r#"{"status":"OK"}"#);
        client(&fake).update_rss_feed(5, &feed()).unwrap();
        let request = fake.last_request().unwrap();
        assert_eq!(request.describe(), "POST /rss/5");
        let Body::Form(body) = request.body else {
            panic!("a form");
        };
        assert!(body.ends_with("&delete_old_files=false"), "{body}");
        assert!(!body.contains("dont_process_whole_feed"), "{body}");
        let mut on = feed();
        on.delete_old_files = true;
        fake.push_json(200, r#"{"status":"OK"}"#);
        client(&fake).update_rss_feed(5, &on).unwrap();
        let Body::Form(body) = fake.last_request().unwrap().body else {
            panic!("a form");
        };
        assert!(body.ends_with("&delete_old_files=true"), "{body}");
    }

    #[test]
    fn a_feed_without_an_address_or_a_title_is_refused_before_anything_is_sent() {
        let fake = FakeTransport::new();
        let c = client(&fake);
        let mut no_url = feed();
        no_url.url = "  ".into();
        assert_eq!(
            c.create_rss_feed(&no_url).unwrap_err(),
            ApiError::InvalidInput("feed address is empty")
        );
        let mut no_title = feed();
        no_title.title.clear();
        assert_eq!(
            c.update_rss_feed(5, &no_title).unwrap_err(),
            ApiError::InvalidInput("feed title is empty")
        );
        assert_eq!(fake.requests().len(), 0);
    }

    #[test]
    fn pause_resume_and_delete_are_posts_without_a_body() {
        let fake = FakeTransport::new();
        let c = client(&fake);
        for _ in 0..3 {
            fake.push_json(200, r#"{"status":"OK"}"#);
        }
        c.set_rss_feed_paused(5, true).unwrap();
        c.set_rss_feed_paused(5, false).unwrap();
        c.delete_rss_feed(5).unwrap();
        let requests = fake.requests();
        let sent: Vec<String> = requests.iter().map(Request::describe).collect();
        assert_eq!(
            sent,
            [
                "POST /rss/5/pause",
                "POST /rss/5/resume",
                "POST /rss/5/delete"
            ]
        );
        assert!(requests.iter().all(|r| r.body == Body::None));
    }

    #[test]
    fn put_io_refusing_a_feed_comes_back_with_its_reason() {
        let fake = FakeTransport::new();
        fake.push_json(
            400,
            r#"{"status":"ERROR","error_type":"FEED_CANNOT_FETCHED","error_message":"could not fetch"}"#,
        );
        let error = client(&fake).create_rss_feed(&feed()).unwrap_err();
        assert_eq!(error.error_type(), Some("FEED_CANNOT_FETCHED"));
        // An error inside a 200 is an error too.
        fake.push_json(
            200,
            r#"{"status":"ERROR","error_type":"URL_NOT_VALID","error_message":"bad url"}"#,
        );
        let error = client(&fake).update_rss_feed(5, &feed()).unwrap_err();
        assert_eq!(error.error_type(), Some("URL_NOT_VALID"));
        // A rejected token is its own error.
        fake.push_json(401, r#"{"error_type":"invalid_scope"}"#);
        assert_eq!(
            client(&fake).delete_rss_feed(5).unwrap_err(),
            ApiError::Unauthorized
        );
    }

    #[test]
    fn clean_sends_the_ids_and_refuses_an_empty_list() {
        let fake = FakeTransport::new();
        fake.push_json(200, r#"{"status":"OK"}"#);
        let c = client(&fake);
        c.clean_transfers(&[3, 9, 12]).unwrap();
        assert_eq!(
            fake.last_request().unwrap().describe(),
            "POST /transfers/clean"
        );
        assert_eq!(
            fake.last_request().unwrap().body,
            Body::Form("transfer_ids=3%2C9%2C12".into())
        );
        assert_eq!(
            c.clean_transfers(&[]).unwrap_err(),
            ApiError::InvalidInput("no transfer ids")
        );
        assert_eq!(
            fake.requests().len(),
            1,
            "an empty list must not reach the network"
        );
    }

    #[test]
    fn transfer_source_is_read_when_present() {
        let fake = FakeTransport::new();
        fake.push_json(200, include_str!("../tests/fixtures/transfers_list.json"));
        let transfers = client(&fake).transfers().unwrap();
        assert!(
            transfers[0]
                .source
                .as_deref()
                .is_some_and(|s| s.starts_with("magnet:?xt=urn:btih:"))
        );
        assert_eq!(transfers[1].source, None);
    }

    #[test]
    fn transfer_details_are_read_when_present_and_absent_otherwise() {
        let fake = FakeTransport::new();
        fake.push_json(200, include_str!("../tests/fixtures/transfers_list.json"));
        let transfers = client(&fake).transfers().unwrap();
        assert_eq!(transfers[0].peers, Some(12));
        assert_eq!(transfers[0].availability, Some(100));
        assert_eq!(transfers[0].current_ratio, None);
        let done = &transfers[1];
        assert_eq!(done.uploaded, Some(209_715_200));
        assert_eq!(done.downloaded, Some(104_857_600));
        assert_eq!(done.current_ratio, Some(2.0));
        assert_eq!(done.seconds_seeding, Some(3600));
        assert_eq!(done.is_private, Some(false));
        assert_eq!(done.tracker_message.as_deref(), Some("Announce OK"));
        assert_eq!(done.created_at.as_deref(), Some("2024-05-01T12:34:56"));
        assert_eq!(done.finished_at.as_deref(), Some("2024-05-01T13:00:00"));
    }

    #[test]
    fn a_detail_of_the_wrong_type_is_dropped_without_losing_the_transfer() {
        let fake = FakeTransport::new();
        fake.push_json(
            200,
            r#"{"status":"OK","transfers":[{"id":5,"name":"x","status":"SEEDING","peers":"many","availability":1.5,
               "current_ratio":"high","is_private":"yes","tracker_message":["a"],"created_at":12,"uploaded":null}]}"#,
        );
        let transfers = client(&fake).transfers().unwrap();
        let t = &transfers[0];
        assert_eq!((t.id, t.name.as_str()), (5, "x"));
        assert_eq!(t.peers, None);
        assert_eq!(t.availability, None);
        assert_eq!(t.current_ratio, None);
        assert_eq!(t.is_private, None);
        assert_eq!(t.tracker_message, None);
        assert_eq!(t.created_at, None);
        assert_eq!(t.uploaded, None);
    }

    #[test]
    fn error_statuses_map_to_distinct_errors() {
        let fake = FakeTransport::new();
        fake.push_json(401, r#"{"error_type":"invalid_grant"}"#);
        fake.push_response(429, &[("X-RateLimit-Reset", "30")], "");
        fake.push_json(404, include_str!("../tests/fixtures/error_not_found.json"));
        fake.push_json(500, "<html>boom</html>");
        fake.push_json(200, include_str!("../tests/fixtures/error_not_found.json"));
        let c = client(&fake);
        assert_eq!(c.account().unwrap_err(), ApiError::Unauthorized);
        assert_eq!(
            c.account().unwrap_err(),
            ApiError::RateLimited {
                retry_after: Duration::from_secs(30)
            }
        );
        let not_found = c.account().unwrap_err();
        assert_eq!(not_found.error_type(), Some("NotFound"));
        assert!(
            matches!(c.account().unwrap_err(), ApiError::Api(f) if f.status == 500 && f.error_type.is_none())
        );
        // 200 with {"status":"ERROR"} is still an error.
        assert_eq!(c.account().unwrap_err().error_type(), Some("NotFound"));
    }

    #[test]
    fn garbage_and_network_errors() {
        let fake = FakeTransport::new();
        fake.push_json(200, "not json");
        fake.push_json(200, r#"{"info": "wrong shape"}"#);
        fake.push_error(TransportError::Network("dns".into()));
        fake.push_error(TransportError::TooLarge);
        let c = client(&fake);
        assert!(matches!(c.account().unwrap_err(), ApiError::Decode(_)));
        assert!(matches!(c.account().unwrap_err(), ApiError::Decode(_)));
        assert_eq!(c.account().unwrap_err(), ApiError::Network("dns".into()));
        assert_eq!(c.account().unwrap_err(), ApiError::TooLarge);
    }

    #[test]
    fn oob_poll_pending_then_linked() {
        let fake = FakeTransport::new();
        fake.push_json(200, r#"{"status":"OK","oauth_token":null}"#);
        fake.push_json(200, r#"{"status":"OK","oauth_token":"NEWTOKEN"}"#);
        let c = Client::new(&fake);
        assert!(c.oob_poll("ABCD1234").unwrap().is_none());
        assert_eq!(
            c.oob_poll("ABCD1234").unwrap().unwrap().expose(),
            "NEWTOKEN"
        );
        assert_eq!(
            fake.last_request().unwrap().path,
            "/oauth2/oob/code/ABCD1234"
        );
    }

    #[test]
    fn oob_poll_rejects_codes_that_could_alter_the_path() {
        let c = Client::new(FakeTransport::new());
        for bad in [
            "",
            "../account/info",
            "AB/CD",
            "AB?x=1",
            "AB CD",
            &"A".repeat(65),
        ] {
            assert!(
                matches!(c.oob_poll(bad), Err(ApiError::InvalidInput(_))),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn oob_poll_rejects_a_token_that_is_not_header_safe() {
        let fake = FakeTransport::new();
        fake.push_json(200, r#"{"oauth_token":"bad\r\nX-Evil: 1"}"#);
        assert!(matches!(
            Client::new(&fake).oob_poll("ABCD").unwrap_err(),
            ApiError::Decode(_)
        ));
    }

    #[test]
    fn list_files_follows_cursors_and_filters() {
        let fake = FakeTransport::new();
        fake.push_json(200, include_str!("../tests/fixtures/files_list_page1.json"));
        fake.push_json(200, include_str!("../tests/fixtures/files_list_page2.json"));
        let listing = client(&fake).list_files(0, Some(FileType::Folder)).unwrap();
        let names: Vec<_> = listing.files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["Movies", "Music", "Shows"]);
        assert_eq!(listing.parent.unwrap().name, "Your Files");
        let requests = fake.requests();
        assert_eq!(
            requests[0].path,
            "/files/list?parent_id=0&per_page=1000&file_type=FOLDER"
        );
        assert_eq!(requests[1].describe(), "POST /files/list/continue");
        assert_eq!(
            requests[1].body,
            Body::Form("cursor=abc123&per_page=1000".into())
        );
    }

    #[test]
    fn list_files_stops_after_the_page_cap() {
        let fake = FakeTransport::new();
        for _ in 0..(MAX_PAGES + 5) {
            fake.push_json(200, r#"{"files":[],"cursor":"again"}"#);
        }
        client(&fake).list_files(0, None).unwrap();
        assert_eq!(fake.requests().len(), MAX_PAGES);
    }

    #[test]
    fn upload_goes_to_the_upload_host_as_multipart() {
        let fake = FakeTransport::new();
        fake.push_json(
            200,
            r#"{"status":"OK","transfer":{"id":5,"name":"t","status":"IN_QUEUE"}}"#,
        );
        let outcome = client(&fake)
            .upload_torrent("a.torrent", b"d4:infod4:name1:aee", Some(9))
            .unwrap();
        assert!(matches!(outcome, UploadOutcome::Transfer(t) if t.id == 5));
        let request = fake.last_request().unwrap();
        assert_eq!(request.host, Host::Upload);
        assert_eq!(request.path, "/files/upload");
        let Body::Raw {
            content_type,
            bytes,
        } = request.body
        else {
            panic!("expected raw body")
        };
        assert!(content_type.starts_with("multipart/form-data; boundary="));
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("name=\"parent_id\"\r\n\r\n9"));
        assert!(text.contains("filename=\"a.torrent\""));
    }

    #[test]
    fn upload_refuses_non_torrents_without_touching_the_network() {
        let fake = FakeTransport::new();
        let c = client(&fake);
        assert!(matches!(
            c.upload_torrent("a.torrent", b"<html>", None),
            Err(ApiError::InvalidInput(_))
        ));
        assert!(matches!(
            c.upload_torrent("a.torrent", b"", None),
            Err(ApiError::InvalidInput(_))
        ));
        assert!(fake.requests().is_empty());
    }

    #[test]
    fn creating_a_zip_sends_the_ids_and_returns_the_zip_id() {
        let fake = FakeTransport::new();
        fake.push_json(200, r#"{"status":"OK","zip_id":42}"#);
        let c = client(&fake);
        assert_eq!(c.create_zip(&[5, 6]).unwrap(), 42);
        let request = &fake.requests()[0];
        assert_eq!(request.method, Method::Post);
        assert_eq!(request.path, "/zips/create");
        assert_eq!(request.body, Body::Form("file_ids=5%2C6".into()));
        assert!(matches!(c.create_zip(&[]), Err(ApiError::InvalidInput(_))));
    }

    #[test]
    fn zip_status_reads_the_state_the_url_and_the_missing_files() {
        let fake = FakeTransport::new();
        fake.push_json(200, r#"{"zip_status":"PROCESSING"}"#);
        fake.push_json(
            200,
            r#"{"zip_status":"DONE","url":"https://s1.put.io/z","size":10,"missing_files":[{"id":1,"name":"a","missing":true}]}"#,
        );
        fake.push_json(
            200,
            r#"{"zip_status":"EXPLODED","url":7,"missing_files":"x"}"#,
        );
        let c = client(&fake);
        let busy = c.zip_status(42).unwrap();
        assert_eq!(busy.zip_status, ZipState::Processing);
        assert_eq!(busy.url, None);
        assert_eq!(fake.requests()[0].path, "/zips/42");
        let done = c.zip_status(42).unwrap();
        assert_eq!(done.zip_status, ZipState::Done);
        assert_eq!(done.url.as_deref(), Some("https://s1.put.io/z"));
        assert_eq!(done.missing_count(), 1);
        let odd = c.zip_status(42).unwrap();
        assert_eq!(odd.zip_status, ZipState::Unknown);
        assert_eq!((odd.url.as_deref(), odd.missing_count()), (None, 0));
    }

    #[test]
    fn file_url_must_be_https() {
        let fake = FakeTransport::new();
        fake.push_json(200, r#"{"url":"http://insecure.example/f"}"#);
        assert!(matches!(
            client(&fake).file_url(1),
            Err(ApiError::Decode(_))
        ));
    }

    #[test]
    fn latest_release_goes_to_github_without_the_put_io_token() {
        let fake = FakeTransport::new();
        fake.push_json(200, r#"{"tag_name":"v1.2.3","html_url":"https://github.com/nicoSWD/put.io-adder/releases/tag/v1.2.3","prerelease":false,"draft":false}"#);
        fake.push_json(
            200,
            r#"{"tag_name":"v9","html_url":"https://evil.example/x"}"#,
        );
        let c = client(&fake);
        let release = c.latest_release().unwrap();
        assert_eq!(release.tag_name, "v1.2.3");
        let request = &fake.requests()[0];
        assert_eq!(request.host, Host::GitHub);
        assert_eq!(request.path, "/repos/nicoSWD/put.io-adder/releases/latest");
        assert!(
            request.bearer.is_none(),
            "the put.io token must never go to GitHub"
        );
        assert!(matches!(c.latest_release(), Err(ApiError::Decode(_))));
    }
}
