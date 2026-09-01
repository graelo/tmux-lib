//! This module provides a few types and functions to handle Tmux windows.
//!
//! The main use cases are running Tmux commands & parsing Tmux window information.

use std::str::FromStr;

use smol::process::Command;

use nom::{Parser, character::complete::char, combinator::all_consuming};
use serde::{Deserialize, Serialize};

use crate::{
    Result,
    error::{
        Error, check_empty_process_output, check_process_success, map_add_intent,
        map_byte_parse_error,
    },
    layout::{self, window_layout},
    pane::Pane,
    pane_id::{PaneId, parse::pane_id},
    parse::{ByteCursor, ByteParseError, FIELD_SEPARATOR, RECORD_SEPARATOR},
    session::Session,
    window_id::{WindowId, parse::window_id},
};

/// Format used by [`available_windows`] for one window per newline-terminated record.
const WINDOW_LIST_FORMAT: &str = "#{window_id}\x1f#{window_index}\x1f#{?window_active,true,false}\x1f#{window_layout}\x1f#{n:window_name}\x1f#{window_name}\x1f#{n:window_linked_sessions_list}\x1f#{window_linked_sessions_list}";
const WINDOW_LIST_INTENT: &str = "#{window_id}\\x1f#{window_index}\\x1f#{?window_active,true,false}\\x1f#{window_layout}\\x1f#{n:window_name}\\x1f#{window_name}\\x1f#{n:window_linked_sessions_list}\\x1f#{window_linked_sessions_list}\\n";

/// A Tmux window.
///
/// ```
/// use std::str::FromStr;
/// use tmux_lib::window::Window;
///
/// let line = "@5\x1f0\x1ftrue\x1f64f0,334x85,0,0,11\x1f3\x1fben\x1f4\x1frust\n";
/// let window = Window::from_str(line).unwrap();
///
/// assert_eq!(window.id.as_str(), "@5");
/// assert_eq!(window.index, 0);
/// assert!(window.is_active);
/// assert_eq!(window.name, "ben");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    /// Window identifier, e.g. `@3`.
    pub id: WindowId,
    /// Index of the Window in the Session.
    pub index: u16,
    /// Describes whether the Window is active.
    pub is_active: bool,
    /// Describes how panes are laid out in the Window.
    pub layout: String,
    /// Name of the Window.
    pub name: String,
    /// Name of Sessions to which this Window is attached.
    pub sessions: Vec<String>,
}

impl FromStr for Window {
    type Err = Error;

