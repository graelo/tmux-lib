//! Running tmux by forking a client process per command.

use std::process::Command;

use crate::{
    Result,
    tmux::Server,
    transport::{Reply, address},
};

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

    /// Build the full argument vector actually handed to `tmux`.
    fn argv<'a>(&'a self, argv: &[&'a str]) -> Vec<&'a str> {
        let mut full = address(&self.server);
        full.extend_from_slice(argv);
        full
    }

    /// Run one command, forking a client for it.
    pub(crate) fn run(&self, argv: &[&str]) -> Result<Reply> {
        Ok(Command::new("tmux").args(self.argv(argv)).output()?.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_server_is_addressed_before_the_command() {
        let spawning = Spawning::new(Server::socket_name("bench"));

        assert_eq!(
            spawning.argv(&["list-panes", "-a"]),
            ["-u", "-L", "bench", "list-panes", "-a"]
        );
    }

    #[test]
    fn the_default_server_needs_no_addressing() {
        let spawning = Spawning::new(Server::Default);

        assert_eq!(
            spawning.argv(&["list-panes", "-a"]),
            ["-u", "list-panes", "-a"]
        );
    }
}
