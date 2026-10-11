//! Server-level operations: lifecycle and options.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use crate::{Result, error::Error, tmux::Tmux, wire::options::parse_options};

/// Maximum time to wait for the server to become ready.
const SERVER_READY_TIMEOUT: Duration = Duration::from_secs(5);

/// Delay between readiness checks.
const SERVER_READY_POLL_INTERVAL: Duration = Duration::from_millis(50);

impl Tmux {
    /// Start the tmux server if needed, creating a session named
    /// `initial_session_name` in order to keep the server running.
    ///
    /// This waits for the server to be fully ready before returning, so a
    /// subsequent command can be issued immediately.
    pub fn start_server(&self, initial_session_name: &str) -> Result<()> {
        // Forks a client throughout: this runs when there may be no server at
        // all, which is exactly when there is nothing to attach a control
        // client to.
        self.run_spawned(&["new-session", "-d", "-s", initial_session_name])?
            .no_output("new-session")?;

        self.wait_for_server_ready()
    }

    /// Poll the server with `list-sessions` until it answers or the deadline
    /// passes.
    fn wait_for_server_ready(&self) -> Result<()> {
        let deadline = Instant::now() + SERVER_READY_TIMEOUT;

        loop {
            if self.run_spawned(&["list-sessions"])?.succeeded() {
                return Ok(());
            }

            if Instant::now() >= deadline {
                return Err(Error::UnexpectedTmuxOutput {
                    intent: "wait-for-server-ready",
                    stdout: String::new(),
                    stderr: format!("server did not become ready within {SERVER_READY_TIMEOUT:?}"),
                });
            }

            std::thread::sleep(SERVER_READY_POLL_INTERVAL);
        }
    }

    /// Remove the session exactly named `name`.
    ///
    /// This forks a client even on a control handle. Killing the session a
    /// control client is attached to ends the connection by succeeding, which
    /// would be reported as a failure.
    pub fn kill_session(&self, name: &str) -> Result<()> {
        let exact_name = format!("={name}");

        self.run_spawned(&["kill-session", "-t", &exact_name])?
            .no_output("kill-session")
    }

    /// Return the value of one tmux option, or `None` when it is unset.
    ///
    /// `global` selects the same scope as [`Self::show_options`]: the global
    /// table with it, the session table without it. tmux resolves the option
    /// name against the table that declares it, so window options such as
    /// `automatic-rename` are reachable either way.
    ///
    /// `-v` asks for the value alone; without it tmux prints `name value`.
    /// `-q` turns an unknown option into empty output rather than an error.
    pub fn show_option(&self, option_name: &str, global: bool) -> Result<Option<String>> {
        let mut args = vec!["show-options", "-q", "-v"];
        if global {
            args.push("-g");
        }
        args.push(option_name);

        let output = self.run(&args)?.output("show-options")?;
        let buffer = String::from_utf8(output)?;
        let buffer = buffer.trim_end();

        if buffer.is_empty() {
            return Ok(None);
        }
        Ok(Some(buffer.to_string()))
    }

    /// Return every tmux option as a `HashMap`.
    pub fn show_options(&self, global: bool) -> Result<HashMap<String, String>> {
        let args = if global {
            vec!["show-options", "-g"]
        } else {
            vec!["show-options"]
        };

        let output = self.run(&args)?.output("show-options")?;
        let buffer = String::from_utf8(output)?;

        Ok(parse_options(&buffer))
    }

    /// Return the `default-command` used to start a pane, falling back to
    /// `default-shell` when unset.
    ///
    /// In the case of bash, a `-l` flag is added.
    pub fn default_command(&self) -> Result<String> {
        let all_options = self.show_options(true)?;

        let default_shell = all_options
            .get("default-shell")
            .ok_or(Error::TmuxConfig("no default-shell"))
            .map(|cmd| cmd.to_owned())
            .map(|cmd| {
                if cmd.ends_with("bash") {
                    format!("-l {cmd}")
                } else {
                    cmd
                }
            })?;

        all_options
            .get("default-command")
            .or(Some(&default_shell))
            .ok_or(Error::TmuxConfig("no default-command nor default-shell"))
            .map(|cmd| cmd.to_owned())
    }
}
