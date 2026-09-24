//! Client for the EMX mail API.
//!
//! EMX is hosted business mail by Elchi Studios. The API the web client
//! uses is the developer API: everything the client can do, a program can
//! do with a token, within the token's scopes and the person's own rights.
//! This crate is that API as Rust types and calls, with nothing added and
//! nothing left out that a token can reach.
//!
//! ```no_run
//! use emx_sdk::Client;
//!
//! let emx = Client::new("emx_...")?;
//! let me = emx.me()?;
//! for account in &me.accounts {
//!     let mailboxes = emx.mailboxes(&account.id)?;
//!     for mailbox in mailboxes {
//!         println!("{} {} unread", mailbox.name, mailbox.unseen);
//!     }
//! }
//! # Ok::<(), emx_sdk::Error>(())
//! ```
//!
//! Calls are blocking. A program that wants them elsewhere puts the client
//! on a thread; the client is `Send` and `Sync` and can be shared.
//!
//! The reference for every call is at <https://docs.elchi.dev/emx-api>.

mod client;
mod error;
mod events;
mod types;
pub mod webhook;

pub use client::{Client, Page};
pub use error::Error;
pub use events::{Event, Events};
pub use types::*;

/// The address of the hosted service.
pub const DEFAULT_BASE_URL: &str = "https://mail.emxmail.app";

/// The account name that stands for the person's own account.
pub const ME: &str = "me";

/// The tenant name that stands for the person's own organisation.
pub const MINE: &str = "mine";
