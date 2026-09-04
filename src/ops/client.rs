//! Client-level operations: reporting state, and messaging the user.

use crate::{
    Result,
    client::Client,
    error::{check_empty_process_output, check_process_success, map_byte_parse_error},
    tmux::Tmux,
    wire::{
        decode_one,
        formats::{CLIENT_FIELDS, CLIENT_FORMAT, CLIENT_INTENT},
        normalize_tmux_output,
    },
};

impl Tmux {
    /// Return the attributes of the client issuing this command.
    ///
    /// # Errors
    ///
    /// Returns an error if tmux fails or emits a malformed client record.
    pub fn current_client(&self) -> Result<Client> {
        self.client_record(&["display-message", "-p", "-F", CLIENT_FORMAT.as_str()])
    }

    /// Return the attributes of the client attached to `target`.
    ///
    /// Use this rather than building a `display-message` format by hand: the
    /// record format is this crate's own, and a caller that spells it out will
    /// silently drift from it.
    pub fn client_for_target(&self, target: &str) -> Result<Client> {
        self.client_record(&[
            "display-message",
            "-t",
            target,
            "-p",
            "-F",
            CLIENT_FORMAT.as_str(),
        ])
    }

    fn client_record(&self, argv: &[&str]) -> Result<Client> {
        let output = self.output(argv)?;
        check_process_success(&output, "display-message")?;
        let stdout = normalize_tmux_output(&output.stdout)
            .map_err(|e| map_byte_parse_error("Client", CLIENT_INTENT.as_str(), e))?;
        decode_one(&stdout, CLIENT_FIELDS, Client::decode)
            .map_err(|e| map_byte_parse_error("Client", CLIENT_INTENT.as_str(), e))
    }

    /// Display `message` in the status line of the current client.
    pub fn display_message(&self, message: &str) -> Result<()> {
        let output = self.output(&["display-message", message])?;
        check_empty_process_output(&output, "display-message")
    }

    /// Switch the current client to the session exactly named `session_name`.
    ///
    /// An empty name is a no-op: `Client::last_session_name` is legitimately
    /// empty when there is no previous session, and switching to nothing is
    /// not a failure.
    pub fn switch_client(&self, session_name: &str) -> Result<()> {
        if session_name.is_empty() {
            return Ok(());
        }

        let exact_session_name = format!("={session_name}");

        let output = self.output(&["switch-client", "-t", &exact_session_name])?;
        check_empty_process_output(&output, "switch-client")
    }
}
