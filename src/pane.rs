//! This module provides a few types and functions to handle Tmux Panes.
//!
//! The main use cases are running Tmux commands & parsing Tmux panes
//! information.

use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use smol::process::Command;

use crate::{
    Result,
    error::{Error, check_empty_process_output, check_process_success, map_byte_parse_error},
    pane_id::PaneId,
    parse::{ByteCursor, ByteParseError, FIELD_SEPARATOR, RECORD_SEPARATOR, normalize_tmux_output},
    window_id::WindowId,
};

/// Format used by [`available_panes`] for one pane per newline-terminated record.
const PANE_LIST_FORMAT: &str = "#{pane_id}\x1f#{pane_index}\x1f#{?pane_active,true,false}\x1f#{n:pane_title}\x1f#{s|\\\\|\\\\\\\\|:pane_title}\x1f#{n:pane_current_command}\x1f#{s|\\\\|\\\\\\\\|:pane_current_command}\x1f#{n:pane_current_path}\x1f#{s|\\\\|\\\\\\\\|:pane_current_path}";
const PANE_LIST_INTENT: &str = "#{pane_id}\\x1f#{pane_index}\\x1f#{?pane_active,true,false}\\x1f#{n:pane_title}\\x1f#{s|\\\\|\\\\\\\\|:pane_title}\\x1f#{n:pane_current_command}\\x1f#{s|\\\\|\\\\\\\\|:pane_current_command}\\x1f#{n:pane_current_path}\\x1f#{s|\\\\|\\\\\\\\|:pane_current_path}\\n";

/// A Tmux pane.
///
/// ```
/// use std::str::FromStr;
/// use tmux_lib::pane::Pane;
///
/// let line = "%20\x1f0\x1ffalse\x1f4\x1frmbp\x1f4\x1fnvim\x1f35\x1f/Users/graelo/code/rust/tmux-backup\n";
/// let pane = Pane::from_str(line).unwrap();
///
/// assert_eq!(pane.id.as_str(), "%20");
/// assert_eq!(pane.index, 0);
/// assert!(!pane.is_active);
/// assert_eq!(pane.command, "nvim");
/// ```
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

impl FromStr for Pane {
    type Err = Error;

    /// Parse a string containing tmux panes status into a new `Pane`.
    ///
    /// This returns a `Result<Pane, Error>` as this call can obviously
    /// fail if provided an invalid format.
    ///
    /// The preferred format is a byte-framed, newline-terminated record:
    ///
    /// ```text
    /// #{pane_id}\x1f#{pane_index}\x1f#{?pane_active,true,false}\x1f#{n:pane_title}\x1f#{pane_title}\x1f#{n:pane_current_command}\x1f#{pane_current_command}\x1f#{n:pane_current_path}\x1f#{pane_current_path}\n
    /// ```
    ///
    /// `#{n:...}` is a byte length, and `\x1f` is Unit Separator. Data fields
    /// are therefore allowed to contain either delimiter or newline. This
    /// parser accepts only this raw framed format.
    ///
    /// The CLI query doubles literal backslashes in data fields so tmux 3.2
    /// through 3.5 can be normalized before parsing:
    ///
    /// ```text
    /// tmux list-panes -a -F "#{pane_id}\x1f#{pane_index}\x1f#{?pane_active,true,false}\x1f#{n:pane_title}\x1f#{s|\\|\\\\|:pane_title}\x1f#{n:pane_current_command}\x1f#{s|\\|\\\\|:pane_current_command}\x1f#{n:pane_current_path}\x1f#{s|\\|\\\\|:pane_current_path}"
    /// ```
    ///
    /// For definitions, look at `Pane` type and the tmux man page for
    /// definitions.
    fn from_str(input: &str) -> std::result::Result<Self, Self::Err> {
        parse::framed_pane(input.as_bytes())
            .map_err(|e| map_byte_parse_error("Pane", PANE_LIST_INTENT, e))
    }
}