    /// Parse a string containing the tmux window status into a new `Window`.
    ///
    /// This returns a `Result<Window, Error>` as this call can obviously
    /// fail if provided an invalid format.
    ///
    /// The tmux status is a byte-framed, newline-terminated record:
    ///
    /// ```text
    /// #{window_id}\x1f#{window_index}\x1f#{?window_active,true,false}\x1f#{window_layout}\x1f#{n:window_name}\x1f#{window_name}\x1f#{n:window_linked_sessions_list}\x1f#{window_linked_sessions_list}\n
    /// ```
    ///
    /// `#{n:...}` is a byte length, and `\x1f` is Unit Separator. This parser
    /// accepts only this framed format. For example, tmux may emit these
    /// records:
    ///
    /// ```text
    /// @1\x1f0\x1ftrue\x1f035d,334x85,0,0{167x85,0,0,1,166x85,168,0[166x48,168,0,2,166x36,168,49,3]}\x1f6\x1fignite\x1f7\x1fpytorch\n
    /// @2\x1f1\x1ffalse\x1f4438,334x85,0,0[334x41,0,0{167x41,0,0,4,166x41,168,0,5},334x43,0,42{167x43,0,42,6,166x43,168,42,7}]\x1f10\x1fdates-attn\x1f7\x1fpytorch\n
    /// @3\x1f2\x1ffalse\x1f9e8b,334x85,0,0{167x85,0,0,8,166x85,168,0,9}\x1f7\x1fth-bits\x1f7\x1fpytorch\n
    /// @4\x1f3\x1ffalse\x1f64ef,334x85,0,0,10\x1f14\x1fdocker-pytorch\x1f7\x1fpytorch\n
    /// @5\x1f0\x1ftrue\x1f64f0,334x85,0,0,11\x1f3\x1fben\x1f4\x1frust\n
    /// @6\x1f1\x1ffalse\x1f64f1,334x85,0,0,12\x1f4\x1fpyo3\x1f4\x1frust\n
    /// @7\x1f2\x1ffalse\x1f64f2,334x85,0,0,13\x1f13\x1fmdns-repeater\x1f4\x1frust\n
    /// @8\x1f0\x1ftrue\x1f64f3,334x85,0,0,14\x1f7\x1fcombine\x1f5\x1fswift\n
    /// @9\x1f0\x1ffalse\x1f64f4,334x85,0,0,15\x1f7\x1fcopyrat\x1f12\x1ftmux-hacking\n
    /// @10\x1f1\x1ffalse\x1fae3a,334x85,0,0[334x48,0,0,17,334x36,0,49{175x36,0,49,18,158x36,176,49,19}]\x1f9\x1fmytui-app\x1f12\x1ftmux-hacking\n
    /// @11\x1f2\x1ftrue\x1fe2e2,334x85,0,0{175x85,0,0,20,158x85,176,0[158x42,176,0,21,158x42,176,43,27]}\x1f11\x1ftmux-backup\x1f12\x1ftmux-hacking\n
    /// ```
    /// The framed status is obtained with
    ///
    /// ```text
    /// tmux list-windows -a -F "#{window_id}\x1f#{window_index}\x1f#{?window_active,true,false}\x1f#{window_layout}\x1f#{n:window_name}\x1f#{window_name}\x1f#{n:window_linked_sessions_list}\x1f#{window_linked_sessions_list}"
    /// ```
    ///
    /// For definitions, look at `Window` type and the tmux man page for
    /// definitions.
    fn from_str(input: &str) -> std::result::Result<Self, Self::Err> {
        parse::framed_window(input.as_bytes())
            .map_err(|e| map_byte_parse_error("Window", WINDOW_LIST_INTENT, e))
    }
}

impl Window {
    /// Return all `PaneId` in this window.
    pub fn pane_ids(&self) -> Vec<PaneId> {
        let layout = layout::parse_window_layout(&self.layout).unwrap();
        layout.pane_ids().iter().map(PaneId::from).collect()
    }
}

mod parse {
    use super::*;

    pub(super) fn framed_window(input: &[u8]) -> std::result::Result<Window, ByteParseError> {
        let mut cursor = ByteCursor::new(input);
        let window = framed_window_record(&mut cursor)?;
        if !cursor.is_at_end() {
            return Err(ByteParseError::new(
                "unexpected trailing bytes after window record",
            ));
        }
        Ok(window)
    }

    pub(super) fn framed_windows(input: &[u8]) -> Result<Vec<Window>> {
        let mut cursor = ByteCursor::new(input);
        let mut windows = Vec::new();
        while !cursor.is_at_end() {
            windows.push(
                framed_window_record(&mut cursor)
                    .map_err(|e| map_byte_parse_error("Window", WINDOW_LIST_INTENT, e))?,
            );
        }
        Ok(windows)
    }

