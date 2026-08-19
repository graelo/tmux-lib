//! Read or manipulate tmux.
//!
//! See the [project README](https://github.com/graelo/tmux-lib#readme) for
//! installation and usage guidance.

pub mod error;

pub mod client;
pub use client::display_message;
pub mod layout;
pub mod pane;
pub mod pane_id;
pub(crate) mod parse;
pub mod server;
pub mod session;
pub mod session_id;
pub mod utils;
pub mod window;
pub mod window_id;

/// Result type for this crate.
pub type Result<T> = std::result::Result<T, error::Error>;
