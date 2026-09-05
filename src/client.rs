//! Client-level functions: for representing client state (`client_session` etc) or reporting information inside Tmux.

use serde::{Deserialize, Serialize};

use crate::wire::{ByteParseError, RecordReader};

/// A Tmux client.
///
/// The default value — both session names empty — is what a caller running
/// outside any tmux client has to record: there is no client to describe. See
/// [`crate::Tmux::current_client`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Client {
    /// The current session.
    pub session_name: String,
    /// The last session.
    pub last_session_name: String,
}

impl Client {
    /// Build a `Client` from one framed record, reading the fields declared in
    /// [`CLIENT_FIELDS`].
    pub(crate) fn decode(
        reader: &mut RecordReader<'_, '_>,
    ) -> std::result::Result<Client, ByteParseError> {
        Ok(Client {
            // Not `required_data`: tmux answers `display-message` with an
            // empty session when it has no client to resolve against, and
            // that is a state to report rather than a malformed record.
            session_name: reader.data("client session")?,
            last_session_name: reader.data("last client session")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Client;
    use crate::wire::formats::{CLIENT_FIELDS, CLIENT_INTENT};
    use crate::wire::framing::{FIELD_SEPARATOR, RECORD_SEPARATOR};
    use crate::wire::{decode_all, decode_one};

    fn parse_all(input: &[u8]) -> crate::Result<Vec<Client>> {
        decode_all(input, CLIENT_FIELDS, Client::decode)
            .map_err(|e| crate::error::map_byte_parse_error("Client", CLIENT_INTENT.as_str(), e))
    }

    fn parse_one(input: &str) -> crate::Result<Client> {
        decode_one(input.as_bytes(), CLIENT_FIELDS, Client::decode)
            .map_err(|e| crate::error::map_byte_parse_error("Client", CLIENT_INTENT.as_str(), e))
    }

    #[test]
    fn parse_client_with_both_sessions() {
        let input =
            String::from_utf8(framed_client_record(b"current-session", b"last-session")).unwrap();
        let client = parse_one(&input).expect("Should parse valid client");

        assert_eq!(client.session_name, "current-session");
        assert_eq!(client.last_session_name, "last-session");
    }

    #[test]
    fn parse_client_with_empty_last_session() {
        // When there's no previous session, last_session is empty.
        let input = String::from_utf8(framed_client_record(b"my-session", b"")).unwrap();
        let client = parse_one(&input).expect("Should parse client with empty last session");

        assert_eq!(client.session_name, "my-session");
        assert_eq!(client.last_session_name, "");
    }

    #[test]
    fn parse_client_with_special_chars_in_name() {
        let input = String::from_utf8(framed_client_record(b"server: $123", b"dev-env")).unwrap();
        let client = parse_one(&input).expect("Should parse client with special chars");

        assert_eq!(client.session_name, "server: $123");
        assert_eq!(client.last_session_name, "dev-env");
    }

    #[test]
    fn parse_client_with_no_session_at_all() {
        // Outside a tmux client, tmux resolves both session formats to empty.
        // `Tmux::current_client` turns this into `None`; the decoder's job is
        // only to read it back faithfully.
        let input = String::from_utf8(framed_client_record(b"", b"")).unwrap();
        let client = parse_one(&input).expect("an empty client record is well formed");

        assert_eq!(client.session_name, "");
        assert_eq!(client.last_session_name, "");
    }

    #[test]
    fn parse_client_rejects_legacy_format() {
        let result = parse_one("'current-session':'last-session'");

        assert!(result.is_err());
    }

    #[test]
    fn parse_client_fails_on_invalid_length() {
        let input = "not-a-number\x1fcurrent\x1f4\x1flast\n";
        let result = parse_one(input);

        assert!(result.is_err());
    }

    #[test]
    fn parse_client_fails_on_extra_content() {
        let input = "7\x1fcurrent\x1f4\x1flast\nextra";
        let result = parse_one(input);

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
    fn parse_framed_client_preserves_arbitrary_utf8_data() {
        let session = "π's: \\\x1f# $;\n";
        let last_session = "last\\session:two";
        let input = String::from_utf8(framed_client_record(
            session.as_bytes(),
            last_session.as_bytes(),
        ))
        .unwrap();

        let client = parse_one(&input).unwrap();

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
        assert!(parse_all(&missing_terminator).is_err());
        assert!(parse_all(&trailing_bytes).is_err());
        assert!(parse_all(&invalid_utf8).is_err());
        assert!(parse_all(invalid_length).is_err());
    }
}
