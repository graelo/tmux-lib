//! This module provides a few types and functions to handle Tmux sessions.
//!
//! The main use cases are running Tmux commands & parsing Tmux session
//! information.

use std::{path::PathBuf, str::FromStr};

use nom::{
    IResult, Parser,
    character::complete::{char, not_line_ending},
    combinator::all_consuming,
};
use serde::{Deserialize, Serialize};
use smol::process::Command;

use crate::{
    Result,
    error::{Error, check_process_success, map_add_intent, map_byte_parse_error},
    pane::Pane,
    pane_id::{PaneId, parse::pane_id},
    parse::{
        ByteCursor, ByteParseError, FIELD_SEPARATOR, RECORD_SEPARATOR, looks_like_framed,
        quoted_nonempty_string,
    },
    session_id::{SessionId, parse::session_id},
    window::Window,
    window_id::{WindowId, parse::window_id},
};

/// Format used by [`available_sessions`] for one session per newline-terminated record.
const SESSION_LIST_FORMAT: &str =
    "#{session_id}\x1f#{n:session_name}\x1f#{session_name}\x1f#{n:session_path}\x1f#{session_path}";
const SESSION_LIST_INTENT: &str = "#{session_id}\\x1f#{n:session_name}\\x1f#{session_name}\\x1f#{n:session_path}\\x1f#{session_path}\\n";

/// A Tmux session.
///
/// ```
/// use std::str::FromStr;
/// use tmux_lib::session::Session;
///
/// let line = "$1:'pytorch':/Users/graelo/ml/pytorch";
/// let session = Session::from_str(line).unwrap();
///
/// assert_eq!(session.id.as_str(), "$1");
/// assert_eq!(session.name, "pytorch");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    /// Session identifier, e.g. `$3`.
    pub id: SessionId,
    /// Name of the session.
    pub name: String,
    /// Working directory of the session.
    pub dirpath: PathBuf,
}

impl FromStr for Session {
    type Err = Error;

    /// Parse a string containing tmux session status into a new `Session`.
    ///
    /// This returns a `Result<Session, Error>` as this call can obviously
    /// fail if provided an invalid format.
    ///
    /// The expected format of the tmux status is
    ///
    /// ```text
    /// $1:'pytorch':/Users/graelo/dl/pytorch
    /// $2:'rust':/Users/graelo/rust
    /// $3:'server: $~':/Users/graelo/swift
    /// $4:'tmux-hacking':/Users/graelo/tmux
    /// ```
    ///
    /// The preferred status format is a byte-framed, newline-terminated
    /// record:
    ///
    /// ```text
    /// #{session_id}\x1f#{n:session_name}\x1f#{session_name}\x1f#{n:session_path}\x1f#{session_path}\n
    /// ```
    ///
    /// The legacy quote-delimited format is also accepted for compatibility:
    ///
    /// ```text
    /// $1:'pytorch':/Users/graelo/ml/pytorch
    /// $2:'rust':/Users/graelo/rust
    /// $3:'server: $~':/Users/graelo/swift
    /// $4:'tmux-hacking':/Users/graelo/tmux
    /// ```
    ///
    /// The framed status is obtained with
    ///
    /// ```text
    /// tmux list-sessions -F "#{session_id}\x1f#{n:session_name}\x1f#{session_name}\x1f#{n:session_path}\x1f#{session_path}"
    /// ```
    ///
    /// For definitions, look at `Session` type and the tmux man page for
    /// definitions.
    fn from_str(input: &str) -> std::result::Result<Self, Self::Err> {
        let desc = "Session";
        if looks_like_framed(input.as_bytes()) {
            return parse::framed_session(input.as_bytes())
                .map_err(|e| map_byte_parse_error(desc, SESSION_LIST_INTENT, e));
        }

        let intent = "##{session_id}:'##{session_name}':##{session_path}";
        let (_, sess) = all_consuming(parse::legacy_session)
            .parse(input)
            .map_err(|e| map_add_intent(desc, intent, e))?;
        Ok(sess)
    }
}

pub(crate) mod parse {
    use super::*;

