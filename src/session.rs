//! This module provides a few types and functions to handle Tmux sessions.
//!
//! The main use cases are running Tmux commands & parsing Tmux session
//! information.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{
    session_id::SessionId,
    wire::{ByteParseError, RecordReader},
};

/// A Tmux session.
///
/// Values are decoded from a framed `list-sessions` record; see
/// [`crate::Tmux::available_sessions`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    /// Session identifier, e.g. `$3`.
    pub id: SessionId,
    /// Name of the session.
    pub name: String,
    /// Working directory of the session.
    pub dirpath: PathBuf,
}

impl Session {
    /// Build a `Session` from one framed record, reading the fields declared
    /// in [`SESSION_FIELDS`].
    pub(crate) fn decode(
        reader: &mut RecordReader<'_, '_>,
    ) -> std::result::Result<Session, ByteParseError> {
        let id = reader
            .token("session ID")?
            .parse()
            .map_err(|_| ByteParseError::new("invalid session ID"))?;
        let name = reader.required_data("session name")?;
        let dirpath = reader.data("session path")?;

        Ok(Session {
            id,
            name,
            dirpath: dirpath.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Session;
    use super::SessionId;
    use crate::wire::formats::{SESSION_FIELDS, SESSION_INTENT};
    use crate::wire::framing::{FIELD_SEPARATOR, RECORD_SEPARATOR};
    use crate::wire::{decode_all, decode_one};

    fn parse_all(input: &[u8]) -> crate::Result<Vec<Session>> {
        decode_all(input, SESSION_FIELDS, Session::decode)
            .map_err(|e| crate::error::map_byte_parse_error("Session", SESSION_INTENT.as_str(), e))
    }

    fn parse_one(input: &str) -> crate::Result<Session> {
        decode_one(input.as_bytes(), SESSION_FIELDS, Session::decode)
            .map_err(|e| crate::error::map_byte_parse_error("Session", SESSION_INTENT.as_str(), e))
    }
    use crate::Result;
    use std::path::PathBuf;
    use std::str::FromStr;

    #[test]
    fn parse_list_sessions() {
        let output = [
            String::from_utf8(framed_session_record(
                b"$1",
                b"pytorch",
                b"/Users/graelo/ml/pytorch",
            ))
            .unwrap(),
            String::from_utf8(framed_session_record(b"$2", b"rust", b"/Users/graelo/rust"))
                .unwrap(),
            String::from_utf8(framed_session_record(
                b"$3",
                b"server: $",
                b"/Users/graelo/swift",
            ))
            .unwrap(),
            String::from_utf8(framed_session_record(
                b"$4",
                b"tmux-hacking",
                b"/Users/graelo/tmux",
            ))
            .unwrap(),
        ];
        let sessions: Result<Vec<Session>> = output.iter().map(|line| parse_one(line)).collect();
        let sessions = sessions.expect("Could not parse tmux sessions");

        let expected = vec![
            Session {
                id: SessionId::from_str("$1").unwrap(),
                name: String::from("pytorch"),
                dirpath: PathBuf::from("/Users/graelo/ml/pytorch"),
            },
            Session {
                id: SessionId::from_str("$2").unwrap(),
                name: String::from("rust"),
                dirpath: PathBuf::from("/Users/graelo/rust"),
            },
            Session {
                id: SessionId::from_str("$3").unwrap(),
                name: String::from("server: $"),
                dirpath: PathBuf::from("/Users/graelo/swift"),
            },
            Session {
                id: SessionId::from_str("$4").unwrap(),
                name: String::from("tmux-hacking"),
                dirpath: PathBuf::from("/Users/graelo/tmux"),
            },
        ];

        assert_eq!(sessions, expected);
    }

    #[test]
    fn parse_session_with_large_id() {
        let input = String::from_utf8(framed_session_record(
            b"$999",
            b"large-id-session",
            b"/home/user/projects",
        ))
        .unwrap();
        let session = parse_one(&input).expect("Should parse session with large id");

        assert_eq!(session.id, SessionId::from_str("$999").unwrap());
        assert_eq!(session.name, "large-id-session");
        assert_eq!(session.dirpath, PathBuf::from("/home/user/projects"));
    }

    #[test]
    fn parse_session_with_spaces_in_path() {
        let input = String::from_utf8(framed_session_record(
            b"$5",
            b"dev",
            b"/Users/user/My Projects/rust",
        ))
        .unwrap();
        let session = parse_one(&input).expect("Should parse session with spaces in path");

        assert_eq!(session.name, "dev");
        assert_eq!(
            session.dirpath,
            PathBuf::from("/Users/user/My Projects/rust")
        );
    }

    #[test]
    fn parse_session_with_unicode_in_name() {
        let input = String::from_utf8(framed_session_record(
            b"$6",
            "项目-日本語".as_bytes(),
            b"/home/user/code",
        ))
        .unwrap();
        let session = parse_one(&input).expect("Should parse session with unicode name");

        assert_eq!(session.name, "项目-日本語");
    }

    #[test]
    fn parse_session_fails_on_missing_id() {
        let input = String::from_utf8(framed_session_record(
            b"bad",
            b"session-name",
            b"/path/to/dir",
        ))
        .unwrap();
        let result = parse_one(&input);

        assert!(result.is_err());
    }

    #[test]
    fn parse_session_rejects_legacy_format() {
        let result = parse_one("$1:'session-name':/path/to/dir");

        assert!(result.is_err());
    }

    #[test]
    fn parse_session_fails_on_empty_name() {
        let input = String::from_utf8(framed_session_record(b"$1", b"", b"/path/to/dir")).unwrap();
        let result = parse_one(&input);

        assert!(result.is_err());
    }

    #[test]
    fn parse_session_fails_on_malformed_id() {
        let input = String::from_utf8(framed_session_record(b"@1", b"session", b"/path")).unwrap();
        let result = parse_one(&input); // @ is window prefix, not session.

        assert!(result.is_err());
    }

    #[test]
    fn parse_session_with_colon_in_path() {
        // Paths can contain colons (e.g., Windows-style paths or special paths).
        let input = String::from_utf8(framed_session_record(
            b"$7",
            b"test",
            b"/path/with:colon/here",
        ))
        .unwrap();
        let session = parse_one(&input).expect("Should parse session with colon in path");

        assert_eq!(session.dirpath, PathBuf::from("/path/with:colon/here"));
    }

    fn framed_session_record(id: &[u8], name: &[u8], path: &[u8]) -> Vec<u8> {
        let mut record = id.to_vec();
        record.push(FIELD_SEPARATOR);
        append_field(&mut record, name, FIELD_SEPARATOR);
        append_field(&mut record, path, RECORD_SEPARATOR);
        record
    }

    fn append_field(record: &mut Vec<u8>, data: &[u8], terminator: u8) {
        record.extend_from_slice(data.len().to_string().as_bytes());
        record.push(FIELD_SEPARATOR);
        record.extend_from_slice(data);
        record.push(terminator);
    }

    #[test]
    fn parse_framed_session_preserves_arbitrary_utf8_data() {
        let name = "π's: \\\x1f# $;";
        let path = "/tmp/a:b\\c\nnext";
        let session = parse_one(
            std::str::from_utf8(&framed_session_record(
                b"$7",
                name.as_bytes(),
                path.as_bytes(),
            ))
            .unwrap(),
        )
        .unwrap();

        assert_eq!(session.name, name);
        assert_eq!(session.dirpath, PathBuf::from(path));
    }

    #[test]
    fn parse_framed_sessions_rejects_malformed_records() {
        let valid = framed_session_record(b"$7", b"name", b"/tmp");
        let mut missing_terminator = valid.clone();
        missing_terminator.pop();
        let mut trailing_bytes = valid.clone();
        trailing_bytes.extend_from_slice(b"trailing");
        let invalid_utf8 = framed_session_record(b"$7", &[0xff], b"/tmp");

        for record in [missing_terminator, trailing_bytes, invalid_utf8] {
            assert!(parse_all(&record).is_err());
        }

        let invalid_length = b"$7\x1fnot-a-number\x1fname\x1f4\x1f/tmp\n";
        assert!(parse_all(invalid_length).is_err());
    }
}
