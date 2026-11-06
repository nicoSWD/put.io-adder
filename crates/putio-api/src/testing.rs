//! An in-memory [`Transport`] for tests in this and other crates.

use std::collections::VecDeque;
use std::sync::Mutex;

use crate::transport::{Method, Request, Response, Transport, TransportError};

/// Replays queued responses in order and records every request it saw.
#[derive(Debug, Default)]
pub struct FakeTransport {
    responses: Mutex<VecDeque<Result<Response, TransportError>>>,
    requests: Mutex<Vec<Request>>,
}

impl FakeTransport {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn queue(&self, item: Result<Response, TransportError>) {
        if let Ok(mut queue) = self.responses.lock() {
            queue.push_back(item);
        }
    }

    pub fn push_json(&self, status: u16, body: &str) -> &Self {
        self.push_response(status, &[], body)
    }

    pub fn push_response(&self, status: u16, headers: &[(&str, &str)], body: &str) -> &Self {
        self.queue(Ok(Response {
            status,
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), (*v).to_owned()))
                .collect(),
            body: body.as_bytes().to_vec(),
        }));
        self
    }

    /// Queue an answer whose body is bytes, not text.
    pub fn queue_bytes(&self, status: u16, body: Vec<u8>) -> &Self {
        self.queue(Ok(Response {
            status,
            headers: Vec::new(),
            body,
        }));
        self
    }

    pub fn push_error(&self, error: TransportError) -> &Self {
        self.queue(Err(error));
        self
    }

    /// Every request executed so far.
    #[must_use]
    pub fn requests(&self) -> Vec<Request> {
        self.requests
            .lock()
            .map(|requests| requests.clone())
            .unwrap_or_default()
    }

    /// The most recent request.
    #[must_use]
    pub fn last_request(&self) -> Option<Request> {
        self.requests().pop()
    }
}

impl Transport for FakeTransport {
    fn execute(&self, request: &Request) -> Result<Response, TransportError> {
        if let Ok(mut requests) = self.requests.lock() {
            requests.push(request.clone());
        }
        self.responses
            .lock()
            .ok()
            .and_then(|mut queue| queue.pop_front())
            .unwrap_or_else(|| {
                Err(TransportError::Network(
                    "FakeTransport: no response queued".into(),
                ))
            })
    }
}

impl Request {
    /// Convenience for assertions: `GET /path` / `POST /path`.
    #[must_use]
    pub fn describe(&self) -> String {
        let verb = match self.method {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Head => "HEAD",
            Method::Patch => "PATCH",
        };
        format!("{verb} {}", self.path)
    }
}

/// Lets a test keep using the fake after handing it to a [`Client`](crate::Client).
impl Transport for &FakeTransport {
    fn execute(&self, request: &Request) -> Result<Response, TransportError> {
        (**self).execute(request)
    }
}
