//! Window-level operations.

use nom::{Parser, character::complete::char, combinator::all_consuming};

use crate::{
    Result,
    error::{map_add_intent, map_byte_parse_error},
    pane::Pane,
    pane_id::{PaneId, parse::pane_id},
    session::Session,
    tmux::Tmux,
    window::Window,
    window_id::{WindowId, parse::window_id},
    wire::{
        decode_all,
        formats::{
            NEW_WINDOW_FORMAT, NEW_WINDOW_INTENT, WINDOW_FIELDS, WINDOW_FORMAT, WINDOW_INTENT,
        },
        normalize_tmux_output,
    },
};

impl Tmux {
    /// Return every `Window` from every session.
    pub fn available_windows(&self) -> Result<Vec<Window>> {
        let output = self
            .run(&["list-windows", "-a", "-F", WINDOW_FORMAT.as_str()])?
            .output("list-windows")?;
        let stdout = normalize_tmux_output(&output)
            .map_err(|e| map_byte_parse_error("Window", WINDOW_INTENT.as_str(), e))?;
        decode_all(&stdout, WINDOW_FIELDS, Window::decode)
            .map_err(|e| map_byte_parse_error("Window", WINDOW_INTENT.as_str(), e))
    }

    /// Create a window in `session`.
    ///
    /// The window name is taken from `window`, and the working directory from
    /// `pane`.
    pub fn new_window(
        &self,
        session: &Session,
        window: &Window,
        pane: &Pane,
        pane_command: Option<&str>,
    ) -> Result<(WindowId, PaneId)> {
        // Target by session id: it is unambiguous and immediately valid after
        // session creation, unlike a name, which may contain a colon or be
        // briefly unresolvable.
        let target_session = session.id.as_str();
        let dirpath = pane
            .dirpath
            .to_str()
            .ok_or(crate::error::Error::TmuxConfig(
                "pane working directory is not valid UTF-8",
            ))?;

        let mut args = vec![
            "new-window",
            "-d",
            "-c",
            dirpath,
            "-n",
            &window.name,
            "-t",
            target_session,
            "-P",
            "-F",
            NEW_WINDOW_FORMAT,
        ];
        if let Some(pane_command) = pane_command {
            args.push(pane_command);
        }

        // Forks a client: the window name and the working directory are not
        // this crate's to vouch for, and the control connection takes one
        // command per line.
        //
        // The reply is checked before parsing, so that a failing tmux does
        // not surface as a confusing parse error over whatever it printed.
        let output = self.run_spawned(&args)?.output("new-window")?;

        let buffer = String::from_utf8(output)?;
        let buffer = buffer.trim_end();

        let (_, (new_window_id, _, new_pane_id)) = all_consuming((window_id, char(':'), pane_id))
            .parse(buffer)
            .map_err(|e| map_add_intent("new-window", NEW_WINDOW_INTENT, e))?;

        Ok((new_window_id, new_pane_id))
    }

    /// Apply `layout` to the window with `window_id`.
    pub fn set_layout(&self, layout: &str, window_id: &WindowId) -> Result<()> {
        self.run(&["select-layout", "-t", window_id.as_str(), layout])?
            .no_output("select-layout")
    }

    /// Select (make active) the window with `window_id`.
    pub fn select_window(&self, window_id: &WindowId) -> Result<()> {
        self.run(&["select-window", "-t", window_id.as_str()])?
            .no_output("select-window")
    }
}
