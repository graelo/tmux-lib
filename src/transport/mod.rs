//! How bytes reach tmux.
//!
//! One transport today: fork a client per command. The long-lived `tmux -C`
//! control connection joins it here rather than replacing it, because client
//! identity, arguments carrying arbitrary bytes, and commands that kill the
//! attached session each need a fork/exec path to fall back to.

pub(crate) mod spawning;

pub(crate) use spawning::Spawning;