    fn framed_window_record(
        cursor: &mut ByteCursor<'_>,
    ) -> std::result::Result<Window, ByteParseError> {
        let id = cursor
            .take_token_str("window ID")?
            .parse()
            .map_err(|_| ByteParseError::new("invalid window ID"))?;
        let index = cursor
            .take_token_str("window index")?
            .parse()
            .map_err(|_| ByteParseError::new("invalid window index"))?;
        let is_active = match cursor.take_token_str("window active flag")? {
            "true" => true,
            "false" => false,
            _ => return Err(ByteParseError::new("invalid window active flag")),
        };
        let layout_text = cursor.take_token_str("window layout")?;
        all_consuming(window_layout)
            .parse(layout_text)
            .map_err(|_| ByteParseError::new("invalid window layout"))?;
        let name = cursor.take_length_prefixed_string(FIELD_SEPARATOR, "window name")?;
        if name.is_empty() {
            return Err(ByteParseError::new("window name is empty"));
        }
        let session_names =
            cursor.take_length_prefixed_string(RECORD_SEPARATOR, "linked session names")?;
        if session_names.is_empty() {
            return Err(ByteParseError::new("linked session names are empty"));
        }

        Ok(Window {
            id,
            index,
            is_active,
            layout: layout_text.to_owned(),
            name,
            sessions: vec![session_names],
        })
    }
}

// ------------------------------
// Ops
// ------------------------------

/// Return a list of all `Window` from all sessions.
pub async fn available_windows() -> Result<Vec<Window>> {
    let args = vec!["list-windows", "-a", "-F", WINDOW_LIST_FORMAT];

    let output = Command::new("tmux").args(&args).output().await?;
    check_process_success(&output, "list-windows")?;
    parse::framed_windows(&output.stdout)
}

/// Create a Tmux window in a session exactly named as the passed `session`.
///
/// The new window attributes:
///
/// - created in the `session`
/// - the window name is taken from the passed `window`
/// - the working directory is the pane's working directory.
///
pub async fn new_window(
    session: &Session,
    window: &Window,
    pane: &Pane,
    pane_command: Option<&str>,
) -> Result<(WindowId, PaneId)> {
    // Use session ID for targeting - it's unambiguous and immediately valid
    // after session creation, unlike names which may have parsing issues
    // (e.g., names containing colons) or brief lookup race conditions.
    let target_session = session.id.as_str();

    let mut args = vec![
        "new-window",
        "-d",
        "-c",
        pane.dirpath.to_str().unwrap(),
        "-n",
        &window.name,
        "-t",
        target_session,
        "-P",
        "-F",
        "#{window_id}:#{pane_id}",
    ];
    if let Some(pane_command) = pane_command {
        args.push(pane_command);
    }

    let output = Command::new("tmux").args(&args).output().await?;

    // Check exit status before parsing to avoid confusing parse errors
    // when tmux fails and returns empty/garbage stdout.
    check_process_success(&output, "new-window")?;

    let buffer = String::from_utf8(output.stdout)?;
    let buffer = buffer.trim_end();

    let desc = "new-window";
    let intent = "##{window_id}:##{pane_id}";

    let (_, (new_window_id, _, new_pane_id)) = all_consuming((window_id, char(':'), pane_id))
        .parse(buffer)
        .map_err(|e| map_add_intent(desc, intent, e))?;

    Ok((new_window_id, new_pane_id))
}

/// Apply the provided `layout` to the window with `window_id`.
pub async fn set_layout(layout: &str, window_id: &WindowId) -> Result<()> {
    let args = vec!["select-layout", "-t", window_id.as_str(), layout];

    let output = Command::new("tmux").args(&args).output().await?;
    check_empty_process_output(&output, "select-layout")
}

/// Select (make active) the window with `window_id`.
pub async fn select_window(window_id: &WindowId) -> Result<()> {
    let args = vec!["select-window", "-t", window_id.as_str()];

    let output = Command::new("tmux").args(&args).output().await?;
    check_empty_process_output(&output, "select-window")
}

#[cfg(test)]
mod tests {
    use super::Window;
    use super::WindowId;
    use super::parse;
    use super::{FIELD_SEPARATOR, RECORD_SEPARATOR};
    use crate::Result;
    use crate::pane_id::PaneId;
    use std::str::FromStr;

