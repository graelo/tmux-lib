//! Running tmux by forking a client process per command.
//!
//! This is the only place in the crate that starts a tmux process. Every
//! argument every operation sends passes through [`Spawning::run`], so
//! anything that must be true of all invocations — the server address, and
//! later the UTF-8 flag — is stated here once and cannot be forgotten at a
//! call site.

use std::process::Command;

use crate::{Result, tmux::Server, transport::Reply};

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

    /// Arguments that precede every command.
    ///
    /// `-u` forces UTF-8. Without it tmux takes the flag from the locale, and
    /// `server_client_print` runs `utf8_sanitize` for a client that lacks it,
    /// replacing every non-ASCII byte with `_`. That silently mangles titles
    /// and paths whenever the caller runs without `LANG`/`LC_*`, as a daemon
    /// or a cron job does.
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
    fn prefix(&self) -> Vec<&str> {
        let mut prefix = vec!["-u"];
        match &self.server {
            Server::Default => {}
            Server::SocketName(name) => prefix.extend(["-L", name]),
            Server::SocketPath(path) => prefix.extend(["-S", path]),
        }
        prefix
    }

    /// Build the full argument vector actually handed to `tmux`.
    fn argv<'a>(&'a self, argv: &[&'a str]) -> Vec<&'a str> {
        let mut full = self.prefix();
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
    fn the_default_server_needs_no_addressing() {
        let spawning = Spawning::new(Server::Default);

        assert_eq!(
            spawning.argv(&["list-panes", "-a"]),
            ["-u", "list-panes", "-a"]
        );
    }

    #[test]
    fn a_named_socket_is_addressed_before_the_command() {
        let spawning = Spawning::new(Server::socket_name("bench"));

        assert_eq!(
            spawning.argv(&["list-panes"]),
            ["-u", "-L", "bench", "list-panes"]
        );
    }

    #[test]
    fn a_socket_path_is_addressed_before_the_command() {
        let spawning = Spawning::new(Server::socket_path("/tmp/tmux.sock"));

        assert_eq!(
            spawning.argv(&["kill-server"]),
            ["-u", "-S", "/tmp/tmux.sock", "kill-server"]
        );
    }

    #[test]
    fn every_invocation_forces_utf8() {
        for server in [
            Server::Default,
            Server::socket_name("s"),
            Server::socket_path("/tmp/s"),
        ] {
            assert_eq!(Spawning::new(server).argv(&["list-panes"])[0], "-u");
        }
    }
}
