//! The long-lived `tmux -C` control connection.
//!
//! Split so that the parts with no I/O can be tested without a tmux:
//! [`protocol`] turns a stream of lines into command replies and
//! notifications, and [`quoting`] turns an argument vector into the single
//! line the control connection accepts.

// The reader that drives both of these lands in the next commit; until then
// nothing outside the tests calls them.
#![allow(dead_code)]

pub(crate) mod protocol;
pub(crate) mod quoting;