    pub(super) fn legacy_session(input: &str) -> IResult<&str, Session> {
        let (input, (id, _, name, _, dirpath)) = (
            session_id,
            char(':'),
            quoted_nonempty_string,
            char(':'),
            not_line_ending,
        )
            .parse(input)?;

        Ok((
            input,
            Session {
                id,
                name: name.to_string(),
                dirpath: dirpath.into(),
            },
        ))
    }

    pub(super) fn framed_session(input: &[u8]) -> std::result::Result<Session, ByteParseError> {
        let mut cursor = ByteCursor::new(input);
        let session = framed_session_record(&mut cursor)?;
        if !cursor.is_at_end() {
            return Err(ByteParseError::new(
                "unexpected trailing bytes after session record",
            ));
        }
        Ok(session)
    }

    pub(super) fn framed_sessions(input: &[u8]) -> Result<Vec<Session>> {
        let mut cursor = ByteCursor::new(input);
        let mut sessions = Vec::new();
        while !cursor.is_at_end() {
            sessions.push(
                framed_session_record(&mut cursor)
                    .map_err(|e| map_byte_parse_error("Session", SESSION_LIST_INTENT, e))?,
            );
        }
        Ok(sessions)
    }

    fn framed_session_record(
        cursor: &mut ByteCursor<'_>,
    ) -> std::result::Result<Session, ByteParseError> {
        let id = cursor
            .take_token_str("session ID")?
            .parse()
            .map_err(|_| ByteParseError::new("invalid session ID"))?;
        let name = cursor.take_length_prefixed_string(FIELD_SEPARATOR, "session name")?;
        if name.is_empty() {
            return Err(ByteParseError::new("session name is empty"));
        }
        let dirpath = cursor.take_length_prefixed_string(RECORD_SEPARATOR, "session path")?;

        Ok(Session {
            id,
            name,
            dirpath: dirpath.into(),
        })
    }
}

// ------------------------------
// Ops
// ------------------------------

/// Return a list of all `Session` from the current tmux session.
pub async fn available_sessions() -> Result<Vec<Session>> {
    let args = vec!["list-sessions", "-F", SESSION_LIST_FORMAT];

    let output = Command::new("tmux").args(&args).output().await?;
    check_process_success(&output, "list-sessions")?;
    parse::framed_sessions(&output.stdout)
}

/// Create a Tmux session (and thus a window & pane).
///
/// The new session attributes:
///
/// - the session name is taken from the passed `session`
/// - the working directory is taken from the pane's working directory.
///
pub async fn new_session(
    session: &Session,
    window: &Window,
    pane: &Pane,
    pane_command: Option<&str>,
) -> Result<(SessionId, WindowId, PaneId)> {
    let mut args = vec![
        "new-session",
        "-d",
        "-c",
        pane.dirpath.to_str().unwrap(),
        "-s",
        &session.name,
        "-n",
        &window.name,
        "-P",
        "-F",
        "#{session_id}:#{window_id}:#{pane_id}",
    ];
    if let Some(pane_command) = pane_command {
        args.push(pane_command);
    }

    let output = Command::new("tmux").args(&args).output().await?;

    // Check exit status before parsing to avoid confusing parse errors
    // when tmux fails and returns empty/garbage stdout.
    check_process_success(&output, "new-session")?;

    let buffer = String::from_utf8(output.stdout)?;
    let buffer = buffer.trim_end();

    let desc = "new-session";
    let intent = "##{session_id}:##{window_id}:##{pane_id}";
    let (_, (new_session_id, _, new_window_id, _, new_pane_id)) =
        all_consuming((session_id, char(':'), window_id, char(':'), pane_id))
            .parse(buffer)
            .map_err(|e| map_add_intent(desc, intent, e))?;

    Ok((new_session_id, new_window_id, new_pane_id))
}

#[cfg(test)]
mod tests {
    use super::Session;
    use super::SessionId;
    use super::parse;
    use super::{FIELD_SEPARATOR, RECORD_SEPARATOR};
    use crate::Result;
    use std::path::PathBuf;
    use std::str::FromStr;

