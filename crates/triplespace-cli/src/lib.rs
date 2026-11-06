//! `triplespace`: the command line (0005 §2; 0033 §13), as a library so the acceptance
//! tests drive the same code the binary runs.

#![forbid(unsafe_code)]

pub mod accounts;
pub mod adopt;
pub mod adopt_pages;
pub mod adopt_talk;
pub mod bootstrap;
pub mod challenge;
pub mod common;
pub mod env_file;
pub mod follow;
pub mod forwarder;
pub mod instance;
pub mod oauth_client;
pub mod rights;
pub mod settings;
pub mod source;
pub mod status;
pub mod sync;
pub mod tenant;
