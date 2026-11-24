//! Snapshots of tmux resources and their identifiers.
//!
//! ```
//! use tmux_lib::state::pane::{Pane, PaneId};
//!
//! fn pane_id(pane: &Pane) -> &PaneId {
//!     &pane.id
//! }
//! ```

pub mod client;
pub mod pane;
pub mod session;
pub mod window;
