//! The handle every tmux operation hangs off.

use crate::{
    Result,
    transport::{Control, Reply, Spawning, Transport},
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
/// shared across threads. Cloning is cheap and shares whatever connection the
/// handle has.
///
/// ```no_run
/// let tmux = tmux_lib::Tmux::spawning();
///
/// for pane in tmux.available_panes()? {
///     println!("{} {}", pane.id, pane.command);
/// }
/// # Ok::<(), tmux_lib::error::Error>(())
/// ```
///
/// # Choosing a transport
///
/// [`Tmux::spawning`] forks a `tmux` client per command. It costs nothing to
/// construct, cannot fail, attaches no client, and needs no session to exist.
/// A program that runs a handful of commands and exits should use it.
///
/// [`Tmux::control`] keeps one client attached and sends every command down
/// it, so a command costs a round trip on an open pipe rather than a fork, an
/// exec and a connect. It pays for itself over many commands. It needs a
/// session to attach to, and the attached client is visible to the user: it
/// bumps `#{session_attached}` and fires the `client-attached` and
/// `client-detached` hooks.
#[derive(Debug, Clone)]
pub struct Tmux {
    transport: Transport,
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
            transport: Transport::Spawning(Spawning::new(server)),
        }
    }

    /// Talk to the default tmux server over one attached control client.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ControlAttachFailed`] when tmux will not give us a
    /// client. The ordinary cause is that there is no session to attach to,
    /// which is also the state a tool restoring a session from a backup starts
    /// in. There is no constructor that falls back on your behalf, because
    /// which transport you ended up with changes what the handle costs and
    /// what it can do; write the fallback where it can be seen:
    ///
    /// ```no_run
    /// use tmux_lib::Tmux;
    ///
    /// let tmux = Tmux::control().unwrap_or_else(|_| Tmux::spawning());
    /// ```
    ///
    /// [`Error::ControlAttachFailed`]: crate::error::Error::ControlAttachFailed
    pub fn control() -> Result<Tmux> {
        Tmux::control_on(Server::Default)
    }

    /// Talk to `server` over one attached control client.
    ///
    /// # Errors
    ///
    /// See [`Tmux::control`].
    pub fn control_on(server: Server) -> Result<Tmux> {
        Ok(Tmux {
            transport: Transport::Control(Control::connect(server)?),
        })
    }

    /// Release the attached control client, if this handle has one.
    ///
    /// The next command attaches again. A spawning handle has nothing to
    /// release, so this does nothing. Worth calling when a long-running
    /// program is done talking to tmux for a while, since an attached client
    /// is visible to the user.
    pub fn disconnect(&self) {
        if let Transport::Control(control) = &self.transport {
            control.disconnect();
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

    /// Run one tmux command by forking a client, whatever transport this
    /// handle otherwise uses. See [`Transport::run_spawned`].
    pub(crate) fn run_spawned(&self, argv: &[&str]) -> Result<Reply> {
        self.transport.run_spawned(argv)
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
