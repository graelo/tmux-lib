//! Running tmux by forking a client process per command.
//!
//! This is the only place in the crate that starts a tmux process. Every
//! argument every operation sends passes through [`Spawning::output`], so
//! anything that must be true of all invocations — the server address, and
//! later the UTF-8 flag — is stated here once and cannot be forgotten at a
//! call site.

use std::process::{Command, Output};

use crate::{Result, tmux::Server};

/// Forks one `tmux` client per command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Spawning {
    server: Server,
}

impl Spawning {
    pub(crate) fn new(server: Server) -> Self {
        Self { server }
    }

    pub(crate) fn server(&self) -> &Server {
        &self.server
    }

    /// Arguments that precede every command, addressing the server to talk to.
    fn prefix(&self) -> Vec<&str> {
        match &self.server {
            Server::Default => Vec::new(),
            Server::SocketName(name) => vec!["-L", name],
            Server::SocketPath(path) => vec!["-S", path],
        }
    }

    /// Build the full argument vector actually handed to `tmux`.
    fn argv<'a>(&'a self, argv: &[&'a str]) -> Vec<&'a str> {
        let mut full = self.prefix();
        full.extend_from_slice(argv);
        full
    }

    pub(crate) fn output(&self, argv: &[&str]) -> Result<Output> {
        Ok(Command::new("tmux").args(self.argv(argv)).output()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_server_needs_no_addressing() {
        let spawning = Spawning::new(Server::Default);

        assert_eq!(spawning.argv(&["list-panes", "-a"]), ["list-panes", "-a"]);
    }

    #[test]
    fn a_named_socket_is_addressed_before_the_command() {
        let spawning = Spawning::new(Server::socket_name("bench"));

        assert_eq!(
            spawning.argv(&["list-panes"]),
            ["-L", "bench", "list-panes"]
        );
    }

    #[test]
    fn a_socket_path_is_addressed_before_the_command() {
        let spawning = Spawning::new(Server::socket_path("/tmp/tmux.sock"));

        assert_eq!(
            spawning.argv(&["kill-server"]),
            ["-S", "/tmp/tmux.sock", "kill-server"]
        );
    }
}
