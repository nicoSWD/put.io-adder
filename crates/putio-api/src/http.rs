//! Production transport on top of `ureq` + rustls.

use std::time::Duration;

use crate::transport::{Body, Host, Method, Request, Response, Transport, TransportError};

/// Largest response body accepted (a 1000-entry file listing is well under 2 MiB).
pub const MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Uploads of a few MiB over a slow link need longer than API calls.
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(120);
/// The whole of a file upload: files are big and links are slow, so this is long; it only ends an upload
/// that never finishes.
const FILE_UPLOAD_TIMEOUT: Duration = Duration::from_hours(12);

/// Base URLs for each [`Host`].
#[derive(Debug, Clone)]
struct Bases {
    api: String,
    upload: String,
    upload_tus: String,
    github: String,
}

impl Bases {
    fn production() -> Self {
        Self {
            api: "https://api.put.io/v2".into(),
            upload: "https://upload.put.io/v2".into(),
            upload_tus: "https://upload.put.io".into(),
            github: "https://api.github.com".into(),
        }
    }

    fn url(&self, host: Host, path: &str) -> String {
        let base = match host {
            Host::Api => &self.api,
            Host::Upload => &self.upload,
            Host::UploadTus => &self.upload_tus,
            Host::GitHub => &self.github,
        };
        format!("{base}{path}")
    }
}

/// HTTPS-only transport with a fixed host allowlist (see [`Host`]), redirects disabled (a redirect
/// could carry the bearer token to another host), bounded bodies and timeouts.
#[derive(Debug)]
pub struct UreqTransport {
    agent: ureq::Agent,
    upload_agent: ureq::Agent,
    file_agent: ureq::Agent,
    bases: Bases,
}

fn agent(timeout: Duration, user_agent: &str, https_only: bool) -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .https_only(https_only)
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(timeout))
            .user_agent(user_agent)
            .build(),
    )
}

impl UreqTransport {
    /// `user_agent` is sent on every request (GitHub requires one), e.g. `"putio-adder/0.1.0"`.
    #[must_use]
    pub fn new(user_agent: &str) -> Self {
        Self {
            agent: agent(REQUEST_TIMEOUT, user_agent, true),
            upload_agent: agent(UPLOAD_TIMEOUT, user_agent, true),
            file_agent: agent(FILE_UPLOAD_TIMEOUT, user_agent, true),
            bases: Bases::production(),
        }
    }

    /// Plain-HTTP transport pointing every host at a loopback test server. Tests only.
    #[cfg(test)]
    pub(crate) fn for_local_tests(port: u16) -> Self {
        let base = format!("http://127.0.0.1:{port}");
        Self {
            agent: agent(Duration::from_secs(5), "putio-adder-test", false),
            upload_agent: agent(Duration::from_secs(5), "putio-adder-test", false),
            file_agent: agent(Duration::from_secs(5), "putio-adder-test", false),
            bases: Bases {
                api: base.clone(),
                upload: base.clone(),
                upload_tus: base.clone(),
                github: base,
            },
        }
    }
}

fn map_error(error: &ureq::Error) -> TransportError {
    match error {
        ureq::Error::BodyExceedsLimit(_) => TransportError::TooLarge,
        other => TransportError::Network(other.to_string()),
    }
}

