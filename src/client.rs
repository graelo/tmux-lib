//! Client-level functions: for representing client state (`client_session` etc) or reporting information inside Tmux.

use std::str::FromStr;

use nom::{Parser, character::complete::char, combinator::all_consuming};
use serde::{Deserialize, Serialize};
use smol::process::Command;

use crate::{
    Result,
    error::{Error, check_process_success, map_add_intent, map_byte_parse_error},
    parse::{
        ByteCursor, ByteParseError, FIELD_SEPARATOR, RECORD_SEPARATOR, quoted_nonempty_string,
        quoted_string,
    },
};

/// Format used by [`current`] for a newline-terminated client record.
const CLIENT_FORMAT: &str = "#{n:client_session}\x1f#{client_session}\x1f#{n:client_last_session}\x1f#{client_last_session}";
const CLIENT_INTENT: &str = "#{n:client_session}\\x1f#{client_session}\\x1f#{n:client_last_session}\\x1f#{client_last_session}\\n";

/// A Tmux client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Client {
    /// The current session.
    pub session_name: String,
    /// The last session.
    pub last_session_name: String,
}

impl FromStr for Client {
    type Err = Error;

    /// Parse a string containing client information into a new `Client`.
    ///
    /// This returns a `Result<Client, Error>` as this call can obviously
    /// fail if provided an invalid format.
    ///
    /// The legacy format accepted by this parser is
    ///
    /// ```text
    /// 'name-of-current-session':'name-of-last-session'
    /// ```
    ///
    /// The preferred status format is a byte-framed, newline-terminated
    /// record:
    ///
    /// ```text
    /// #{n:client_session}\x1f#{client_session}\x1f#{n:client_last_session}\x1f#{client_last_session}\n
    /// ```
    ///
    /// The framed status is obtained with
    ///
    /// ```text
    /// tmux display-message -p -F "#{n:client_session}\x1f#{client_session}\x1f#{n:client_last_session}\x1f#{client_last_session}"
    /// ```
    ///
    /// For definitions, look at `Client` type and the tmux man page for
    /// definitions.
    fn from_str(input: &str) -> std::result::Result<Self, Self::Err> {
        let desc = "Client";
        if matches!(input.as_bytes().first(), Some(b'0'..=b'9')) {
            return parse_framed_client(input.as_bytes())
                .map_err(|e| map_byte_parse_error(desc, CLIENT_INTENT, e));
        }

        let intent = "'##{client_session}':'##{client_last_session}'";
        let parser = (quoted_nonempty_string, char(':'), quoted_string);
        let (_, (session_name, _, last_session_name)) = all_consuming(parser)
            .parse(input)
            .map_err(|e| map_add_intent(desc, intent, e))?;

        Ok(Client {
            session_name: session_name.to_string(),
            last_session_name: last_session_name.to_string(),
        })
    }
}

fn parse_framed_client(input: &[u8]) -> std::result::Result<Client, ByteParseError> {
    let mut cursor = ByteCursor::new(input);
    let session_name = cursor.take_length_prefixed_string(FIELD_SEPARATOR, "client session")?;
    if session_name.is_empty() {
        return Err(ByteParseError::new("client session is empty"));
    }
    let last_session_name =
        cursor.take_length_prefixed_string(RECORD_SEPARATOR, "last client session")?;
    if !cursor.is_at_end() {
        return Err(ByteParseError::new(
            "unexpected trailing bytes after client record",
        ));
    }

    Ok(Client {
        session_name,
        last_session_name,
    })
}

// ------------------------------
// Ops
// ------------------------------

/// Return the current client useful attributes.
///
/// # Errors
///
/// Returns an error if tmux fails or emits a malformed client record.
pub async fn current() -> Result<Client> {
    let args = vec!["display-message", "-p", "-F", CLIENT_FORMAT];

    let output = Command::new("tmux").args(&args).output().await?;
    check_process_success(&output, "display-message")?;
    parse_framed_client(&output.stdout)
        .map_err(|e| map_byte_parse_error("Client", CLIENT_INTENT, e))
}

/// Return a list of all `Pane` from all sessions.
///
/// # Panics
///
/// This function panics if it can't communicate with Tmux.
pub fn display_message(message: &str) {
    let args = vec!["display-message", message];

    std::process::Command::new("tmux")
        .args(&args)
        .output()
        .expect("Cannot communicate with Tmux for displaying message");
}

