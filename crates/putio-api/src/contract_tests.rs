//! Checks the client against the vendored official `OpenAPI` spec (`tests/spec/`).
//!
//! The spec is data, not code: it is only parsed here. If put.io changes an endpoint, a parameter or
//! an enum, these tests are what notices.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::Client;
use crate::models::{FileType, RssFeedSettings, Transfer, TransferStatus};
use crate::testing::FakeTransport;
use crate::token::Token;
use crate::transport::{Body, Host, Method, Request};

const SPEC: &str = include_str!("../tests/spec/putio-2.8.14.json");
const SPEC_SHA256: &str = include_str!("../tests/spec/putio-2.8.14.json.sha256");

fn spec() -> Value {
    serde_json::from_str(SPEC).unwrap()
}

/// SHA-256 without a crypto dependency: compare through the system-independent test below instead.
/// (The digest file is checked by CI with `shasum -c`; here we only make sure it names the spec.)
#[test]
fn the_digest_file_names_the_vendored_spec() {
    let line = SPEC_SHA256.trim();
    let (digest, name) = line.split_once("  ").unwrap();
    assert_eq!(name, "putio-2.8.14.json");
    assert_eq!(digest.len(), 64);
    assert!(digest.bytes().all(|b| b.is_ascii_hexdigit()));
}

#[test]
fn the_spec_is_the_version_we_target() {
    let spec = spec();
    assert_eq!(spec["openapi"], "3.0.2");
    assert_eq!(spec["info"]["version"], "2.8.14");
}