    #[test]
    fn parse_list_windows() {
        let output = vec![
            "@1\x1f0\x1ftrue\x1f035d,334x85,0,0{167x85,0,0,1,166x85,168,0[166x48,168,0,2,166x36,168,49,3]}\x1f6\x1fignite\x1f7\x1fpytorch\n",
            "@2\x1f1\x1ffalse\x1f4438,334x85,0,0[334x41,0,0{167x41,0,0,4,166x41,168,0,5},334x43,0,42{167x43,0,42,6,166x43,168,42,7}]\x1f10\x1fdates-attn\x1f7\x1fpytorch\n",
            "@3\x1f2\x1ffalse\x1f9e8b,334x85,0,0{167x85,0,0,8,166x85,168,0,9}\x1f7\x1fth-bits\x1f7\x1fpytorch\n",
            "@4\x1f3\x1ffalse\x1f64ef,334x85,0,0,10\x1f14\x1fdocker-pytorch\x1f7\x1fpytorch\n",
            "@5\x1f0\x1ftrue\x1f64f0,334x85,0,0,11\x1f3\x1fben\x1f4\x1frust\n",
            "@6\x1f1\x1ffalse\x1f64f1,334x85,0,0,12\x1f4\x1fpyo3\x1f4\x1frust\n",
            "@7\x1f2\x1ffalse\x1f64f2,334x85,0,0,13\x1f13\x1fmdns-repeater\x1f4\x1frust\n",
            "@8\x1f0\x1ftrue\x1f64f3,334x85,0,0,14\x1f7\x1fcombine\x1f5\x1fswift\n",
            "@9\x1f0\x1ffalse\x1f64f4,334x85,0,0,15\x1f7\x1fcopyrat\x1f12\x1ftmux-hacking\n",
            "@10\x1f1\x1ffalse\x1fae3a,334x85,0,0[334x48,0,0,17,334x36,0,49{175x36,0,49,18,158x36,176,49,19}]\x1f9\x1fmytui-app\x1f12\x1ftmux-hacking\n",
            "@11\x1f2\x1ftrue\x1fe2e2,334x85,0,0{175x85,0,0,20,158x85,176,0[158x42,176,0,21,158x42,176,43,27]}\x1f11\x1ftmux-backup\x1f12\x1ftmux-hacking\n",
        ];
        let sessions: Result<Vec<Window>> =
            output.iter().map(|line| Window::from_str(line)).collect();
        let windows = sessions.expect("Could not parse tmux sessions");

        let expected = vec![
            Window {
                id: WindowId::from_str("@1").unwrap(),
                index: 0,
                is_active: true,
                layout: String::from(
                    "035d,334x85,0,0{167x85,0,0,1,166x85,168,0[166x48,168,0,2,166x36,168,49,3]}",
                ),
                name: String::from("ignite"),
                sessions: vec![String::from("pytorch")],
            },
            Window {
                id: WindowId::from_str("@2").unwrap(),
                index: 1,
                is_active: false,
                layout: String::from(
                    "4438,334x85,0,0[334x41,0,0{167x41,0,0,4,166x41,168,0,5},334x43,0,42{167x43,0,42,6,166x43,168,42,7}]",
                ),
                name: String::from("dates-attn"),
                sessions: vec![String::from("pytorch")],
            },
            Window {
                id: WindowId::from_str("@3").unwrap(),
                index: 2,
                is_active: false,
                layout: String::from("9e8b,334x85,0,0{167x85,0,0,8,166x85,168,0,9}"),
                name: String::from("th-bits"),
                sessions: vec![String::from("pytorch")],
            },
            Window {
                id: WindowId::from_str("@4").unwrap(),
                index: 3,
                is_active: false,
                layout: String::from("64ef,334x85,0,0,10"),
                name: String::from("docker-pytorch"),
                sessions: vec![String::from("pytorch")],
            },
            Window {
                id: WindowId::from_str("@5").unwrap(),
                index: 0,
                is_active: true,
                layout: String::from("64f0,334x85,0,0,11"),
                name: String::from("ben"),
                sessions: vec![String::from("rust")],
            },
            Window {
                id: WindowId::from_str("@6").unwrap(),
                index: 1,
                is_active: false,
                layout: String::from("64f1,334x85,0,0,12"),
                name: String::from("pyo3"),
                sessions: vec![String::from("rust")],
            },
            Window {
                id: WindowId::from_str("@7").unwrap(),
                index: 2,
                is_active: false,
                layout: String::from("64f2,334x85,0,0,13"),
                name: String::from("mdns-repeater"),
                sessions: vec![String::from("rust")],
            },
            Window {
                id: WindowId::from_str("@8").unwrap(),
                index: 0,
                is_active: true,
                layout: String::from("64f3,334x85,0,0,14"),
                name: String::from("combine"),
                sessions: vec![String::from("swift")],
            },
            Window {
                id: WindowId::from_str("@9").unwrap(),
                index: 0,
                is_active: false,
                layout: String::from("64f4,334x85,0,0,15"),
                name: String::from("copyrat"),
                sessions: vec![String::from("tmux-hacking")],
            },
            Window {
                id: WindowId::from_str("@10").unwrap(),
                index: 1,
                is_active: false,
                layout: String::from(
                    "ae3a,334x85,0,0[334x48,0,0,17,334x36,0,49{175x36,0,49,18,158x36,176,49,19}]",
                ),
                name: String::from("mytui-app"),
                sessions: vec![String::from("tmux-hacking")],
            },
            Window {
                id: WindowId::from_str("@11").unwrap(),
                index: 2,
                is_active: true,
                layout: String::from(
                    "e2e2,334x85,0,0{175x85,0,0,20,158x85,176,0[158x42,176,0,21,158x42,176,43,27]}",
                ),
                name: String::from("tmux-backup"),
                sessions: vec![String::from("tmux-hacking")],
            },
        ];

        assert_eq!(windows, expected);
    }

