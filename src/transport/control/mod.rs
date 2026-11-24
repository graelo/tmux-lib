//! Run commands over one attached `tmux -C` client.
//!
//! The reader thread drains stdout even when no command is waiting.
//! `protocol` reads replies; `quoting` renders command arguments.

mod protocol;
mod quoting;

use std::io::{self, BufReader, Read, Write};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use crate::{
    Result,
    error::Error,
    tmux::Server,
    transport::{Reply, Spawning, address},
};

use protocol::{Block, Frame, read_frame};

/// Talks to tmux over one attached control client.
///
/// Cloning shares the connection rather than opening a second one: commands
/// are serialised on it, so a handle shared across threads answers them one at
/// a time in the order they were issued.
#[derive(Debug, Clone)]
pub(crate) struct Control {
    server: Server,
    connection: Arc<Mutex<Connection>>,
    /// The way out, for the operations this transport cannot carry.
    ///
    /// An attached control client is itself a client, so anything whose target
    /// is implicitly "the calling client" would answer about us. It cannot
    /// carry an argument holding a newline. And a command that kills the
    /// attached session ends the connection by succeeding. Those operations
    /// fork a client instead, which is why this is here rather than left to
    /// the caller to arrange.
    spawning: Spawning,
}

impl Control {
    /// Attach a control client to `server`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ControlAttachFailed`] when tmux will not give us a
    /// client, carrying what it said. The ordinary cause is that there is no
    /// session to attach to — a control client is a client, not a session, so
    /// one has to exist already.
    pub(crate) fn connect(server: Server) -> Result<Control> {
        // Attach here rather than lazily, so that the failure is reported to
        // whoever asked for a control transport rather than to whoever
        // happens to issue the first command.
        let live = Live::attach(&server)?;

        Ok(Control {
            server: server.clone(),
            spawning: Spawning::new(server.clone()),
            connection: Arc::new(Mutex::new(Connection {
                server,
                live: Some(live),
            })),
        })
    }

    pub(crate) fn server(&self) -> &Server {
        &self.server
    }

    /// The fork/exec transport onto the same server.
    pub(crate) fn spawning(&self) -> &Spawning {
        &self.spawning
    }

    /// Send one command and block until its reply block closes.
    pub(crate) fn run(&self, argv: &[&str]) -> Result<Reply> {
        // Rendered before taking the lock: an argument this transport cannot
        // carry is the caller's mistake, not a reason to hold up other threads
        // or to disturb the connection.
        let line = quoting::command_line(argv)?;

        self.locked().command(&line)
    }

    /// Release the attached client.
    ///
    /// The next command attaches again. Worth doing when a long-running
    /// program is done talking to tmux for a while, because an attached client
    /// is visible: it bumps `#{session_attached}` and fires the user's
    /// `client-attached` and `client-detached` hooks.
    pub(crate) fn disconnect(&self) {
        self.locked().live = None;
    }

