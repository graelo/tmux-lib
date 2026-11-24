//! A library for reading and manipulating tmux state.
//!
//! See the project README for an overview.

pub mod error;

pub mod layout;
mod ops;
pub mod state;
mod tmux;
mod transport;
pub mod utils;
pub(crate) mod wire;

pub use state::{client, pane, session, window};
pub use tmux::{Server, Tmux};

/// Compatibility import for [`state::pane::PaneId`].
pub mod pane_id {
    pub use crate::state::pane::PaneId;
}

/// Compatibility import for [`state::session::SessionId`].
pub mod session_id {
    pub use crate::state::session::SessionId;
}

/// Compatibility import for [`state::window::WindowId`].
pub mod window_id {
    pub use crate::state::window::WindowId;
}

/// Convenience type alias for the crate's fallible operations.
pub type Result<T> = std::result::Result<T, error::Error>;
