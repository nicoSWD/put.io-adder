//! Production transport on top of `ureq` + rustls.

use std::time::Duration;

use crate::transport::{Body, Host, Method, Request, Response, Transport, TransportError};

/// Largest response body accepted (a 1000-entry file listing is well under 2 MiB).
pub const MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Uploads of a few MiB over a slow link need longer than API calls.
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(120);

/// Base URLs for each [`Host`].
#[derive(Debug, Clone)]
struct Bases {
    api: String,
    upload: String,
    github: String,
}

impl Bases {
    fn production() -> Self {
        Self {
            api: "https://api.put.io/v2".into(),
            upload: "https://upload.put.io/v2".into(),
            github: "https://api.github.com".into(),
        }
    }

    fn url(&self, host: Host, path: &str) -> String {
        let base = match host {
            Host::Api => &self.api,
            Host::Upload => &self.upload,
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
            bases: Bases::production(),
        }
    }

    /// Plain-HTTP transport pointing every host at a loopback test server. Tests only.
    #[cfg(test)]
    fn for_local_tests(port: u16) -> Self {
        let base = format!("http://127.0.0.1:{port}");
        Self {
            agent: agent(Duration::from_secs(5), "putio-adder-test", false),
            upload_agent: agent(Duration::from_secs(5), "putio-adder-test", false),
            bases: Bases {
                api: base.clone(),
                upload: base.clone(),
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
) -> ureq::RequestBuilder<B> {
    let builder = builder.header("Accept", accept);
    match authorization {
        Some(value) => builder.header("Authorization", value),
        None => builder,
    }
}

impl Transport for UreqTransport {
    fn execute(&self, request: &Request) -> Result<Response, TransportError> {
        let url = self.bases.url(request.host, &request.path);
        let agent = if request.host == Host::Upload {
            &self.upload_agent
        } else {
            &self.agent
        };

        let authorization = request
            .bearer
            .as_ref()
            .map(|token| format!("Bearer {}", token.expose()));
        let authorization = authorization.as_deref();
        let accept = request.accept;

        let result = match (request.method, &request.body) {
            (Method::Get, _) => with_headers(agent.get(&url), accept, authorization).call(),
            (Method::Post, Body::None) => {
                with_headers(agent.post(&url), accept, authorization).send_empty()
            }
            (Method::Post, Body::Form(form)) => {
                with_headers(agent.post(&url), accept, authorization)
                    .header("Content-Type", "application/x-www-form-urlencoded")
                    .send(form.as_str())
            }
            (
                Method::Post,
                Body::Raw {
                    content_type,
                    bytes,
                },
            ) => with_headers(agent.post(&url), accept, authorization)
                .header("Content-Type", content_type)
                .send(bytes.as_slice()),
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

    fn get(path: &str) -> Request {
        Request {
            method: Method::Get,
            host: Host::Api,
            path: path.into(),
            accept: "application/json",
            bearer: Some(Token::new("sekrit").unwrap()),
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
