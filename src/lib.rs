//! A library for reading and manipulating tmux state.
//!
//! See the project README for an overview.

pub mod error;

pub mod client;
pub mod layout;
mod ops;
pub mod pane;
pub mod pane_id;
pub mod session;
pub mod session_id;
mod tmux;
mod transport;
pub mod utils;
pub mod window;
pub mod window_id;
pub(crate) mod wire;

pub use tmux::{Server, Tmux};

/// Convenience type alias for the crate's fallible operations.
pub type Result<T> = std::result::Result<T, error::Error>;
