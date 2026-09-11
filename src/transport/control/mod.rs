//! The long-lived `tmux -C` control connection.
//!
//! One attached client handles every command, so a command costs a round trip
//! on an open pipe rather than a fork, an exec and a connect.
//!
//! Split so that the parts with no I/O can be tested without a tmux:
//! [`protocol`] turns a stream of lines into command replies and
//! notifications, and [`quoting`] turns an argument vector into the single
//! line the control connection accepts. What is left here is the process, the
//! reader thread, and the reconnect.

pub(crate) mod protocol;
pub(crate) mod quoting;

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use crate::{
    Result,
    error::Error,
    tmux::Server,
    transport::{Reply, address},
};

use protocol::{Demux, Event};

/// Talks to tmux over one attached control client.
///
/// Cloning shares the connection rather than opening a second one: commands
/// are serialised on it, so a handle shared across threads answers them one at
/// a time in the order they were issued.
#[derive(Debug, Clone)]
pub(crate) struct Control {
    server: Server,
    connection: Arc<Mutex<Connection>>,
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
            connection: Arc::new(Mutex::new(Connection {
                server,
                live: Some(live),
            })),
        })
    }

    pub(crate) fn server(&self) -> &Server {
        &self.server
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
    events: Receiver<Event>,
}

impl Live {
    fn attach(server: &Server) -> Result<Live> {
        let mut child = Command::new("tmux")
            .args(address(server))
            // `no-output` stops the flood: an attached control client is sent
            // every byte of every pane otherwise. `ignore-size` keeps our
            // client, which reports no size, from resizing the user's windows.
            .args(["-C", "attach", "-f", "no-output,ignore-size"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdin = child.stdin.take().expect("stdin was piped");
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");

        let (sender, events) = channel();
        thread::spawn(move || read_events(stdout, &sender));

        let mut live = Live {
            child,
            stdin: Some(stdin),
            stderr: Some(stderr),
            events,
        };

        // The attach performs a command of its own, and its block is the only
        // one carrying flags `0`. Consuming it here is what keeps it from
        // being handed to the first caller as their reply.
        //
        // It is also where a refused attach is reported: with no session to
        // attach to, that block is terminated by `%error` and its body is
        // tmux's `no sessions`. Accepting any flags-`0` block would hand back
        // a handle whose every command fails.
        match live.events.recv() {
            Ok(Event::Block(block)) if block.flags == 0 && !block.failed => Ok(live),
            Ok(Event::Block(block)) if block.flags == 0 => Err(Error::ControlAttachFailed {
                message: String::from_utf8_lossy(&block.body).trim_end().to_owned(),
            }),
            Ok(_) | Err(_) => Err(Error::ControlAttachFailed {
                message: live.final_words(),
            }),
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

        loop {
            match self.events.recv() {
                Ok(Event::Block(block)) => {
                    return Ok(if block.failed {
                        Reply::failure(block.body)
                    } else {
                        Reply::success(block.body)
                    });
                }
                // The reader drops these, but the channel carries the type.
                Ok(Event::Notification(_)) => continue,
                Ok(Event::Exit(reason)) => {
                    let reason = reason.unwrap_or_else(|| self.final_words());
                    return Err(Error::ControlDisconnected { reason });
                }
                // The reader is gone, so tmux closed stdout without saying
                // why. Whatever it wrote on the way out is the best answer.
                Err(_) => {
                    return Err(Error::ControlDisconnected {
                        reason: self.final_words(),
                    });
                }
            }
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

/// Read the control stream until it ends, forwarding what callers wait on.
///
/// Notifications are dropped rather than queued. The reader has to recognise
/// them either way, to know they are not part of a reply; queueing them would
/// grow without bound between commands, and nothing exposes them yet.
fn read_events(stdout: ChildStdout, events: &Sender<Event>) {
    let mut reader = BufReader::new(stdout);
    let mut demux = Demux::new();
    let mut line = Vec::new();

    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if line.last() == Some(&b'\n') {
            line.pop();
        }

        let Some(event) = demux.line(&line) else {
            continue;
        };
        if matches!(event, Event::Notification(_)) {
            continue;
        }
        if events.send(event).is_err() {
            return;
        }
    }
}