impl Pane {
    /// Return the entire Pane content as a `Vec<u8>`.
    ///
    /// # Note
    ///
    /// The output contains the escape codes, joined lines with trailing spaces. This output is
    /// processed by the function `tmux_lib::utils::cleanup_captured_buffer`.
    ///
    pub async fn capture(&self) -> Result<Vec<u8>> {
        let args = vec![
            "capture-pane",
            "-t",
            self.id.as_str(),
            "-J", // preserves trailing spaces & joins any wrapped lines
            "-e", // include escape sequences for text & background
            "-p", // output goes to stdout
            "-S", // starting line number
            "-",  // start of history
            "-E", // ending line number
            "-",  // end of history
        ];

        let output = Command::new("tmux").args(&args).output().await?;

        Ok(output.stdout)
    }
}

mod parse {
    use super::*;

    pub(super) fn framed_pane(input: &[u8]) -> std::result::Result<Pane, ByteParseError> {
        let mut cursor = ByteCursor::new(input);
        let pane = framed_pane_record(&mut cursor)?;
        if !cursor.is_at_end() {
            return Err(ByteParseError::new(
                "unexpected trailing bytes after pane record",
            ));
        }
        Ok(pane)
    }

    pub(super) fn framed_panes(input: &[u8]) -> crate::Result<Vec<Pane>> {
        let mut cursor = ByteCursor::new(input);
        let mut panes = Vec::new();
        while !cursor.is_at_end() {
            panes.push(
                framed_pane_record(&mut cursor)
                    .map_err(|e| map_byte_parse_error("Pane", PANE_LIST_INTENT, e))?,
            );
        }
        Ok(panes)
    }

