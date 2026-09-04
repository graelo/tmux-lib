//! This module provides a few types and functions to handle Tmux windows.
//!
//! The main use cases are running Tmux commands & parsing Tmux window information.

use std::str::FromStr;

use nom::{Parser, combinator::all_consuming};
use serde::{Deserialize, Serialize};

use crate::{
    error::{Error, map_byte_parse_error},
    layout::{self, window_layout},
    pane_id::PaneId,
    window_id::WindowId,
    wire::{
        ByteParseError, RecordReader, decode_one,
        formats::{WINDOW_FIELDS, WINDOW_INTENT},
    },
};

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
    /// The CLI query doubles literal backslashes in data fields so tmux 3.4
    /// through 3.5 can be normalized before parsing:
    ///
    /// ```text
    /// tmux list-windows -a -F "#{window_id}\x1f#{window_index}\x1f#{?window_active,true,false}\x1f#{window_layout}\x1f#{n:window_name}\x1f#{s|\\|\\\\|:window_name}\x1f#{n:window_linked_sessions_list}\x1f#{s|\\|\\\\|:window_linked_sessions_list}"
    /// ```
    ///
    /// For definitions, look at `Window` type and the tmux man page for
    /// definitions.
    fn from_str(input: &str) -> std::result::Result<Self, Self::Err> {
        decode_one(input.as_bytes(), WINDOW_FIELDS, Window::decode)
            .map_err(|e| map_byte_parse_error("Window", WINDOW_INTENT.as_str(), e))
    }
}

impl Window {
    /// Return all `PaneId` in this window.
    pub fn pane_ids(&self) -> Vec<PaneId> {
        let layout = layout::parse_window_layout(&self.layout).unwrap();
        layout.pane_ids().iter().map(PaneId::from).collect()
    }
}

impl Window {
    /// Build a `Window` from one framed record, reading the fields declared in
    /// [`WINDOW_FIELDS`].
    pub(crate) fn decode(
        reader: &mut RecordReader<'_, '_>,
    ) -> std::result::Result<Window, ByteParseError> {
        let id = reader
            .token("window ID")?
            .parse()
            .map_err(|_| ByteParseError::new("invalid window ID"))?;
        let index = reader
            .token("window index")?
            .parse()
            .map_err(|_| ByteParseError::new("invalid window index"))?;
        let is_active = reader.flag("window active flag")?;
        let layout_text = reader.token("window layout")?;
        all_consuming(window_layout)
            .parse(layout_text)
            .map_err(|_| ByteParseError::new("invalid window layout"))?;
        let name = reader.required_data("window name")?;
        let session_names = reader.required_data("linked session names")?;

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

#[cfg(test)]
mod tests {
    use super::Window;
    use super::WindowId;
    use crate::wire::decode_all;
    use crate::wire::formats::{WINDOW_FIELDS, WINDOW_INTENT};
    use crate::wire::framing::{FIELD_SEPARATOR, RECORD_SEPARATOR};

    fn decode_all_test(input: &[u8]) -> crate::Result<Vec<Window>> {
        decode_all(input, WINDOW_FIELDS, Window::decode)
            .map_err(|e| crate::error::map_byte_parse_error("Window", WINDOW_INTENT.as_str(), e))
    }
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
            assert!(decode_all_test(&record).is_err());
        }

        let invalid_length =
            b"@5\x1f0\x1ftrue\x1f64f0,334x85,0,0,11\x1fnot-a-number\x1fname\x1f7\x1fsession\n";
        assert!(decode_all_test(invalid_length).is_err());
    }
}