    #[test]
    fn parse_window_single_pane() {
        let input = "@5\x1f0\x1ftrue\x1f64f0,334x85,0,0,11\x1f3\x1fben\x1f4\x1frust\n";
        let window = Window::from_str(input).expect("Should parse window with single pane");

        assert_eq!(window.id, WindowId::from_str("@5").unwrap());
        assert_eq!(window.index, 0);
        assert!(window.is_active);
        assert_eq!(window.name, "ben");
        assert_eq!(window.sessions, vec!["rust".to_string()]);
    }

    #[test]
    fn parse_window_with_large_index() {
        let input = "@100\x1f99\x1ffalse\x1f64f0,334x85,0,0,11\x1f4\x1ftest\x1f7\x1fsession\n";
        let window = Window::from_str(input).expect("Should parse window with large index");

        assert_eq!(window.id, WindowId::from_str("@100").unwrap());
        assert_eq!(window.index, 99);
        assert!(!window.is_active);
    }

    #[test]
    fn parse_window_fails_on_missing_id() {
        let input = "bad\x1f0\x1ftrue\x1f64f0,334x85,0,0,11\x1f4\x1fname\x1f7\x1fsession\n";
        let result = Window::from_str(input);

        assert!(result.is_err());
    }

    #[test]
    fn parse_window_fails_on_invalid_boolean() {
        let input = "@1\x1f0\x1fyes\x1f64f0,334x85,0,0,11\x1f4\x1fname\x1f7\x1fsession\n";
        let result = Window::from_str(input);

        assert!(result.is_err());
    }

    #[test]
    fn parse_window_fails_on_empty_name() {
        let input = "@1\x1f0\x1ftrue\x1f64f0,334x85,0,0,11\x1f0\x1f\x1f7\x1fsession\n";
        let result = Window::from_str(input);

        assert!(result.is_err());
    }