    fn framed_pane_record(
        cursor: &mut ByteCursor<'_>,
    ) -> std::result::Result<Pane, ByteParseError> {
        let id = cursor
            .take_token_str("pane ID")?
            .parse()
            .map_err(|_| ByteParseError::new("invalid pane ID"))?;
        let index = cursor
            .take_token_str("pane index")?
            .parse()
            .map_err(|_| ByteParseError::new("invalid pane index"))?;
        let is_active = match cursor.take_token_str("pane active flag")? {
            "true" => true,
            "false" => false,
            _ => return Err(ByteParseError::new("invalid pane active flag")),
        };
        let title = cursor.take_length_prefixed_string(FIELD_SEPARATOR, "pane title")?;
        let command = cursor.take_length_prefixed_string(FIELD_SEPARATOR, "pane command")?;
        if command.is_empty() {
            return Err(ByteParseError::new("pane command is empty"));
        }
        let dirpath = cursor.take_length_prefixed_string(RECORD_SEPARATOR, "pane path")?;

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

// ------------------------------
// Ops
// ------------------------------

/// Return a list of all `Pane` from all sessions.
pub async fn available_panes() -> Result<Vec<Pane>> {
    let args = vec!["list-panes", "-a", "-F", PANE_LIST_FORMAT];

    let output = Command::new("tmux").args(&args).output().await?;
    check_process_success(&output, "list-panes")?;
    let stdout = normalize_tmux_output(&output.stdout)
        .map_err(|e| map_byte_parse_error("Pane", PANE_LIST_INTENT, e))?;
    parse::framed_panes(&stdout)
}

/// Create a new pane (horizontal split) in the window with `window_id`, and return the new
/// pane id.
pub async fn new_pane(
    reference_pane: &Pane,
    pane_command: Option<&str>,
    window_id: &WindowId,
) -> Result<PaneId> {
    let mut args = vec![
        "split-window",
        "-h",
        "-c",
        reference_pane.dirpath.to_str().unwrap(),
        "-t",
        window_id.as_str(),
        "-P",
        "-F",
        "#{pane_id}",
    ];
    if let Some(pane_command) = pane_command {
        args.push(pane_command);
    }

    let output = Command::new("tmux").args(&args).output().await?;

    // Check exit status before parsing to avoid confusing parse errors
    // when tmux fails and returns empty/garbage stdout.
    check_process_success(&output, "split-window")?;

    let buffer = String::from_utf8(output.stdout)?;

    let new_id = PaneId::from_str(buffer.trim_end())?;
    Ok(new_id)
}

/// Select (make active) the pane with `pane_id`.
pub async fn select_pane(pane_id: &PaneId) -> Result<()> {
    let args = vec!["select-pane", "-t", pane_id.as_str()];

    let output = Command::new("tmux").args(&args).output().await?;
    check_empty_process_output(&output, "select-pane")
}

#[cfg(test)]
mod tests {
    use super::Pane;
    use super::PaneId;
    use super::parse;
    use super::{FIELD_SEPARATOR, RECORD_SEPARATOR};
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
        let panes: Result<Vec<Pane>> = output.iter().map(|line| Pane::from_str(line)).collect();
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
        let pane = Pane::from_str(&line).expect("Could not parse pane with empty title");

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
        let pane = Pane::from_str(&line).expect("Should parse pane with large index");

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
        let pane = Pane::from_str(&line).expect("Should parse pane with spaces in path");

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
        let pane = Pane::from_str(&line).expect("Should parse pane with unicode title");

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
        let pane = Pane::from_str(&line).expect("Should parse pane with complex command");

        assert_eq!(pane.command, "python -m http.server 8080");
    }

    #[test]
    fn parse_pane_fails_on_missing_id() {
        let line = String::from_utf8(framed_pane_record(
            b"bad", b"0", b"false", b"title", b"cmd", b"/path",
        ))
        .unwrap();
        let result = Pane::from_str(&line);

        assert!(result.is_err());
    }

    #[test]
    fn parse_pane_fails_on_invalid_boolean() {
        let line = String::from_utf8(framed_pane_record(
            b"%1", b"0", b"yes", b"title", b"cmd", b"/path",
        ))
        .unwrap();
        let result = Pane::from_str(&line);

        assert!(result.is_err());
    }

    #[test]
    fn parse_pane_fails_on_empty_command() {
        let line = String::from_utf8(framed_pane_record(
            b"%1", b"0", b"true", b"title", b"", b"/path",
        ))
        .unwrap();
        let result = Pane::from_str(&line);

        assert!(result.is_err());
    }

    #[test]
    fn parse_pane_fails_on_missing_path() {
        let mut line = framed_pane_record(b"%1", b"0", b"true", b"title", b"cmd", b"/path");
        line.pop();
        let line = String::from_utf8(line).unwrap();
        let result = Pane::from_str(&line);

        assert!(result.is_err());
    }

    #[test]
    fn parse_pane_fails_on_wrong_id_prefix() {
        // % is for pane, @ is for window, $ is for session.
        let line = String::from_utf8(framed_pane_record(
            b"@1", b"0", b"true", b"title", b"cmd", b"/path",
        ))
        .unwrap();
        let result = Pane::from_str(&line);

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
        assert!(Pane::from_str("%1:0:false:'title':'cmd':/tmp").is_err());
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

        let pane = parse::framed_panes(&record).unwrap().remove(0);

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

        let pane = Pane::from_str(&input).unwrap();

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
            assert!(parse::framed_panes(&record).is_err());
        }

        let invalid_utf8 = framed_pane_record(b"%1", b"0", b"false", &[0xff], b"zsh", b"/tmp");
        assert!(parse::framed_panes(&invalid_utf8).is_err());

        let undersized = b"%1\x1f0\x1ffalse\x1f4\x1ftitle\x1f3\x1fzsh\x1f4\x1f/tmp\n";
        assert!(parse::framed_panes(undersized).is_err());

        let oversized = b"%1\x1f0\x1ffalse\x1f999\x1ftitle\x1f3\x1fzsh\x1f4\x1f/tmp\n";
        assert!(parse::framed_panes(oversized).is_err());

        let overflowing = b"%1\x1f0\x1ffalse\x1f184467440737095516160\x1ftitle\n";
        assert!(parse::framed_panes(overflowing).is_err());
    }
}