/// Switch to session exactly named `session_name`.
///
/// # Panics
///
/// This function panics if it can't communicate with Tmux.
pub async fn switch_client(session_name: &str) -> Result<()> {
    let exact_session_name = format!("={session_name}");
    let args = vec!["switch-client", "-t", &exact_session_name];

    Command::new("tmux")
        .args(&args)
        .output()
        .await
        .expect("Cannot communicate with Tmux for switching the client");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Client;
    use super::parse_framed_client;
    use super::{FIELD_SEPARATOR, RECORD_SEPARATOR};
    use std::str::FromStr;

    #[test]
    fn parse_client_with_both_sessions() {
        let input = "'current-session':'last-session'";
        let client = Client::from_str(input).expect("Should parse valid client");

        assert_eq!(client.session_name, "current-session");
        assert_eq!(client.last_session_name, "last-session");
    }

    #[test]
    fn parse_client_with_empty_last_session() {
        // When there's no previous session, last_session is empty
        let input = "'my-session':''";
        let client = Client::from_str(input).expect("Should parse client with empty last session");

        assert_eq!(client.session_name, "my-session");
        assert_eq!(client.last_session_name, "");
    }

    #[test]
    fn parse_client_with_special_chars_in_name() {
        let input = "'server: $123':'dev-env'";
        let client = Client::from_str(input).expect("Should parse client with special chars");

        assert_eq!(client.session_name, "server: $123");
        assert_eq!(client.last_session_name, "dev-env");
    }

    #[test]
    fn parse_client_fails_on_empty_current_session() {
        // Current session should not be empty
        let input = "'':'last-session'";
        let result = Client::from_str(input);

        assert!(result.is_err());
    }

    #[test]
    fn parse_client_fails_on_missing_quotes() {
        let input = "current-session:last-session";
        let result = Client::from_str(input);

        assert!(result.is_err());
    }

    #[test]
    fn parse_client_fails_on_missing_colon() {
        let input = "'current-session''last-session'";
        let result = Client::from_str(input);

        assert!(result.is_err());
    }

    #[test]
    fn parse_client_fails_on_extra_content() {
        let input = "'current':'last':extra";
        let result = Client::from_str(input);

        assert!(result.is_err());
    }

    fn framed_client_record(session: &[u8], last_session: &[u8]) -> Vec<u8> {
        let mut record = Vec::new();
        append_field(&mut record, session, FIELD_SEPARATOR);
        append_field(&mut record, last_session, RECORD_SEPARATOR);
        record
    }

    fn append_field(record: &mut Vec<u8>, data: &[u8], terminator: u8) {
        record.extend_from_slice(data.len().to_string().as_bytes());
        record.push(FIELD_SEPARATOR);
        record.extend_from_slice(data);
        record.push(terminator);
    }

    #[test]
    fn parse_legacy_client_with_unit_separator() {
        let client = Client::from_str("'current\x1f':'last'").unwrap();

        assert_eq!(client.session_name, "current\x1f");
    }

    #[test]
    fn parse_framed_client_preserves_arbitrary_utf8_data() {
        let session = "π's: \\\x1f# $;\n";
        let last_session = "last\\session:two";
        let input = String::from_utf8(framed_client_record(
            session.as_bytes(),
            last_session.as_bytes(),
        ))
        .unwrap();

        let client = Client::from_str(&input).unwrap();

        assert_eq!(client.session_name, session);
        assert_eq!(client.last_session_name, last_session);
    }

    #[test]
    fn parse_framed_client_rejects_malformed_records() {
        let valid = framed_client_record(b"current", b"last");
        let mut missing_terminator = valid.clone();
        missing_terminator.pop();
        let mut trailing_bytes = valid.clone();
        trailing_bytes.extend_from_slice(b"trailing");
        let invalid_utf8 = framed_client_record(&[0xff], b"last");
        let invalid_length = b"7\x1fcurrent\x1fnot-a-number\x1flast\n";
        let empty_current = framed_client_record(b"", b"last");

        assert!(parse_framed_client(&missing_terminator).is_err());
        assert!(parse_framed_client(&trailing_bytes).is_err());
        assert!(parse_framed_client(&invalid_utf8).is_err());
        assert!(parse_framed_client(invalid_length).is_err());
        assert!(parse_framed_client(&empty_current).is_err());
    }
}
