//! Client-level operations: reporting state, and messaging the user.

use crate::{
    Result,
    client::Client,
    error::{Error, map_byte_parse_error},
    tmux::Tmux,
    wire::{
        ByteParseError, RecordReader, decode_all, decode_one,
        formats::{
            CLIENT_FIELDS, CLIENT_FORMAT, CLIENT_INTENT, CLIENT_LIST_FIELDS, CLIENT_LIST_FORMAT,
            CLIENT_LIST_INTENT,
        },
        normalize_tmux_output,
    },
};

/// One `list-clients` row: a client's name, and when it was last active.
struct ClientActivity {
    activity: u64,
    name: String,
}

impl ClientActivity {
    fn decode(
        reader: &mut RecordReader<'_, '_>,
    ) -> std::result::Result<ClientActivity, ByteParseError> {
        let activity = reader
            .token("client activity")?
            .parse()
            .map_err(|_| ByteParseError::new("invalid client activity"))?;
        let name = reader.required_data("client name")?;

        Ok(ClientActivity { activity, name })
    }
}

impl Tmux {
    /// Return the attributes of the client issuing this command, or `None`
    /// when the caller is not running inside one.
    ///
    /// Outside a client — a scheduler, a cron job, a plain shell — tmux has
    /// nothing to resolve `#{client_session}` against and answers with an
    /// empty session. That is a state to report, not a failure: see
    /// [`Self::most_recent_client_name`] for picking a client in that case.
    ///
    /// # Errors
    ///
    /// Returns an error if tmux fails or emits a malformed client record.
    pub fn current_client(&self) -> Result<Option<Client>> {
        let client =
            self.client_record(&["display-message", "-p", "-F", CLIENT_FORMAT.as_str()])?;

        Ok((!client.session_name.is_empty()).then_some(client))
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
        let output = self.run(argv)?.output("display-message")?;
        let stdout = normalize_tmux_output(&output)
            .map_err(|e| map_byte_parse_error("Client", CLIENT_INTENT.as_str(), e))?;
        decode_one(&stdout, CLIENT_FIELDS, Client::decode)
            .map_err(|e| map_byte_parse_error("Client", CLIENT_INTENT.as_str(), e))
    }

    /// Return the name of the client issuing this command, such as
    /// `/dev/ttys002`.
    ///
    /// This is the target other clients are addressed by; see
    /// [`Self::client_for_target`] and [`Self::display_message_to`].
    ///
    /// # Errors
    ///
    /// Returns an error if tmux fails, or if it names no client — which is
    /// what happens outside a tmux client, where there is nothing to name.
    pub fn current_client_name(&self) -> Result<String> {
        let output = self
            .run(&["display-message", "-p", "-F", "#{client_name}"])?
            .output("display-message")?;

        let name = String::from_utf8(output)?.trim_end().to_string();
        if name.is_empty() {
            return Err(Error::TmuxConfig("tmux named no current client"));
        }

        Ok(name)
    }

    /// Return the name of the attached client that was active most recently,
    /// or `None` when no client is attached.
    ///
    /// This is how a process running outside tmux — a scheduler, a hook —
    /// picks a client to report to.
    ///
    /// # Errors
    ///
    /// Returns an error if tmux fails or emits a malformed client record.
    pub fn most_recent_client_name(&self) -> Result<Option<String>> {
        let output = self
            .run(&["list-clients", "-F", CLIENT_LIST_FORMAT.as_str()])?
            .output("list-clients")?;

        let stdout = normalize_tmux_output(&output)
            .map_err(|e| map_byte_parse_error("Client", CLIENT_LIST_INTENT.as_str(), e))?;
        let clients = decode_all(&stdout, CLIENT_LIST_FIELDS, ClientActivity::decode)
            .map_err(|e| map_byte_parse_error("Client", CLIENT_LIST_INTENT.as_str(), e))?;

        Ok(most_recent(clients))
    }

    /// Display `message` in the status line of the current client.
    pub fn display_message(&self, message: &str) -> Result<()> {
        self.run(&["display-message", message])?
            .no_output("display-message")
    }

    /// Display `message` in the status line of the client named `target`.
    ///
    /// Use this when the caller is not itself a tmux client and has picked one
    /// with [`Self::most_recent_client_name`].
    ///
    /// The client is selected with `-c`, not `-t`. `-t` is a target *pane*; it
    /// accepts a client name and resolves formats against that client, which
    /// is why [`Self::client_for_target`] uses it, but it does not choose
    /// where a message is shown. With two clients attached, `-t <client>`
    /// delivers the message to the other one.
    ///
    /// Delivery is best effort: tmux reports no error for a client name that
    /// does not exist, it just shows the message somewhere else.
    ///
    /// # Compatibility
    ///
    /// Requires tmux 3.3 or later. tmux 3.2 declares `-c` as taking no
    /// argument — `.args = { "acd:INpt:F:v", 0, 1 }`, missing the colon that
    /// 3.3 added — so the client name is parsed as a second positional
    /// argument and tmux answers with its usage string. Nothing else on 3.2
    /// targets a client either: `-t` names a pane, and the message still goes
    /// to the current client. [`Self::display_message`] is unaffected.
    pub fn display_message_to(&self, target: &str, message: &str) -> Result<()> {
        self.run(&["display-message", "-c", target, message])?
            .no_output("display-message")
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

        self.run(&["switch-client", "-t", &exact_session_name])?
            .no_output("switch-client")
    }
}

/// Pick the name of the client that was active most recently.
fn most_recent(clients: Vec<ClientActivity>) -> Option<String> {
    clients
        .into_iter()
        .max_by_key(|client| client.activity)
        .map(|client| client.name)
}

#[cfg(test)]
mod tests {
    use super::{ClientActivity, most_recent};
    use crate::wire::{decode_all, formats::CLIENT_LIST_FIELDS};

    /// Frame a `list-clients` reply the way tmux would.
    fn framed(rows: &[(&str, &str)]) -> Vec<u8> {
        let mut record = Vec::new();
        for (activity, name) in rows {
            record.extend_from_slice(activity.as_bytes());
            record.push(0x1f);
            record.extend_from_slice(name.len().to_string().as_bytes());
            record.push(0x1f);
            record.extend_from_slice(name.as_bytes());
            record.push(b'\n');
        }
        record
    }

    fn decode(rows: &[(&str, &str)]) -> Vec<ClientActivity> {
        decode_all(&framed(rows), CLIENT_LIST_FIELDS, ClientActivity::decode).unwrap()
    }

    #[test]
    fn selects_the_most_recently_active_client() {
        let clients = decode(&[
            ("10", "/dev/ttys001"),
            ("22", "/dev/ttys002"),
            ("15", "/dev/ttys003"),
        ]);

        assert_eq!(most_recent(clients).as_deref(), Some("/dev/ttys002"));
    }

    #[test]
    fn no_client_is_none() {
        assert_eq!(most_recent(decode(&[])), None);
    }

    #[test]
    fn a_non_numeric_activity_is_a_parse_error() {
        // The old hand-rolled reader skipped rows it could not parse, which
        // would silently report the wrong client. A malformed record is a
        // failure now.
        let record = framed(&[("not-a-number", "/dev/ttys001")]);

        assert!(decode_all(&record, CLIENT_LIST_FIELDS, ClientActivity::decode).is_err());
    }

    #[test]
    fn a_client_name_may_contain_a_separator() {
        let clients = decode(&[("5", "/dev/pts/1\tweird\nname")]);

        assert_eq!(
            most_recent(clients).as_deref(),
            Some("/dev/pts/1\tweird\nname")
        );
    }
}
