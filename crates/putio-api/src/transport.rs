//! The seam between the typed client and the network.

use crate::token::Token;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

/// Which server a request goes to. Requests name a host from this closed set instead of carrying a
/// URL, so a bug (or hostile input) cannot make the client talk to anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    /// `https://api.put.io/v2`
    Api,
    /// `https://upload.put.io/v2`
    Upload,
    /// `https://api.github.com` (update check)
    GitHub,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    None,
    /// `application/x-www-form-urlencoded`, already encoded.
    Form(String),
    /// Any other body (multipart uploads).
    Raw {
        content_type: String,
        bytes: Vec<u8>,
    },
    /// A body with a file in the middle, read from disk while it is sent (so a big file is never held in
    /// memory).
    File(FileBody),
}

/// Called with how many bytes of the file have been sent.
pub type ProgressFn = std::sync::Arc<dyn Fn(u64) + Send + Sync>;

/// A body made of `head`, then the first `len` bytes of the file at `path`, then `tail`: a multipart upload
/// with one file part.
#[derive(Clone)]
pub struct FileBody {
    pub content_type: String,
    pub head: Vec<u8>,
    pub path: std::path::PathBuf,
    /// How long the file was when the request was made. A file that is not that long when it is read
    /// (it changed meanwhile) is an error, not a short upload.
    pub len: u64,
    pub tail: Vec<u8>,
    /// Told how far the sending has got, now and then.
    pub progress: Option<ProgressFn>,
    /// Set to stop sending: the request then fails.
    pub cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

impl std::fmt::Debug for FileBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileBody")
            .field("content_type", &self.content_type)
            .field("head", &self.head.len())
            .field("path", &self.path)
            .field("len", &self.len)
            .field("tail", &self.tail.len())
            .finish_non_exhaustive()
    }
}

/// What is sent decides equality; the callbacks do not.
impl PartialEq for FileBody {
    fn eq(&self, other: &Self) -> bool {
        self.content_type == other.content_type
            && self.head == other.head
            && self.path == other.path
            && self.len == other.len
            && self.tail == other.tail
    }
}

impl Eq for FileBody {}

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub host: Host,
    /// Path and query, starting with `/`, relative to the host's base (e.g. `/transfers/list`).
    pub path: String,
    /// Value of the `Accept` header.
    pub accept: &'static str,
    /// Sent as `Authorization: Bearer …` and nowhere else.
    pub bearer: Option<Token>,
    pub body: Body,
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    /// Header names are lower-case.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    #[error("{0}")]
    Network(String),
    #[error("response exceeds the size limit")]
    TooLarge,
    /// The file being uploaded could not be read, or changed while it was read: nothing to do with the
    /// network.
    #[error("{0}")]
    File(String),
}

pub trait Transport: Send + Sync {
    /// Perform one request. HTTP error statuses are *not* errors here; the client maps them.
    fn execute(&self, request: &Request) -> Result<Response, TransportError>;
}

/// Lets several threads share one transport (and its connection pool).
impl<T: Transport + ?Sized> Transport for std::sync::Arc<T> {
    fn execute(&self, request: &Request) -> Result<Response, TransportError> {
        (**self).execute(request)
    }
}
