//! Pane-level operations.

use std::str::FromStr;

use crate::{
    Result,
    error::{check_empty_process_output, check_process_success, map_byte_parse_error},
    pane::Pane,
    pane_id::PaneId,
    tmux::Tmux,
    window_id::WindowId,
    wire::{
        decode_all,
        formats::{NEW_PANE_FORMAT, PANE_FIELDS, PANE_FORMAT, PANE_INTENT},
        normalize_tmux_output,
    },
};

impl Tmux {
    /// Return every `Pane` from every session.
    pub fn available_panes(&self) -> Result<Vec<Pane>> {
        let output = self.output(&["list-panes", "-a", "-F", PANE_FORMAT.as_str()])?;
        check_process_success(&output, "list-panes")?;
        let stdout = normalize_tmux_output(&output.stdout)
            .map_err(|e| map_byte_parse_error("Pane", PANE_INTENT.as_str(), e))?;
        decode_all(&stdout, PANE_FIELDS, Pane::decode)
            .map_err(|e| map_byte_parse_error("Pane", PANE_INTENT.as_str(), e))
    }

    /// Return the entire scrollback of the pane with `pane_id`.
    ///
    /// # Note
    ///
    /// The output keeps escape codes and joined lines with trailing spaces,
    /// because tmux cannot both preserve escape codes and trim lines. Pass the
    /// result through [`crate::utils::cleanup_captured_buffer`].
    pub fn capture_pane(&self, pane_id: &PaneId) -> Result<Vec<u8>> {
        let output = self.output(&[
            "capture-pane",
            "-t",
            pane_id.as_str(),
            "-J", // preserves trailing spaces & joins any wrapped lines
            "-e", // include escape sequences for text & background
            "-p", // output goes to stdout
            "-S", // starting line number
            "-",  // start of history
            "-E", // ending line number
            "-",  // end of history
        ])?;

        Ok(output.stdout)
    }

    /// Create a pane by splitting the window with `window_id` horizontally,
    /// and return the new pane's id.
    pub fn new_pane(
        &self,
        reference_pane: &Pane,
        pane_command: Option<&str>,
        window_id: &WindowId,
    ) -> Result<PaneId> {
        let dirpath = reference_pane
            .dirpath
            .to_str()
            .ok_or(crate::error::Error::TmuxConfig(
                "pane working directory is not valid UTF-8",
            ))?;

        let mut args = vec![
            "split-window",
            "-h",
            "-c",
            dirpath,
            "-t",
            window_id.as_str(),
            "-P",
            "-F",
            NEW_PANE_FORMAT,
        ];
        if let Some(pane_command) = pane_command {
            args.push(pane_command);
        }

        let output = self.output(&args)?;

        // Check exit status before parsing to avoid confusing parse errors
        // when tmux fails and returns empty/garbage stdout.
        check_process_success(&output, "split-window")?;

        let buffer = String::from_utf8(output.stdout)?;

        PaneId::from_str(buffer.trim_end())
    }

    /// Select (make active) the pane with `pane_id`.
    pub fn select_pane(&self, pane_id: &PaneId) -> Result<()> {
        let output = self.output(&["select-pane", "-t", pane_id.as_str()])?;
        check_empty_process_output(&output, "select-pane")
    }
}
