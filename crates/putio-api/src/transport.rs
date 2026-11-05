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
}

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