    #[test]
    fn parse_list_sessions() {
        let output = [
            "$1:'pytorch':/Users/graelo/ml/pytorch",
            "$2:'rust':/Users/graelo/rust",
            "$3:'server: $':/Users/graelo/swift",
            "$4:'tmux-hacking':/Users/graelo/tmux",
        ];
        let sessions: Result<Vec<Session>> =
            output.iter().map(|&line| Session::from_str(line)).collect();
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
        let input = "$999:'large-id-session':/home/user/projects";
        let session = Session::from_str(input).expect("Should parse session with large id");

        assert_eq!(session.id, SessionId::from_str("$999").unwrap());
        assert_eq!(session.name, "large-id-session");
        assert_eq!(session.dirpath, PathBuf::from("/home/user/projects"));
    }

    #[test]
    fn parse_session_with_spaces_in_path() {
        let input = "$5:'dev':/Users/user/My Projects/rust";
        let session = Session::from_str(input).expect("Should parse session with spaces in path");

        assert_eq!(session.name, "dev");
        assert_eq!(
            session.dirpath,
            PathBuf::from("/Users/user/My Projects/rust")
        );
    }

    #[test]
    fn parse_session_with_unicode_in_name() {
        let input = "$6:'项目-日本語':/home/user/code";
        let session = Session::from_str(input).expect("Should parse session with unicode name");

        assert_eq!(session.name, "项目-日本語");
    }

    #[test]
    fn parse_session_fails_on_missing_id() {
        let input = "'session-name':/path/to/dir";
        let result = Session::from_str(input);

        assert!(result.is_err());
    }

    #[test]
    fn parse_session_fails_on_missing_name_quotes() {
        let input = "$1:session-name:/path/to/dir";
        let result = Session::from_str(input);

        assert!(result.is_err());
    }

    #[test]
    fn parse_session_fails_on_empty_name() {
        let input = "$1:'':/path/to/dir";
        let result = Session::from_str(input);

        assert!(result.is_err());
    }

    #[test]
    fn parse_session_fails_on_malformed_id() {
        let input = "@1:'session':/path"; // @ is window prefix, not session
        let result = Session::from_str(input);

        assert!(result.is_err());
    }

    #[test]
    fn parse_session_with_colon_in_path() {
        // Paths can contain colons (e.g., Windows-style paths or special paths)
        let input = "$7:'test':/path/with:colon/here";
        let session = Session::from_str(input).expect("Should parse session with colon in path");

        assert_eq!(session.dirpath, PathBuf::from("/path/with:colon/here"));
    }

    fn framed_session_record(name: &[u8], path: &[u8]) -> Vec<u8> {
        let mut record = b"$7\x1f".to_vec();
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
    fn parse_legacy_session_with_unit_separator() {
        let session = Session::from_str("$7:'name\x1f':/tmp").unwrap();

        assert_eq!(session.name, "name\x1f");
    }

    #[test]
    fn parse_framed_session_preserves_arbitrary_utf8_data() {
        let name = "π's: \\\x1f# $;";
        let path = "/tmp/a:b\\c\nnext";
        let session = Session::from_str(
            std::str::from_utf8(&framed_session_record(name.as_bytes(), path.as_bytes())).unwrap(),
        )
        .unwrap();

        assert_eq!(session.name, name);
        assert_eq!(session.dirpath, PathBuf::from(path));
    }

    #[test]
    fn parse_framed_sessions_rejects_malformed_records() {
        let valid = framed_session_record(b"name", b"/tmp");
        let mut missing_terminator = valid.clone();
        missing_terminator.pop();
        let mut trailing_bytes = valid.clone();
        trailing_bytes.extend_from_slice(b"trailing");
        let invalid_utf8 = framed_session_record(&[0xff], b"/tmp");

        for record in [missing_terminator, trailing_bytes, invalid_utf8] {
            assert!(parse::framed_sessions(&record).is_err());
        }

        let invalid_length = b"$7\x1fnot-a-number\x1fname\x1f4\x1f/tmp\n";
        assert!(parse::framed_sessions(invalid_length).is_err());
    }
}
