//! Application logic of put.io adder, free of any I/O.
//!
//! The UI and the OS adapters live in other crates. This crate owns *what the app does*:
//! it is a pure reducer ([`state::update`]) that turns [`state::Event`]s into [`state::Effect`]s
//! (things for the shell to do), plus the validation and formatting rules that go with it.
//! Everything here is deterministic and unit-testable without a network, a clock or a window.

#![cfg_attr(test, allow(clippy::assert_is_empty))] // `assert!(fx.is_empty())` reads better than a typed empty array

pub mod download;
pub mod feeds;
pub mod format;
pub mod handler;
pub mod i18n;
pub mod intake;
pub mod magnet;
pub mod settings;
pub mod state;
pub mod transfers;
pub mod update;