fn with_headers<B>(
    builder: ureq::RequestBuilder<B>,
    accept: &str,
    authorization: Option<&str>,
    extra: &[(&'static str, String)],
) -> ureq::RequestBuilder<B> {
    let mut builder = builder.header("Accept", accept);
    for (name, value) in extra {
        builder = builder.header(*name, value);
    }
    match authorization {
        Some(value) => builder.header("Authorization", value),
        None => builder,
    }
}

/// Send a file body (a whole multipart upload, or one part of a resumable one) and turn a send that failed
/// because the file could not be read into a file error, not a network one.
fn send_file(
    builder: ureq::RequestBuilder<ureq::typestate::WithBody>,
    file: &crate::transport::FileBody,
) -> Result<Result<ureq::http::Response<ureq::Body>, ureq::Error>, TransportError> {
    let (reader, total) = FileReader::open(file)?;
    let failure = std::sync::Arc::clone(&reader.failure);
    let sent = builder
        .header("Content-Type", &file.content_type)
        .header("Content-Length", total.to_string())
        .send(ureq::SendBody::from_owned_reader(reader));
    let why = failure.lock().ok().and_then(|slot| slot.clone());
    if sent.is_err()
        && let Some(why) = why
    {
        return Err(TransportError::File(why));
    }
    Ok(sent)
}

/// The body of a file upload as one stream: the head, the file's bytes, the tail. Reads the file as it is
/// sent, tells how far it has got, and stops when asked to.
struct FileReader {
    head: std::io::Cursor<Vec<u8>>,
    file: std::fs::File,
    file_left: u64,
    tail: std::io::Cursor<Vec<u8>>,
    sent: u64,
    reported: u64,
    progress: Option<crate::transport::ProgressFn>,
    cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Why reading stopped, when it was the file's doing: the send then fails with an io error that
    /// says nothing of the cause, and this is what to tell instead.
    failure: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

/// How many bytes of the file go by between two reports of progress.
const REPORT_EVERY: u64 = 256 * 1024;

impl FileReader {
    /// Open the file and check it is what the request said: a regular file of the length it had.
    /// Returns the reader and the length of the whole body.
    fn open(body: &crate::transport::FileBody) -> Result<(Self, u64), TransportError> {
        let refuse = TransportError::File;
        let file = std::fs::File::open(&body.path)
            .map_err(|error| refuse(format!("cannot read the file: {error}")))?;
        let meta = file
            .metadata()
            .map_err(|error| refuse(format!("cannot read the file: {error}")))?;
        if !meta.is_file() {
            return Err(refuse("not a regular file".into()));
        }
        if meta.len() != body.len {
            return Err(refuse(
                "the file changed while it was being prepared".into(),
            ));
        }
        if body.offset.saturating_add(body.count) > body.len {
            return Err(refuse("the part to send is not inside the file".into()));
        }
        let mut file = file;
        {
            use std::io::Seek;
            file.seek(std::io::SeekFrom::Start(body.offset))
                .map_err(|error| refuse(format!("cannot read the file: {error}")))?;
        }
        let total = body.head.len() as u64 + body.count + body.tail.len() as u64;
        Ok((
            Self {
                head: std::io::Cursor::new(body.head.clone()),
                file,
                file_left: body.count,
                tail: std::io::Cursor::new(body.tail.clone()),
                sent: 0,
                reported: 0,
                progress: body.progress.clone(),
                cancel: body.cancel.clone(),
                failure: std::sync::Arc::default(),
            },
            total,
        ))
    }
}

impl FileReader {
    /// Remember why reading stopped, and make the io error that stops the send.
    fn fail(&self, kind: std::io::ErrorKind, why: String) -> std::io::Error {
        if let Ok(mut slot) = self.failure.lock() {
            *slot = Some(why.clone());
        }
        std::io::Error::new(kind, why)
    }
}

impl std::io::Read for FileReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        use std::sync::atomic::Ordering;
        if self
            .cancel
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
        {
            // Not `Interrupted`: readers retry that.
            return Err(std::io::Error::other("cancelled"));
        }
        let from_head = self.head.read(buf)?;
        if from_head > 0 {
            return Ok(from_head);
        }
        if self.file_left > 0 {
            let want =
                usize::try_from(self.file_left).map_or(buf.len(), |left| left.min(buf.len()));
            let Some(window) = buf.get_mut(..want) else {
                return Ok(0);
            };
            let n = match self.file.read(window) {
                Ok(0) => {
                    return Err(self.fail(
                        std::io::ErrorKind::UnexpectedEof,
                        "the file got shorter while it was being sent".into(),
                    ));
                }
                Ok(n) => n,
                Err(error) => {
                    return Err(self.fail(error.kind(), format!("cannot read the file: {error}")));
                }
            };
            self.file_left -= n as u64;
            self.sent += n as u64;
            if let Some(progress) = &self.progress
                && (self.file_left == 0 || self.sent - self.reported >= REPORT_EVERY)
            {
                self.reported = self.sent;
                progress(self.sent);
            }
            return Ok(n);
        }
        self.tail.read(buf)
    }
}

impl Transport for UreqTransport {
    fn execute(&self, request: &Request) -> Result<Response, TransportError> {
        let url = self.bases.url(request.host, &request.path);
        let agent = match (&request.body, request.host) {
            (Body::File(_), _) => &self.file_agent,
            (_, Host::Upload | Host::UploadTus) => &self.upload_agent,
            _ => &self.agent,
        };

        let authorization = request
            .bearer
            .as_ref()
            .map(|token| format!("Bearer {}", token.expose()));
        let authorization = authorization.as_deref();
        let accept = request.accept;

        let result = match (request.method, &request.body) {
            (Method::Get, _) => {
                with_headers(agent.get(&url), accept, authorization, &request.headers).call()
            }
            (Method::Post, Body::None) => {
                with_headers(agent.post(&url), accept, authorization, &request.headers).send_empty()
            }
            (Method::Post, Body::Form(form)) => {
                with_headers(agent.post(&url), accept, authorization, &request.headers)
                    .header("Content-Type", "application/x-www-form-urlencoded")
                    .send(form.as_str())
            }
            (
                Method::Post,
                Body::Raw {
                    content_type,
                    bytes,
                },
            ) => with_headers(agent.post(&url), accept, authorization, &request.headers)
                .header("Content-Type", content_type)
                .send(bytes.as_slice()),
            (Method::Post, Body::File(file)) => send_file(
                with_headers(agent.post(&url), accept, authorization, &request.headers),
                file,
            )?,
            (Method::Patch, Body::File(file)) => send_file(
                with_headers(agent.patch(&url), accept, authorization, &request.headers),
                file,
            )?,
            (Method::Head, _) => {
                with_headers(agent.head(&url), accept, authorization, &request.headers).call()
            }
            // The client never builds these; refusing is better than guessing what was meant.
            (Method::Patch, _) => {
                return Err(TransportError::Network(
                    "this method does not take that kind of body".into(),
                ));
            }
        };

        let mut response = result.map_err(|error| map_error(&error))?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                Some((
                    name.as_str().to_ascii_lowercase(),
                    value.to_str().ok()?.to_owned(),
                ))
            })
            .collect();
        let body = response
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE_BYTES)
            .read_to_vec()
            .map_err(|error| map_error(&error))?;
        Ok(Response {
            status,
            headers,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    use super::*;
    use crate::token::Token;

    /// One-shot HTTP server: answers the first request with `raw_response` and returns what it received.
    fn serve_once(raw_response: Vec<u8>) -> (u16, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut received = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = stream.read(&mut buf).unwrap();
                received.extend_from_slice(&buf[..n]);
                if n == 0 || received.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            tx.send(String::from_utf8_lossy(&received).into_owned())
                .unwrap();
            stream.write_all(&raw_response).unwrap();
        });
        (port, rx)
    }

    /// Answers one request after reading all of its body; returns the headers and the body it received.
    fn serve_upload() -> (u16, mpsc::Receiver<(String, Vec<u8>)>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut received = Vec::new();
            let mut buf = [0u8; 8192];
            let split = loop {
                let n = stream.read(&mut buf).unwrap();
                received.extend_from_slice(&buf[..n]);
                if let Some(at) = received.windows(4).position(|w| w == b"\r\n\r\n") {
                    break at + 4;
                }
                assert!(n > 0, "the request ended before its headers did");
            };
            let headers = String::from_utf8_lossy(&received[..split]).into_owned();
            let length: usize = headers
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .map(str::to_owned)
                })
                .expect("a length-delimited body, not a chunked one")
                .trim()
                .parse()
                .unwrap();
            let mut body = received[split..].to_vec();
            while body.len() < length {
                let n = stream.read(&mut buf).unwrap();
                assert!(n > 0, "the body ended early");
                body.extend_from_slice(&buf[..n]);
            }
            tx.send((headers, body)).unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}")
                .unwrap();
        });
        (port, rx)
    }

    /// A file of `bytes` in the temp folder that is removed when dropped.
    struct TempFile(std::path::PathBuf);

    impl TempFile {
        fn new(name: &str, bytes: &[u8]) -> Self {
            let path =
                std::env::temp_dir().join(format!("putio-api-{}-{name}", std::process::id()));
            std::fs::write(&path, bytes).unwrap();
            Self(path)
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn upload_of(file: &TempFile, len: u64) -> Request {
        let (head, tail) = (b"HEAD:".to_vec(), b":TAIL".to_vec());
        Request {
            method: Method::Post,
            host: Host::Upload,
            path: "/files/upload".into(),
            accept: "application/json",
            bearer: Some(Token::new("sekrit").unwrap()),
            headers: Vec::new(),
            body: Body::File(crate::transport::FileBody {
                content_type: "multipart/form-data; boundary=x".into(),
                head,
                path: file.0.clone(),
                len,
                offset: 0,
                count: len,
                tail,
                progress: None,
                cancel: None,
            }),
        }
    }

    #[test]
    fn a_file_is_streamed_between_its_head_and_tail_with_a_length_and_progress() {
        let data: Vec<u8> = (0..700_000u32).map(|i| (i % 251) as u8).collect();
        let file = TempFile::new("stream.bin", &data);
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut request = upload_of(&file, data.len() as u64);
        if let Body::File(body) = &mut request.body {
            let seen = seen.clone();
            body.progress = Some(std::sync::Arc::new(move |sent| {
                seen.lock().unwrap().push(sent);
            }));
        }
        let (port, rx) = serve_upload();
        let response = UreqTransport::for_local_tests(port)
            .execute(&request)
            .unwrap();
        assert_eq!(response.status, 200);
        let (headers, body) = rx.recv().unwrap();
        let headers = headers.to_ascii_lowercase();
        assert!(
            headers.contains("content-type: multipart/form-data; boundary=x"),
            "{headers}"
        );
        assert!(
            headers.contains(&format!("content-length: {}", data.len() + 10)),
            "{headers}"
        );
        assert!(!headers.contains("transfer-encoding"), "{headers}");
        assert_eq!(&body[..5], b"HEAD:");
        assert_eq!(&body[5..5 + data.len()], data.as_slice());
        assert_eq!(&body[5 + data.len()..], b":TAIL");
        // Progress only goes up and ends at the file's length.
        let seen = seen.lock().unwrap();
        assert!(seen.windows(2).all(|w| w[0] < w[1]), "{seen:?}");
        assert_eq!(seen.last().copied(), Some(data.len() as u64));
        assert!(seen.len() >= 2, "reported along the way: {seen:?}");
    }

    #[test]
    fn a_file_that_changed_or_is_not_a_file_is_not_sent() {
        let file = TempFile::new("changed.bin", b"12345");
        // The request said the file was longer than it is now.
        let error = UreqTransport::for_local_tests(1)
            .execute(&upload_of(&file, 9))
            .unwrap_err();
        assert!(
            matches!(&error, TransportError::File(why) if why.contains("changed")),
            "{error:?}"
        );
        // A folder, and a file that is not there.
        let folder = TempFile(std::env::temp_dir());
        let error = UreqTransport::for_local_tests(1)
            .execute(&upload_of(&folder, 0))
            .unwrap_err();
        assert!(matches!(error, TransportError::File(_)), "{error:?}");
        let gone = TempFile(std::env::temp_dir().join("putio-api-no-such-file"));
        assert!(
            UreqTransport::for_local_tests(1)
                .execute(&upload_of(&gone, 0))
                .is_err()
        );
    }

    #[test]
    fn a_file_that_gets_shorter_while_it_is_sent_is_a_file_error_not_a_network_one() {
        let data = vec![1u8; 2_000_000];
        let file = TempFile::new("shrinks.bin", &data);
        let mut request = upload_of(&file, data.len() as u64);
        if let Body::File(body) = &mut request.body {
            let path = file.0.clone();
            // As soon as some of it has gone out, the file is cut short.
            body.progress = Some(std::sync::Arc::new(move |_| {
                let _ = std::fs::OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .and_then(|f| f.set_len(10));
            }));
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = vec![0u8; 65536];
            while stream.read(&mut buf).is_ok_and(|n| n > 0) {}
        });
        let error = UreqTransport::for_local_tests(port)
            .execute(&request)
            .unwrap_err();
        assert!(
            matches!(&error, TransportError::File(why) if why.contains("shorter")),
            "{error:?}"
        );
    }

    #[test]
    fn a_cancelled_upload_stops_and_fails() {
        let data = vec![7u8; 3_000_000];
        let file = TempFile::new("cancel.bin", &data);
        let mut request = upload_of(&file, data.len() as u64);
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        if let Body::File(body) = &mut request.body {
            body.cancel = Some(cancel.clone());
            let flag = cancel.clone();
            // Cancel as soon as anything of the file has gone out.
            body.progress = Some(std::sync::Arc::new(move |_| {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
            }));
        }
        // A server that reads whatever comes and never answers in full.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = vec![0u8; 65536];
            while stream.read(&mut buf).is_ok_and(|n| n > 0) {}
        });
        let error = UreqTransport::for_local_tests(port)
            .execute(&request)
            .unwrap_err();
        assert!(matches!(error, TransportError::Network(_)), "{error:?}");
    }

    /// What the loopback tus server has been told and has received.
    #[derive(Debug, Default)]
    struct TusState {
        /// The file's bytes as far as they arrived.
        received: Vec<u8>,
        /// The `Upload-Metadata` of the creation.
        metadata: String,
        /// Every request line, in order.
        log: Vec<String>,
        /// The next part is cut off after this many bytes: they are kept, and the connection is closed
        /// without an answer.
        break_next_part_after: Option<usize>,
    }

    /// A small resumable-upload server (create, offset, parts), a thread for each connection.
    #[allow(clippy::too_many_lines)] // one server, read top to bottom
    fn serve_tus(
        expected_len: usize,
        break_second_part_after: Option<usize>,
    ) -> (u16, std::sync::Arc<std::sync::Mutex<TusState>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = std::sync::Arc::new(std::sync::Mutex::new(TusState::default()));
        let shared = std::sync::Arc::clone(&state);
        let parts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let shared = std::sync::Arc::clone(&shared);
                let parts = std::sync::Arc::clone(&parts);
                // A thread for each connection (the transport uses one per agent); requests come one after
                // the other on a connection until the client closes it.
                thread::spawn(move || {
                    loop {
                        let mut buf = vec![0u8; 65536];
                        let mut head = Vec::new();
                        let split = loop {
                            if let Some(at) = head.windows(4).position(|w| w == b"\r\n\r\n") {
                                break Some(at + 4);
                            }
                            match stream.read(&mut buf) {
                                Ok(0) | Err(_) => break None,
                                Ok(n) => head.extend_from_slice(&buf[..n]),
                            }
                        };
                        let Some(split) = split else { break };
                        let text = String::from_utf8_lossy(&head[..split]).into_owned();
                        let line = text.lines().next().unwrap_or("").to_owned();
                        let header = |name: &str| {
                            text.lines().skip(1).find_map(|l| {
                                let (key, value) = l.split_once(':')?;
                                key.eq_ignore_ascii_case(name)
                                    .then(|| value.trim().to_owned())
                            })
                        };
                        let length: usize = header("content-length")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0);
                        let mut body = head[split..].to_vec();
                        let mut cut_short = false;
                        let break_after = if line.starts_with("PATCH") {
                            let nth = parts.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                            shared
                                .lock()
                                .unwrap()
                                .break_next_part_after
                                .take()
                                .or((nth == 2).then_some(break_second_part_after).flatten())
                        } else {
                            None
                        };
                        while body.len() < length {
                            if break_after.is_some_and(|k| body.len() >= k) {
                                cut_short = true;
                                break;
                            }
                            match stream.read(&mut buf) {
                                Ok(0) | Err(_) => break,
                                Ok(n) => body.extend_from_slice(&buf[..n]),
                            }
                        }
                        let mut state = shared.lock().unwrap();
                        state.log.push(line.clone());
                        let reply = if line.starts_with("POST /files/ ") {
                            state.metadata = header("upload-metadata").unwrap_or_default();
                            "HTTP/1.1 201 Created\r\nLocation: /files/abc123\r\nContent-Length: 0\r\n\r\n"
                            .to_owned()
                        } else if line.starts_with("HEAD /files/abc123 ") {
                            format!(
                                "HTTP/1.1 200 OK\r\nUpload-Offset: {}\r\nContent-Length: 0\r\n\r\n",
                                state.received.len()
                            )
                        } else if line.starts_with("PATCH /files/abc123 ") {
                            let said: usize = header("upload-offset")
                                .and_then(|v| v.parse().ok())
                                .unwrap_or(usize::MAX);
                            let type_ok = header("content-type").as_deref()
                                == Some("application/offset+octet-stream");
                            if said != state.received.len() || !type_ok {
                                "HTTP/1.1 409 Conflict\r\nContent-Length: 0\r\n\r\n".to_owned()
                            } else {
                                let keep = break_after.map_or(body.len(), |k| k.min(body.len()));
                                state.received.extend_from_slice(&body[..keep]);
                                if cut_short || break_after.is_some() {
                                    // Dropped without an answer.
                                    return_drop(&mut stream);
                                    break;
                                }
                                let done = state.received.len() == expected_len;
                                format!(
                                    "HTTP/1.1 204 No Content\r\nUpload-Offset: {}\r\n{}\r\n",
                                    state.received.len(),
                                    if done { "Putio-File-Id: 77\r\n" } else { "" }
                                )
                            }
                        } else {
                            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_owned()
                        };
                        drop(state);
                        stream.write_all(reply.as_bytes()).unwrap();
                    }
                });
            }
        });
        (port, state)
    }

    fn return_drop(stream: &mut std::net::TcpStream) {
        let _ = stream.shutdown(std::net::Shutdown::Both);
    }

    #[test]
    fn a_big_file_goes_up_in_parts_through_the_real_transport_and_survives_a_broken_connection() {
        let data: Vec<u8> = (0..300_000u32).map(|i| (i * 7 % 251) as u8).collect();
        let file = TempFile::new("tus.bin", &data);
        let (port, state) = serve_tus(data.len(), Some(40_000));
        let client = crate::client::Client::with_token(
            UreqTransport::for_local_tests(port),
            Token::new("sekrit").unwrap(),
        );
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = std::sync::Arc::clone(&seen);
        let outcome = client
            .upload_resumable(
                &crate::client::FileUpload {
                    path: file.0.clone(),
                    name: "Großes Video.mkv".into(),
                    len: data.len() as u64,
                    parent_id: 12,
                    progress: Some(std::sync::Arc::new(move |sent| {
                        log.lock().unwrap().push(sent);
                    })),
                    cancel: None,
                },
                crate::client::Tuning {
                    chunk: 100_000,
                    retries: 3,
                    first_wait: Duration::ZERO,
                },
            )
            .unwrap();
        assert_eq!(
            outcome,
            crate::models::UploadOutcome::Resumable { file_id: Some(77) }
        );
        let state = state.lock().unwrap();
        assert_eq!(state.received, data, "every byte arrived, once, in order");
        // The second part was cut at 40 000 bytes; the client asked, and sent on from there.
        let verbs: Vec<&str> = state
            .log
            .iter()
            .map(|l| l.split(' ').next().unwrap())
            .collect();
        assert_eq!(
            verbs,
            ["POST", "PATCH", "PATCH", "HEAD", "PATCH", "PATCH"],
            "{:?}",
            state.log
        );
        // The metadata is the protocol's: `key base64(value)` pairs.
        assert!(
            state.metadata.contains(&format!(
                "name {}",
                crate::form::base64("Großes Video.mkv".as_bytes())
            )),
            "{}",
            state.metadata
        );
        assert!(
            state
                .metadata
                .contains(&format!("parent_id {}", crate::form::base64(b"12")))
        );
        // Progress is of the whole file: it never goes back and ends at its length.
        let seen = seen.lock().unwrap();
        assert!(seen.windows(2).all(|w| w[0] <= w[1]), "{seen:?}");
        assert_eq!(seen.last().copied(), Some(data.len() as u64));
    }

    fn get(path: &str) -> Request {
        Request {
            method: Method::Get,
            host: Host::Api,
            path: path.into(),
            accept: "application/json",
            bearer: Some(Token::new("sekrit").unwrap()),
            headers: Vec::new(),
            body: Body::None,
        }
    }

    #[test]
    fn sends_bearer_header_and_never_puts_the_token_in_the_url() {
        let (port, rx) = serve_once(
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nX-RateLimit-Reset: 9\r\n\r\n{}".to_vec(),
        );
        let response = UreqTransport::for_local_tests(port)
            .execute(&get("/account/info"))
            .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"{}");
        assert_eq!(response.header("x-ratelimit-reset"), Some("9"));
        let request = rx.recv().unwrap().to_ascii_lowercase();
        assert!(request.starts_with("get /account/info http/1.1"));
        assert!(request.contains("authorization: bearer sekrit"));
        assert!(request.contains("user-agent: putio-adder-test"));
        assert_eq!(request.matches("sekrit").count(), 1);
    }

    #[test]
    fn error_statuses_are_returned_not_raised() {
        let (port, _rx) =
            serve_once(b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\n\r\n".to_vec());
        let response = UreqTransport::for_local_tests(port)
            .execute(&get("/x"))
            .unwrap();
        assert_eq!(response.status, 429);
    }

    #[test]
    fn redirects_are_not_followed() {
        let (port, _rx) = serve_once(b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/steal\r\nContent-Length: 0\r\n\r\n".to_vec());
        let response = UreqTransport::for_local_tests(port)
            .execute(&get("/x"))
            .unwrap();
        assert_eq!(response.status, 302);
    }

    #[test]
    fn oversized_bodies_are_rejected() {
        let len = MAX_RESPONSE_BYTES + 1;
        let mut raw = format!("HTTP/1.1 200 OK\r\nContent-Length: {len}\r\n\r\n").into_bytes();
        raw.extend(std::iter::repeat_n(b'x', usize::try_from(len).unwrap()));
        let (port, _rx) = serve_once(raw);
        let error = UreqTransport::for_local_tests(port)
            .execute(&get("/x"))
            .unwrap_err();
        assert_eq!(error, TransportError::TooLarge);
    }

    #[test]
    fn connection_failures_are_network_errors() {
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let error = UreqTransport::for_local_tests(port)
            .execute(&get("/x"))
            .unwrap_err();
        assert!(matches!(error, TransportError::Network(_)));
    }

    #[test]
    fn production_transport_refuses_plain_http() {
        let transport = UreqTransport::new("putio-adder-test");
        // Point at a plain-HTTP base on purpose: https_only must refuse before connecting.
        let mut transport = transport;
        transport.bases.api = "http://127.0.0.1:1".into();
        let error = transport.execute(&get("/x")).unwrap_err();
        assert!(
            matches!(error, TransportError::Network(ref message) if message.to_lowercase().contains("https")),
            "{error:?}"
        );
    }
}