    #[test]
    fn window_pane_ids_single_pane() {
        let window = Window {
            id: WindowId::from_str("@1").unwrap(),
            index: 0,
            is_active: true,
            layout: String::from("64f0,334x85,0,0,11"),
            name: String::from("test"),
            sessions: vec![String::from("session")],
        };

        let pane_ids = window.pane_ids();
        assert_eq!(pane_ids.len(), 1);
        assert_eq!(pane_ids[0], PaneId::from_str("%11").unwrap());
    }

    #[test]
    fn window_pane_ids_multiple_panes() {
        let window = Window {
            id: WindowId::from_str("@3").unwrap(),
            index: 2,
            is_active: false,
            layout: String::from("9e8b,334x85,0,0{167x85,0,0,8,166x85,168,0,9}"),
            name: String::from("th-bits"),
            sessions: vec![String::from("pytorch")],
        };

        let pane_ids = window.pane_ids();
        assert_eq!(pane_ids.len(), 2);
        assert_eq!(pane_ids[0], PaneId::from_str("%8").unwrap());
        assert_eq!(pane_ids[1], PaneId::from_str("%9").unwrap());
    }

    #[test]
    fn window_pane_ids_complex_layout() {
        // Complex nested layout with 4 panes
        let window = Window {
            id: WindowId::from_str("@1").unwrap(),
            index: 0,
            is_active: true,
            layout: String::from(
                "035d,334x85,0,0{167x85,0,0,1,166x85,168,0[166x48,168,0,2,166x36,168,49,3]}",
            ),
            name: String::from("ignite"),
            sessions: vec![String::from("pytorch")],
        };

        let pane_ids = window.pane_ids();
        assert_eq!(pane_ids.len(), 3);
        assert_eq!(pane_ids[0], PaneId::from_str("%1").unwrap());
        assert_eq!(pane_ids[1], PaneId::from_str("%2").unwrap());
        assert_eq!(pane_ids[2], PaneId::from_str("%3").unwrap());
    }

    fn framed_window_record(name: &[u8], sessions: &[u8]) -> Vec<u8> {
        let mut record = b"@5\x1f0\x1ftrue\x1f64f0,334x85,0,0,11\x1f".to_vec();
        append_field(&mut record, name, FIELD_SEPARATOR);
        append_field(&mut record, sessions, RECORD_SEPARATOR);
        record
    }

    fn append_field(record: &mut Vec<u8>, data: &[u8], terminator: u8) {
        record.extend_from_slice(data.len().to_string().as_bytes());
        record.push(FIELD_SEPARATOR);
        record.extend_from_slice(data);
        record.push(terminator);
    }

    #[test]
    fn parse_window_rejects_legacy_format() {
        assert!(Window::from_str("@5:0:true:64f0,334x85,0,0,11:'name':'session'").is_err());
    }

    #[test]
    fn parse_framed_window_preserves_arbitrary_utf8_data() {
        let name = "π's: \\\x1f# $;\n";
        let sessions = "session:two\\\x1f\n";
        let window = Window::from_str(
            std::str::from_utf8(&framed_window_record(name.as_bytes(), sessions.as_bytes()))
                .unwrap(),
        )
        .unwrap();

        assert_eq!(window.name, name);
        assert_eq!(window.sessions, vec![sessions]);
    }

    #[test]
    fn parse_framed_windows_rejects_malformed_records() {
        let valid = framed_window_record(b"name", b"session");
        let mut missing_terminator = valid.clone();
        missing_terminator.pop();
        let mut trailing_bytes = valid.clone();
        trailing_bytes.extend_from_slice(b"trailing");
        let invalid_utf8 = framed_window_record(&[0xff], b"session");

        for record in [missing_terminator, trailing_bytes, invalid_utf8] {
            assert!(parse::framed_windows(&record).is_err());
        }

        let invalid_length =
            b"@5\x1f0\x1ftrue\x1f64f0,334x85,0,0,11\x1fnot-a-number\x1fname\x1f7\x1fsession\n";
        assert!(parse::framed_windows(invalid_length).is_err());
    }
}
