//! The typed put.io client: one method per endpoint the app uses.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::{ApiError, ApiFailure, retry_delay};
use crate::form::{self, Part};
use crate::models::{Account, DeviceCode, File, FileType, Release, Transfer, UploadOutcome};
use crate::token::Token;
use crate::torrent;
use crate::transport::{Body, Host, Method, Request, Response, Transport};

const JSON: &str = "application/json";
const GITHUB_JSON: &str = "application/vnd.github+json";
/// GitHub project the update check reads releases from.
const GITHUB_REPO: &str = "nicoSWD/putio-adder-rs";
/// Page size for file listings (spec maximum is 1000).
const PER_PAGE: &str = "1000";
/// Upper bound on pages followed for one listing, so a misbehaving cursor cannot loop forever.
const MAX_PAGES: usize = 50;

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
            (Some(transfer), _) => Ok(UploadOutcome::Transfer(transfer)),
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
        fake.push_json(200, r#"{"tag_name":"v1.2.3","html_url":"https://github.com/nicoSWD/putio-adder-rs/releases/tag/v1.2.3","prerelease":false,"draft":false}"#);
        fake.push_json(
            200,
            r#"{"tag_name":"v9","html_url":"https://evil.example/x"}"#,
        );
        let c = client(&fake);
        let release = c.latest_release().unwrap();
        assert_eq!(release.tag_name, "v1.2.3");
        let request = &fake.requests()[0];
        assert_eq!(request.host, Host::GitHub);
        assert_eq!(
            request.path,
            "/repos/nicoSWD/putio-adder-rs/releases/latest"
        );
        assert!(
            request.bearer.is_none(),
            "the put.io token must never go to GitHub"
        );
        assert!(matches!(c.latest_release(), Err(ApiError::Decode(_))));
    }
}
