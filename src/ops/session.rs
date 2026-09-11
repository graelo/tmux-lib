//! Session-level operations.

use nom::{Parser, character::complete::char, combinator::all_consuming};

use crate::{
    Result,
    error::{map_add_intent, map_byte_parse_error},
    pane::Pane,
    pane_id::{PaneId, parse::pane_id},
    session::Session,
    session_id::{SessionId, parse::session_id},
    tmux::Tmux,
    window::Window,
    window_id::{WindowId, parse::window_id},
    wire::{
        decode_all,
        formats::{
            NEW_SESSION_FORMAT, NEW_SESSION_INTENT, SESSION_FIELDS, SESSION_FORMAT, SESSION_INTENT,
        },
        normalize_tmux_output,
    },
};

impl Tmux {
    /// Return every `Session` on the server.
    pub fn available_sessions(&self) -> Result<Vec<Session>> {
        let output = self
            .run(&["list-sessions", "-F", SESSION_FORMAT.as_str()])?
            .output("list-sessions")?;
        let stdout = normalize_tmux_output(&output)
            .map_err(|e| map_byte_parse_error("Session", SESSION_INTENT.as_str(), e))?;
        decode_all(&stdout, SESSION_FIELDS, Session::decode)
            .map_err(|e| map_byte_parse_error("Session", SESSION_INTENT.as_str(), e))
    }

    /// Create a session, and with it a window and a pane.
    ///
    /// The session name is taken from `session`, and the working directory
    /// from `pane`.
    pub fn new_session(
        &self,
        session: &Session,
        window: &Window,
        pane: &Pane,
        pane_command: Option<&str>,
    ) -> Result<(SessionId, WindowId, PaneId)> {
        let dirpath = pane
            .dirpath
            .to_str()
            .ok_or(crate::error::Error::TmuxConfig(
                "pane working directory is not valid UTF-8",
            ))?;

        let mut args = vec![
            "new-session",
            "-d",
            "-c",
            dirpath,
            "-s",
            &session.name,
            "-n",
            &window.name,
            "-P",
            "-F",
            NEW_SESSION_FORMAT,
        ];
        if let Some(pane_command) = pane_command {
            args.push(pane_command);
        }

        // Forks a client: the session and window names and the working
        // directory are not this crate's to vouch for, and the control
        // connection takes one command per line.
        //
        // The reply is checked before parsing, so that a failing tmux does
        // not surface as a confusing parse error over whatever it printed.
        let output = self.run_spawned(&args)?.output("new-session")?;

        let buffer = String::from_utf8(output)?;
        let buffer = buffer.trim_end();

        let (_, (new_session_id, _, new_window_id, _, new_pane_id)) =
            all_consuming((session_id, char(':'), window_id, char(':'), pane_id))
                .parse(buffer)
                .map_err(|e| map_add_intent("new-session", NEW_SESSION_INTENT, e))?;

        Ok((new_session_id, new_window_id, new_pane_id))
    }
}
