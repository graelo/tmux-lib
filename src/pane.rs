//! This module provides a few types and functions to handle Tmux Panes.
//!
//! The main use cases are running Tmux commands & parsing Tmux panes
//! information.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{
    pane_id::PaneId,
    wire::{ByteParseError, RecordReader},
};

/// A Tmux pane.
///
/// Values are decoded from a framed `list-panes` record; see
/// [`crate::Tmux::available_panes`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pane {
    /// Pane identifier, e.g. `%37`.
    pub id: PaneId,
    /// Describes the Pane index in the Window
    pub index: u16,
    /// Describes if the pane is currently active (focused).
    pub is_active: bool,
    /// Title of the Pane (usually defaults to the hostname)
    pub title: String,
    /// Current dirpath of the Pane
    pub dirpath: PathBuf,
    /// Current command executed in the Pane
    pub command: String,
}

impl Pane {
    /// Build a `Pane` from one framed record, reading the fields declared in
    /// [`PANE_FIELDS`].
    pub(crate) fn decode(
        reader: &mut RecordReader<'_, '_>,
    ) -> std::result::Result<Pane, ByteParseError> {
        let id = reader
            .token("pane ID")?
            .parse()
            .map_err(|_| ByteParseError::new("invalid pane ID"))?;
        let index = reader
            .token("pane index")?
            .parse()
            .map_err(|_| ByteParseError::new("invalid pane index"))?;
        let is_active = reader.flag("pane active flag")?;
        let title = reader.data("pane title")?;
        let command = reader.required_data("pane command")?;
        let dirpath = reader.data("pane path")?;

        Ok(Pane {
            id,
            index,
            is_active,
            title,
            dirpath: dirpath.into(),
            command,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Pane;
    use super::PaneId;
    use crate::wire::formats::{PANE_FIELDS, PANE_INTENT};
    use crate::wire::framing::{FIELD_SEPARATOR, RECORD_SEPARATOR};
    use crate::wire::{decode_all, decode_one};

    fn parse_all(input: &[u8]) -> crate::Result<Vec<Pane>> {
        decode_all(input, PANE_FIELDS, Pane::decode)
            .map_err(|e| crate::error::map_byte_parse_error("Pane", PANE_INTENT.as_str(), e))
    }

    fn parse_one(input: &str) -> crate::Result<Pane> {
        decode_one(input.as_bytes(), PANE_FIELDS, Pane::decode)
            .map_err(|e| crate::error::map_byte_parse_error("Pane", PANE_INTENT.as_str(), e))
    }
    use crate::Result;
    use std::path::PathBuf;
    use std::str::FromStr;

    #[test]
    fn parse_list_panes() {
        let output = [
            String::from_utf8(framed_pane_record(
                b"%20",
                b"0",
                b"false",
                b"rmbp",
                b"nvim",
                b"/Users/graelo/code/rust/tmux-backup",
            ))
            .unwrap(),
            String::from_utf8(framed_pane_record(
                b"%21",
                b"1",
                b"true",
                b"graelo@server: ~",
                b"tmux",
                b"/Users/graelo/code/rust/tmux-backup",
            ))
            .unwrap(),
            String::from_utf8(framed_pane_record(
                b"%27",
                b"2",
                b"false",
                b"rmbp",
                b"man man",
                b"/Users/graelo/code/rust/tmux-backup",
            ))
            .unwrap(),
        ];
        let panes: Result<Vec<Pane>> = output.iter().map(|line| parse_one(line)).collect();
        let panes = panes.expect("Could not parse tmux panes");

        let expected = vec![
            Pane {
                id: PaneId::from_str("%20").unwrap(),
                index: 0,
                is_active: false,
                title: String::from("rmbp"),
                dirpath: PathBuf::from_str("/Users/graelo/code/rust/tmux-backup").unwrap(),
                command: String::from("nvim"),
            },
            Pane {
                id: PaneId(String::from("%21")),
                index: 1,
                is_active: true,
                title: String::from("graelo@server: ~"),
                dirpath: PathBuf::from_str("/Users/graelo/code/rust/tmux-backup").unwrap(),
                command: String::from("tmux"),
            },
            Pane {
                id: PaneId(String::from("%27")),
                index: 2,
                is_active: false,
                title: String::from("rmbp"),
                dirpath: PathBuf::from_str("/Users/graelo/code/rust/tmux-backup").unwrap(),
                command: String::from("man man"),
            },
        ];

        assert_eq!(panes, expected);
    }

    #[test]
    fn parse_pane_with_empty_title() {
        let line = String::from_utf8(framed_pane_record(
            b"%20",
            b"0",
            b"false",
            b"",
            b"nvim",
            b"/Users/graelo/code/rust/tmux-backup",
        ))
        .unwrap();
        let pane = parse_one(&line).expect("Could not parse pane with empty title");

        let expected = Pane {
            id: PaneId::from_str("%20").unwrap(),
            index: 0,
            is_active: false,
            title: String::from(""),
            dirpath: PathBuf::from_str("/Users/graelo/code/rust/tmux-backup").unwrap(),
            command: String::from("nvim"),
        };

        assert_eq!(pane, expected);
    }

    #[test]
    fn parse_pane_with_large_index() {
        let line = String::from_utf8(framed_pane_record(
            b"%999",
            b"99",
            b"true",
            b"host",
            b"zsh",
            b"/home/user",
        ))
        .unwrap();
        let pane = parse_one(&line).expect("Should parse pane with large index");

        assert_eq!(pane.id, PaneId::from_str("%999").unwrap());
        assert_eq!(pane.index, 99);
        assert!(pane.is_active);
    }

    #[test]
    fn parse_pane_with_spaces_in_path() {
        let line = String::from_utf8(framed_pane_record(
            b"%1",
            b"0",
            b"false",
            b"title",
            b"vim",
            b"/Users/user/My Documents/project",
        ))
        .unwrap();
        let pane = parse_one(&line).expect("Should parse pane with spaces in path");

        assert_eq!(
            pane.dirpath,
            PathBuf::from("/Users/user/My Documents/project")
        );
    }

    #[test]
    fn parse_pane_with_unicode_title() {
        let line = String::from_utf8(framed_pane_record(
            b"%1",
            b"0",
            b"true",
            "日本語タイトル".as_bytes(),
            b"bash",
            b"/home/user",
        ))
        .unwrap();
        let pane = parse_one(&line).expect("Should parse pane with unicode title");

        assert_eq!(pane.title, "日本語タイトル");
    }

    #[test]
    fn parse_pane_with_complex_command() {
        let line = String::from_utf8(framed_pane_record(
            b"%1",
            b"0",
            b"false",
            b"host",
            b"python -m http.server 8080",
            b"/tmp",
        ))
        .unwrap();
        let pane = parse_one(&line).expect("Should parse pane with complex command");

        assert_eq!(pane.command, "python -m http.server 8080");
    }

    #[test]
    fn parse_pane_fails_on_missing_id() {
        let line = String::from_utf8(framed_pane_record(
            b"bad", b"0", b"false", b"title", b"cmd", b"/path",
        ))
        .unwrap();
        let result = parse_one(&line);

        assert!(result.is_err());
    }

    #[test]
    fn parse_pane_fails_on_invalid_boolean() {
        let line = String::from_utf8(framed_pane_record(
            b"%1", b"0", b"yes", b"title", b"cmd", b"/path",
        ))
        .unwrap();
        let result = parse_one(&line);

        assert!(result.is_err());
    }

    #[test]
    fn parse_pane_fails_on_empty_command() {
        let line = String::from_utf8(framed_pane_record(
            b"%1", b"0", b"true", b"title", b"", b"/path",
        ))
        .unwrap();
        let result = parse_one(&line);

        assert!(result.is_err());
    }

    #[test]
    fn parse_pane_fails_on_missing_path() {
        let mut line = framed_pane_record(b"%1", b"0", b"true", b"title", b"cmd", b"/path");
        line.pop();
        let line = String::from_utf8(line).unwrap();
        let result = parse_one(&line);

        assert!(result.is_err());
    }

    #[test]
    fn parse_pane_fails_on_wrong_id_prefix() {
        // % is for pane, @ is for window, $ is for session.
        let line = String::from_utf8(framed_pane_record(
            b"@1", b"0", b"true", b"title", b"cmd", b"/path",
        ))
        .unwrap();
        let result = parse_one(&line);

        assert!(result.is_err());
    }

    fn framed_pane_record(
        id: &[u8],
        index: &[u8],
        active: &[u8],
        title: &[u8],
        command: &[u8],
        path: &[u8],
    ) -> Vec<u8> {
        let mut record = Vec::new();
        for token in [id, index, active] {
            record.extend_from_slice(token);
            record.push(FIELD_SEPARATOR);
        }
        append_field(&mut record, title, FIELD_SEPARATOR);
        append_field(&mut record, command, FIELD_SEPARATOR);
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
    fn parse_pane_rejects_legacy_format() {
        assert!(parse_one("%1:0:false:'title':'cmd':/tmp").is_err());
    }

    #[test]
    fn parse_framed_pane_preserves_arbitrary_utf8_data() {
        let title = "π - Chef d'orchestre: \\\x1f# $;\n";
        let command = "python -c 'print(\"$x\");'\\\x1f\n";
        let path = "/tmp/a:b\\c#d$e;\nnext";
        let record = framed_pane_record(
            b"%274",
            b"1",
            b"true",
            title.as_bytes(),
            command.as_bytes(),
            path.as_bytes(),
        );

        let pane = parse_all(&record).unwrap().remove(0);

        assert_eq!(pane.id.as_str(), "%274");
        assert_eq!(pane.index, 1);
        assert!(pane.is_active);
        assert_eq!(pane.title, title);
        assert_eq!(pane.command, command);
        assert_eq!(pane.dirpath, PathBuf::from(path));
    }

    #[test]
    fn parse_framed_pane_from_str_accepts_record_terminator() {
        let record = framed_pane_record(b"%1", b"0", b"false", b"title", b"zsh", b"/tmp");
        let input = String::from_utf8(record).unwrap();

        let pane = parse_one(&input).unwrap();

        assert_eq!(pane.title, "title");
        assert_eq!(pane.command, "zsh");
    }

    #[test]
    fn parse_framed_panes_rejects_malformed_records() {
        let valid = framed_pane_record(b"%1", b"0", b"false", b"title", b"zsh", b"/tmp");
        let mut missing_terminator = valid.clone();
        missing_terminator.pop();
        let mut trailing_bytes = valid.clone();
        trailing_bytes.extend_from_slice(b"trailing");

        let malformed = [
            framed_pane_record(b"bad", b"0", b"false", b"title", b"zsh", b"/tmp"),
            framed_pane_record(b"%1", b"no", b"false", b"title", b"zsh", b"/tmp"),
            framed_pane_record(b"%1", b"0", b"maybe", b"title", b"zsh", b"/tmp"),
            missing_terminator,
            trailing_bytes,
        ];

        for record in malformed {
            assert!(parse_all(&record).is_err());
        }

        let invalid_utf8 = framed_pane_record(b"%1", b"0", b"false", &[0xff], b"zsh", b"/tmp");
        assert!(parse_all(&invalid_utf8).is_err());

        let undersized = b"%1\x1f0\x1ffalse\x1f4\x1ftitle\x1f3\x1fzsh\x1f4\x1f/tmp\n";
        assert!(parse_all(undersized).is_err());

        let oversized = b"%1\x1f0\x1ffalse\x1f999\x1ftitle\x1f3\x1fzsh\x1f4\x1f/tmp\n";
        assert!(parse_all(oversized).is_err());

        let overflowing = b"%1\x1f0\x1ffalse\x1f184467440737095516160\x1ftitle\n";
        assert!(parse_all(overflowing).is_err());
    }
}