    /// The connection, recovering from a panic in another thread.
    ///
    /// A poisoned lock means some thread panicked mid-command. The connection
    /// is not corrupted by that — the worst case is a reply left unread, which
    /// the next command drains — so refusing to work again would be worse than
    /// carrying on.
    fn locked(&self) -> MutexGuard<'_, Connection> {
        self.connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// The connection, which may currently be down.
#[derive(Debug)]
struct Connection {
    server: Server,
    live: Option<Live>,
}

impl Connection {
    fn command(&mut self, line: &str) -> Result<Reply> {
        if self.live.is_none() {
            self.live = Some(Live::attach(&self.server)?);
        }

        let live = self.live.as_mut().expect("a connection was just attached");

        match live.command(line) {
            Ok(reply) => Ok(reply),
            Err(error) => {
                // The connection is unusable once a command goes unanswered: a
                // later reply would be handed to the wrong caller. Drop it, so
                // the next command attaches again.
                //
                // The failed command is not retried. It may well have taken
                // effect — `kill-session` on the attached session ends the
                // connection precisely by succeeding — and running it twice is
                // worse than reporting it once.
                self.live = None;
                Err(error)
            }
        }
    }
}

/// An attached control client.
#[derive(Debug)]
struct Live {
    child: Child,
    /// Held in an `Option` so that dropping it, which is what tells tmux to
    /// exit, can happen before the child is reaped.
    stdin: Option<ChildStdin>,
    /// Read only when the connection ends, to say why.
    ///
    /// A control client writes to stderr at most once, on the way out, so
    /// leaving the pipe unread costs nothing; command errors arrive on stdout
    /// as `%error` blocks.
    stderr: Option<ChildStderr>,
    frames: Receiver<io::Result<Frame>>,
}

impl Live {
    fn attach(server: &Server) -> Result<Live> {
        let mut child = Command::new("tmux")
            .args(address(server))
            // `no-output` stops the flood: an attached control client is sent
            // every byte of every pane otherwise. `ignore-size` keeps our
            // client, which reports no size, from resizing the user's windows.
            // `new-layouts` gives us the same layout format as a spawned
            // client, including floating panes.
            .args(["-C", "attach", "-f", "no-output,ignore-size,new-layouts"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdin = child.stdin.take().expect("stdin was piped");
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");

        let (sender, frames) = channel();
        thread::spawn(move || read_frames(stdout, &sender));

        let mut live = Live {
            child,
            stdin: Some(stdin),
            stderr: Some(stderr),
            frames,
        };

        // Consume the attach reply (flags 0) before sending any commands.
        match live.recv_block() {
            Ok(block) if block.flags == 0 && !block.failed => Ok(live),
            Ok(block) if block.flags == 0 => Err(Error::ControlAttachFailed {
                message: String::from_utf8_lossy(&block.body).trim_end().to_owned(),
            }),
            Ok(_) => Err(Error::ControlAttachFailed {
                message: "expected the initial attach reply".to_owned(),
            }),
            Err(message) => Err(Error::ControlAttachFailed { message }),
        }
    }

    fn command(&mut self, line: &str) -> Result<Reply> {
        let stdin = self
            .stdin
            .as_mut()
            .expect("stdin is taken only while dropping");
        stdin.write_all(line.as_bytes())?;
        stdin.write_all(b"\n")?;
        stdin.flush()?;

        let block = self
            .recv_block()
            .map_err(|reason| Error::ControlDisconnected { reason })?;
        Ok(if block.failed {
            Reply::failure(block.body)
        } else {
            Reply::success(block.body)
        })
    }

    fn recv_block(&mut self) -> std::result::Result<Block, String> {
        match self.frames.recv() {
            Ok(Ok(Frame::Reply(block))) => Ok(block),
            Ok(Ok(Frame::Exit(Some(reason)))) => Err(reason),
            Ok(Err(error)) => Err(error.to_string()),
            Ok(Ok(Frame::Exit(None))) | Err(_) => Err(self.final_words()),
        }
    }

    /// Whatever tmux wrote to stderr, for reporting why the client is gone.
    fn final_words(&mut self) -> String {
        let Some(mut stderr) = self.stderr.take() else {
            return String::new();
        };

        let mut message = String::new();
        // Called only once the client has closed stdout, so this reads to end
        // of file rather than blocking.
        let _ = stderr.read_to_string(&mut message);

        message.trim_end().to_owned()
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        // Closing stdin is the graceful way out: tmux answers with `%exit` and
        // exits 0, having fired the user's `client-detached` hook.
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

/// Forward only replies, exit, and read failures; notifications are discarded
/// by the protocol reader without allocating an event for each one.
fn read_frames(stdout: ChildStdout, frames: &Sender<io::Result<Frame>>) {
    let mut reader = BufReader::new(stdout);
    let mut line = Vec::new();

    loop {
        match read_frame(&mut reader, &mut line) {
            Ok(Some(frame)) => {
                if frames.send(Ok(frame)).is_err() {
                    return;
                }
            }
            Ok(None) => return,
            Err(error) => {
                let _ = frames.send(Err(error));
                return;
            }
        }
    }
}
