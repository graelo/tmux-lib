//! How bytes reach tmux.
//!
//! Two transports. [`Spawning`] forks a client per command: no setup cost, no
//! attached client, no lifetime coupling. [`Control`] keeps one attached
//! client and sends commands down it, trading an attach for a round trip per
//! command.
//!
//! They are siblings rather than one replacing the other, because client
//! identity, arguments carrying arbitrary bytes, and commands that kill the
//! attached session each need a fork/exec path to fall back to.

pub(crate) mod control;
pub(crate) mod reply;
pub(crate) mod spawning;

pub(crate) use control::Control;
pub(crate) use reply::Reply;
pub(crate) use spawning::Spawning;

use crate::{Result, tmux::Server};

/// The arguments that precede every tmux command, on either transport.
///
/// `-u` forces UTF-8. Without it tmux takes the flag from the locale, and
/// `server_client_print` runs `utf8_sanitize` for a client that lacks it,
/// replacing every non-ASCII byte with `_`. That silently mangles titles and
/// paths whenever the caller runs without `LANG`/`LC_*`, as a daemon or a cron
/// job does.
///
/// Reproduced on tmux 3.7c, where a pane titled `π-test` reads back as
/// `_-test`:
///
/// ```sh
/// env -i PATH="$PATH" HOME="$HOME" tmux -L probe new-session -d
/// env -i PATH="$PATH" HOME="$HOME" tmux -L probe select-pane -T 'π-test'
/// env -i PATH="$PATH" HOME="$HOME" tmux -L probe    list-panes -F '#{pane_title}'
/// env -i PATH="$PATH" HOME="$HOME" tmux -L probe -u list-panes -F '#{pane_title}'
/// ```
pub(crate) fn address(server: &Server) -> Vec<&str> {
    let mut prefix = vec!["-u"];
    match server {
        Server::Default => {}
        Server::SocketName(name) => prefix.extend(["-L", name]),
        Server::SocketPath(path) => prefix.extend(["-S", path]),
    }
    prefix
}

/// How one handle reaches tmux.
#[derive(Debug, Clone)]
pub(crate) enum Transport {
    /// Fork a client per command.
    Spawning(Spawning),
    /// Send commands down one attached control client.
    Control(Control),
}

impl Transport {
    pub(crate) fn run(&self, argv: &[&str]) -> Result<Reply> {
        match self {
            Transport::Spawning(spawning) => spawning.run(argv),
            Transport::Control(control) => control.run(argv),
        }
    }

    pub(crate) fn server(&self) -> &Server {
        match self {
            Transport::Spawning(spawning) => spawning.server(),
            Transport::Control(control) => control.server(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_server_needs_no_addressing() {
        assert_eq!(address(&Server::Default), ["-u"]);
    }

    #[test]
    fn a_named_socket_is_addressed_before_the_command() {
        assert_eq!(
            address(&Server::socket_name("bench")),
            ["-u", "-L", "bench"]
        );
    }

    #[test]
    fn a_socket_path_is_addressed_before_the_command() {
        assert_eq!(
            address(&Server::socket_path("/tmp/tmux.sock")),
            ["-u", "-S", "/tmp/tmux.sock"]
        );
    }

    #[test]
    fn every_invocation_forces_utf8() {
        for server in [
            Server::Default,
            Server::socket_name("s"),
            Server::socket_path("/tmp/s"),
        ] {
            assert_eq!(address(&server)[0], "-u");
        }
    }
}
