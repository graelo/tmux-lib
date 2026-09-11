//! The handle every tmux operation hangs off.

use crate::{
    Result,
    transport::{Reply, Spawning},
};

/// Which tmux server to talk to.
///
/// tmux addresses a server by socket. The default socket is the usual one;
/// naming or pathing a socket reaches a private server, which is what test
/// suites and tools that must not disturb the user's session want.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Server {
    /// The default server, as a bare `tmux` invocation would reach.
    #[default]
    Default,
    /// A server addressed by socket name, as `tmux -L <name>`.
    SocketName(String),
    /// A server addressed by socket path, as `tmux -S <path>`.
    SocketPath(String),
}

impl Server {
    /// Address a server by socket name.
    pub fn socket_name(name: impl Into<String>) -> Server {
        Server::SocketName(name.into())
    }

    /// Address a server by socket path.
    pub fn socket_path(path: impl Into<String>) -> Server {
        Server::SocketPath(path.into())
    }
}

/// A handle onto a tmux server.
///
/// Every operation is an inherent method taking `&self`, so one handle can be
/// shared across threads. Constructing it costs nothing and cannot fail: the
/// spawning transport forks a client per command and holds no state between
/// them.
///
/// ```no_run
/// let tmux = tmux_lib::Tmux::spawning();
///
/// for pane in tmux.available_panes()? {
///     println!("{} {}", pane.id, pane.command);
/// }
/// # Ok::<(), tmux_lib::error::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tmux {
    transport: Spawning,
}

impl Tmux {
    /// Talk to the default tmux server, forking a client per command.
    pub fn spawning() -> Tmux {
        Tmux::spawning_on(Server::Default)
    }

    /// Talk to `server`, forking a client per command.
    ///
    /// ```no_run
    /// use tmux_lib::{Server, Tmux};
    ///
    /// let tmux = Tmux::spawning_on(Server::socket_name("my-private-server"));
    /// # Ok::<(), tmux_lib::error::Error>(())
    /// ```
    pub fn spawning_on(server: Server) -> Tmux {
        Tmux {
            transport: Spawning::new(server),
        }
    }

    /// The server this handle addresses.
    pub fn server(&self) -> &Server {
        self.transport.server()
    }

    /// Run one tmux command and return its standard output.
    ///
    /// This is the escape hatch for commands the crate does not model yet.
    /// Prefer a named operation where one exists: they check exit status and
    /// decode the reply, whereas this hands back raw bytes.
    ///
    /// # Note
    ///
    /// The exact bytes are what this transport observed on stdout. A future
    /// transport may frame command output differently — notably in how a
    /// trailing newline is reported — so do not depend on byte-identical
    /// output across transports.
    pub fn command(&self, argv: &[&str]) -> Result<Vec<u8>> {
        self.run(argv)?.output("command")
    }

    /// Run one tmux command and return what the transport made of it.
    pub(crate) fn run(&self, argv: &[&str]) -> Result<Reply> {
        self.transport.run(argv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_handle_is_shareable_across_threads() {
        fn assert_shareable<T: Send + Sync + Clone>() {}

        assert_shareable::<Tmux>();
    }

    #[test]
    fn spawning_defaults_to_the_default_server() {
        assert_eq!(Tmux::spawning().server(), &Server::Default);
    }

    #[test]
    fn a_handle_remembers_the_server_it_addresses() {
        let tmux = Tmux::spawning_on(Server::socket_name("bench"));

        assert_eq!(tmux.server(), &Server::SocketName("bench".to_owned()));
    }
}
