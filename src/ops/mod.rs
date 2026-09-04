//! Composition: tmux commands in, typed values out.
//!
//! Each submodule adds an `impl Tmux` block. They hold no state of their own —
//! the argument vectors live here, the bytes are moved by `crate::transport`,
//! and their meaning is decoded by `crate::wire`.

mod client;
mod pane;
mod server;
mod session;
mod window;
