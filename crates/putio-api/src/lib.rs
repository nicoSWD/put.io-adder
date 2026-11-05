//! Typed, transport-agnostic client for the [put.io API v2](https://api.put.io/v2/docs/).
//!
//! * [`Client`] has one method per endpoint the app needs and is generic over a [`Transport`].
//! * [`UreqTransport`] is the production transport: HTTPS only, fixed host allowlist, no redirects,
//!   bounded response size, rustls.
//! * [`testing::FakeTransport`] (feature `testing`) lets other crates test without a network.
//!
//! The account token is only ever sent in the `Authorization: Bearer` header, never in a URL.

pub mod client;
pub mod error;
pub mod form;
#[cfg(feature = "http")]
pub mod http;
pub mod models;
pub mod token;
pub mod torrent;
pub mod transport;

#[cfg(test)]
mod contract_tests;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use client::Client;
pub use error::{ApiError, ApiFailure};
#[cfg(feature = "http")]
pub use http::UreqTransport;
pub use models::{
    Account, DeviceCode, Disk, File, FileType, Release, Transfer, TransferKind, TransferStatus,
    UploadOutcome, ZipState, ZipStatus,
};
pub use token::Token;
pub use transport::{Body, Host, Method, Request, Response, Transport, TransportError};
