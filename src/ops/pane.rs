//! Pane-level operations.

use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{
    Result,
    error::{Error, map_byte_parse_error},
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
        let output = self
            .run(&["list-panes", "-a", "-F", PANE_FORMAT.as_str()])?
            .output("list-panes")?;
        let stdout = normalize_tmux_output(&output)
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
    ///
    /// On a control handle the capture travels the connection, so captures
    /// from several threads take turns on it rather than running side by side.
    /// The bytes are checked against the length tmux reports for them, and a
    /// mismatch is an error rather than a short capture.
    pub fn capture_pane(&self, pane_id: &PaneId) -> Result<Vec<u8>> {
        if self.is_control() {
            return self.capture_pane_via_buffer(pane_id);
        }

        self.run(&[
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
        ])?
        .output("capture-pane")
    }

    /// Capture into a named paste buffer, then read it back with its length.
    ///
    /// A control connection frames a reply by its terminator line, and a pane
    /// may print that very line. A buffer has a size tmux states separately,
    /// so its bytes can be checked rather than trusted.
    ///
    /// The buffer is deleted whenever the capture made one, and a failure to
    /// delete it fails the call: it would otherwise stay on the user's server.
    fn capture_pane_via_buffer(&self, pane_id: &PaneId) -> Result<Vec<u8>> {
        let name = format!(
            "tmux-lib-{}-{}",
            std::process::id(),
            CAPTURE_BUFFERS.fetch_add(1, Ordering::Relaxed)
        );

        self.run(&[
            "capture-pane",
            "-b",
            &name,
            "-t",
            pane_id.as_str(),
            "-J",
            "-e",
            "-S",
            "-",
            "-E",
            "-",
        ])?
        .no_output("capture-pane")?;

        let contents = self.read_buffer(&name);
        let deleted = self
            .run(&["delete-buffer", "-b", &name])
            .and_then(|reply| reply.no_output("delete-buffer"));

        let contents = contents?;
        deleted?;
        Ok(contents)
    }

    fn read_buffer(&self, name: &str) -> Result<Vec<u8>> {
        let listing = self
            .run(&["list-buffers", "-F", BUFFER_LISTING])?
            .output("list-buffers")?;
        let size = buffer_size(&listing, name)?;

        let body = self
            .run(&["show-buffer", "-b", name])?
            .output("show-buffer")?;

        unframe_buffer(body, size)
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

        // Forks a client: `-c <dirpath>` carries a path this crate did not
        // choose, and the control connection takes one command per line.
        //
        // The reply is checked before parsing, so that a failing tmux does
        // not surface as a confusing parse error over whatever it printed.
        let output = self.run_spawned(&args)?.output("split-window")?;

        let buffer = String::from_utf8(output)?;

        PaneId::from_str(buffer.trim_end())
    }

    /// Select (make active) the pane with `pane_id`.
    pub fn select_pane(&self, pane_id: &PaneId) -> Result<()> {
        self.run(&["select-pane", "-t", pane_id.as_str()])?
            .no_output("select-pane")
    }
}

/// Numbers capture buffers, so that no two captures in this process share
/// one, whichever handle or thread runs them.
static CAPTURE_BUFFERS: AtomicU64 = AtomicU64::new(0);

/// One `list-buffers` row: the size first, so a name cannot be mistaken for
/// it.
const BUFFER_LISTING: &str = "#{buffer_size} #{buffer_name}";

/// The size tmux reports for the buffer called `name`.
fn buffer_size(listing: &[u8], name: &str) -> Result<usize> {
    let row = listing
        .split(|&b| b == b'\n')
        .filter_map(|row| std::str::from_utf8(row).ok()?.split_once(' '))
        .find(|(_, row_name)| *row_name == name)
        .ok_or_else(|| buffer_error(format!("tmux listed no buffer named {name}")))?;

    row.0
        .parse()
        .map_err(|_| buffer_error(format!("invalid size {:?} for buffer {name}", row.0)))
}

/// A buffer's contents from its `show-buffer` reply, given its size.
///
/// On a control connection the reply is the buffer followed by one newline
/// of tmux's own. Anything else means bytes were added or lost on the way.
fn unframe_buffer(mut body: Vec<u8>, size: usize) -> Result<Vec<u8>> {
    if body.len() != size + 1 || body.last() != Some(&b'\n') {
        return Err(buffer_error(format!(
            "tmux reported {size} bytes and sent {}, expected one more ending in a newline",
            body.len()
        )));
    }

    body.pop();
    Ok(body)
}

fn buffer_error(message: String) -> Error {
    Error::ParseError {
        desc: "capture buffer",
        intent: BUFFER_LISTING,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::{buffer_size, unframe_buffer};

    #[test]
    fn a_buffer_of_the_reported_size_loses_only_the_final_newline() {
        assert_eq!(unframe_buffer(b"ab\n\n".to_vec(), 3).unwrap(), b"ab\n");
    }

    #[test]
    fn an_empty_buffer_is_a_lone_newline() {
        assert_eq!(unframe_buffer(b"\n".to_vec(), 0).unwrap(), b"");
    }

    #[test]
    fn a_short_reply_is_an_error() {
        assert!(unframe_buffer(b"ab\n".to_vec(), 3).is_err());
    }

    #[test]
    fn a_long_reply_is_an_error() {
        assert!(unframe_buffer(b"abc\n\n".to_vec(), 3).is_err());
    }

    #[test]
    fn a_reply_not_ending_in_a_newline_is_an_error() {
        assert!(unframe_buffer(b"ab\nX".to_vec(), 3).is_err());
    }

    #[test]
    fn the_size_is_read_from_the_named_buffer_only() {
        let listing = b"10 buffer0\n42 tmux-lib-1-0\n7 tmux-lib-1-00\n";

        assert_eq!(buffer_size(listing, "tmux-lib-1-0").unwrap(), 42);
    }

    #[test]
    fn a_name_holding_a_space_does_not_confuse_the_size() {
        let listing = b"5 my buffer\n";

        assert_eq!(buffer_size(listing, "my buffer").unwrap(), 5);
    }

    #[test]
    fn a_missing_buffer_is_an_error() {
        assert!(buffer_size(b"10 buffer0\n", "tmux-lib-1-0").is_err());
    }
}