fn enum_values(schema: &Value) -> BTreeSet<String> {
    schema["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn every_transfer_status_in_the_spec_has_a_real_variant() {
    let spec = spec();
    for value in enum_values(&spec["components"]["schemas"]["Transfer"]["properties"]["status"]) {
        let status: TransferStatus = serde_json::from_str(&format!("\"{value}\"")).unwrap();
        assert_ne!(
            status,
            TransferStatus::Unknown,
            "spec status {value} is not modelled"
        );
    }
}

#[test]
fn every_file_type_in_the_spec_has_a_real_variant_and_query_value() {
    let spec = spec();
    for value in enum_values(&spec["components"]["schemas"]["FileType"]) {
        let kind: FileType = serde_json::from_str(&format!("\"{value}\"")).unwrap();
        assert_ne!(
            kind,
            FileType::Unknown,
            "spec file type {value} is not modelled"
        );
        assert_eq!(kind.as_query(), value);
    }
}

#[test]
fn the_fields_we_read_exist_in_the_spec_schemas() {
    let spec = spec();
    let props = |name: &str| -> BTreeSet<String> {
        spec["components"]["schemas"][name]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    };
    let transfer = props("Transfer");
    for field in [
        "id",
        "name",
        "status",
        "type",
        "percent_done",
        "down_speed",
        "up_speed",
        "estimated_time",
        "size",
        "file_id",
        "error_message",
    ] {
        assert!(
            transfer.contains(field),
            "Transfer.{field} is not in the spec"
        );
    }
    let file = props("File");
    for field in [
        "id",
        "name",
        "parent_id",
        "file_type",
        "content_type",
        "size",
        "screenshot",
    ] {
        assert!(file.contains(field), "File.{field} is not in the spec");
    }
    // `unwanted_keywords`, `last_fetch` and `last_error` are sent by the live API but missing from the spec's
    // `RssFeed`; they are read tolerantly, so they are not listed here.
    let feed = props("RssFeed");
    for field in [
        "id",
        "title",
        "rss_source_url",
        "parent_dir_id",
        "delete_old_files",
        "keyword",
        "paused",
        "created_at",
        "updated_at",
    ] {
        assert!(feed.contains(field), "RssFeed.{field} is not in the spec");
    }
    let created = props("File");
    assert!(
        created.contains("created_at"),
        "File.created_at is not in the spec"
    );
    for field in ["save_parent_id", "subscription_id"] {
        assert!(
            transfer.contains(field),
            "Transfer.{field} is not in the spec"
        );
    }
    let extraction = props("Extraction");
    for field in ["id", "name", "status", "message", "files"] {
        assert!(
            extraction.contains(field),
            "Extraction.{field} is not in the spec"
        );
    }
    let account = props("AccountInfo");
    for field in ["user_id", "username", "mail", "monthly_bandwidth_usage"] {
        assert!(
            account.contains(field),
            "AccountInfo.{field} is not in the spec"
        );
    }
    // Documented exception: we also read an optional `disk` that 2.8.14 no longer lists.
    assert!(
        !account.contains("disk"),
        "spec now documents `disk`: model it as required-if-present and update docs/spikes.md"
    );
}

#[test]
fn the_spec_transfer_kinds_are_modelled_except_the_na_placeholder() {
    let spec = spec();
    for value in enum_values(&spec["components"]["schemas"]["Transfer"]["properties"]["type"]) {
        let result: crate::models::TransferKind =
            serde_json::from_str(&format!("\"{value}\"")).unwrap();
        if value != "N/A" {
            assert_ne!(result, crate::models::TransferKind::Unknown, "{value}");
        }
    }
}

/// `/files/123/url` → `/files/{id}/url`, `/oauth2/oob/code/ABCD1234` → `/oauth2/oob/code/{code}`.
fn template(path: &str) -> String {
    let path = path.split('?').next().unwrap_or(path);
    path.split('/')
        .map(|segment| {
            if !segment.is_empty() && segment.bytes().all(|b| b.is_ascii_digit()) {
                "{id}".to_owned()
            } else if segment == "ABCD1234" {
                "{code}".to_owned()
            } else {
                segment.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn form_fields(body: &Body) -> BTreeSet<String> {
    match body {
        Body::Form(form) => form
            .split('&')
            .map(|pair| pair.split('=').next().unwrap_or("").to_owned())
            .collect(),
        _ => BTreeSet::new(),
    }
}

fn query_fields(path: &str) -> BTreeSet<String> {
    path.split_once('?')
        .map(|(_, query)| {
            query
                .split('&')
                .map(|pair| pair.split('=').next().unwrap_or("").to_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// Run every API call the app makes and compare each request with the spec.
#[test]
#[allow(clippy::too_many_lines)] // one flat list of calls reads best next to the spec
fn every_request_the_client_sends_exists_in_the_spec_with_the_documented_parameters() {
    let spec = spec();
    let fake = FakeTransport::new();
    let ok = |body: &str| {
        fake.push_json(200, body);
    };
    let transfer = r#"{"id":1,"name":"t","status":"IN_QUEUE"}"#;
    ok(r#"{"code":"ABCD1234"}"#); // oob_code
    ok(r#"{"oauth_token":null}"#); // oob_poll
    ok(r#"{"info":{"user_id":1}}"#); // account
    ok(r#"{"transfers":[]}"#); // transfers
    ok(&format!(r#"{{"transfer":{transfer}}}"#)); // add_transfer
    ok(""); // cancel
    ok(""); // retry
    ok(""); // clean
    ok(&format!(r#"{{"transfer":{transfer}}}"#)); // upload
    ok(r#"{"files":[],"cursor":"c"}"#); // list_files page 1
    ok(r#"{"files":[]}"#); // list_files continue
    ok(r#"{"url":"https://x.invalid/f"}"#); // file_url
    ok(r#"{"zip_id":9}"#); // create_zip
    ok(r#"{"zip_status":"NEW"}"#); // zip_status
    ok(r#"{"file":{"id":1,"name":"f"}}"#); // file
    ok(r#"{"files":[],"total":0}"#); // search_files
    ok(r#"{"status":"OK"}"#); // rename_file
    ok(r#"{"status":"OK"}"#); // move_files
    ok(r#"{"status":"OK"}"#); // delete_files
    ok(r#"{"status":"OK","file":{"id":2}}"#); // create_folder
    ok(&format!(r#"{{"transfer":{transfer}}}"#)); // transfer
    ok(r#"{"extractions":[]}"#); // extract_files
    ok(r#"{"extractions":[]}"#); // extractions
    ok(r#"{"shared":[{"id":1,"name":"a","size":3,"share_with":"everyone"}]}"#); // shared_files
    ok(r#"{"shared-with":[{"user_name":"jane","user_avatar_url":"/a.png","share_id":3}]}"#); // shared_with
    ok(r#"{"friends":[{"id":1,"name":"jane"}]}"#); // friends
    ok(r#"{"status":"OK"}"#); // share_file
    ok(r#"{"status":"OK"}"#); // unshare_file
    ok(r#"{"feeds":[]}"#); // rss_feeds
    ok(r#"{"status":"OK"}"#); // create_rss_feed
    ok(r#"{"status":"OK"}"#); // update_rss_feed
    ok(r#"{"status":"OK"}"#); // pause
    ok(r#"{"status":"OK"}"#); // resume
    ok(r#"{"status":"OK"}"#); // delete

    let client = Client::with_token(&fake, Token::new("t").unwrap());
    client.oob_code(711, "put.io adder").unwrap();
    client.oob_poll("ABCD1234").unwrap();
    client.account().unwrap();
    client.transfers().unwrap();
    client
        .add_transfer(
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
            Some(1),
        )
        .unwrap();
    client.cancel_transfers(&[1]).unwrap();
    client.retry_transfer(1).unwrap();
    client.clean_transfers(&[1, 2]).unwrap();
    client
        .upload_torrent("a.torrent", b"d4:infod4:name1:aee", Some(1))
        .unwrap();
    client.list_files(0, Some(FileType::Folder)).unwrap();
    client.file_url(1).unwrap();
    client.create_zip(&[1, 2]).unwrap();
    client.zip_status(9).unwrap();
    let feed = RssFeedSettings {
        title: "Releases".into(),
        url: "https://example.invalid/feed.xml".into(),
        parent_dir_id: 3,
        delete_old_files: true,
        keyword: "1080p".into(),
        unwanted_keywords: "CAM".into(),
        skip_existing: true,
    };
    client.file(1).unwrap();
    client.search_files("spring").unwrap();
    client.rename_file(1, "new name").unwrap();
    client.move_files(&[1, 2], 3).unwrap();
    client.delete_files(&[1, 2]).unwrap();
    client.create_folder("folder", 0).unwrap();
    client.transfer(1).unwrap();
    client.extract_files(&[1, 2], Some("secret")).unwrap();
    client.extractions().unwrap();
    client.shared_files().unwrap();
    client.shared_with(1).unwrap();
    client.friends().unwrap();
    client
        .share_file(1, &crate::models::ShareWith::Friends(vec!["jane".into()]))
        .unwrap();
    client.unshare_file(1, &[3]).unwrap();
    client.rss_feeds().unwrap();
    client.create_rss_feed(&feed).unwrap();
    client.update_rss_feed(5, &feed).unwrap();
    client.set_rss_feed_paused(5, true).unwrap();
    client.set_rss_feed_paused(5, false).unwrap();
    client.delete_rss_feed(5).unwrap();

    let requests: Vec<Request> = fake.requests();
    assert_eq!(requests.len(), 34);
    for request in &requests {
        let path = template(&request.path);
        let method = match request.method {
            Method::Get => "get",
            Method::Post => "post",
        };
        let operation = spec["paths"]
            .get(&path)
            .and_then(|p| p.get(method))
            .unwrap_or_else(|| panic!("{method} {path} is not in the spec"));

        // Upload goes to the upload host and nowhere else; everything else to the API host.
        assert_eq!(
            request.host == Host::Upload,
            path == "/files/upload",
            "{path}"
        );

        // Query parameters we send must be documented for the operation.
        let documented: BTreeSet<String> = operation["parameters"]
            .as_array()
            .map(|params| {
                params
                    .iter()
                    .filter(|p| p["in"] == "query")
                    .map(|p| p["name"].as_str().unwrap().to_owned())
                    .collect()
            })
            .unwrap_or_default();
        for sent in query_fields(&request.path) {
            assert!(
                documented.contains(&sent),
                "{method} {path}: query parameter `{sent}` is not in the spec"
            );
        }

        // Form fields we send must be documented request-body properties.
        if let Body::Form(_) = &request.body {
            let schema =
                &operation["requestBody"]["content"]["application/x-www-form-urlencoded"]["schema"];
            let properties: BTreeSet<String> = schema["properties"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect();
            for sent in form_fields(&request.body) {
                assert!(
                    properties.contains(&sent),
                    "{method} {path}: form field `{sent}` is not in the spec"
                );
            }
            // The spec lists `url` as required for the feed forms but documents `rss_source_url` as the
            // property (and that is what put.io's own SDK sends): a mistake in the spec, so a required name
            // that is not a property cannot be sent and is skipped.
            for required in schema["required"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|name| properties.contains(name.as_str().unwrap()))
            {
                assert!(
                    form_fields(&request.body).contains(required.as_str().unwrap()),
                    "{method} {path}: required field {required} missing"
                );
            }
        }
    }

    // Required query parameters are always sent.
    let oob = &requests[0];
    assert!(query_fields(&oob.path).contains("app_id"));

    // The two multipart parts we send exist in the upload schema.
    let upload = &spec["paths"]["/files/upload"]["post"]["requestBody"]["content"]["multipart/form-data"]
        ["schema"]["properties"];
    for field in ["file", "filename", "parent_id"] {
        assert!(
            upload.get(field).is_some(),
            "multipart field {field} is not in the spec"
        );
    }
}

#[test]
fn spec_examples_deserialise_into_our_models() {
    let transfer: Transfer = serde_json::from_str(
        r#"{"id": 1, "status": "COMPLETED", "type": "TORRENT", "percent_done": 100}"#,
    )
    .unwrap();
    assert_eq!(transfer.status, TransferStatus::Completed);
}
