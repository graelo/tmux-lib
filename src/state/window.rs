//! Tmux window state and identifiers.

use std::str::FromStr;

use nom::{
    IResult, Parser,
    character::complete::{char, digit1},
    combinator::all_consuming,
    sequence::preceded,
};
use serde::{Deserialize, Serialize};

use crate::{
    error::{Error, map_add_intent},
    layout::{self, window_layout},
    wire::{ByteParseError, RecordReader},
};

use super::pane::PaneId;

/// A Tmux window.
///
/// Values are decoded from a framed `list-windows` record; see
/// [`crate::Tmux::available_windows`].
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

impl Window {
    /// Return all `PaneId` in this window.
    pub fn pane_ids(&self) -> Vec<PaneId> {
        let layout = layout::parse_window_layout(&self.layout).unwrap();
        layout.pane_ids().iter().map(PaneId::from).collect()
    }
}

impl Window {
    /// Build a `Window` from one framed record, reading the fields declared in
    /// [`WINDOW_FIELDS`](crate::wire::formats::WINDOW_FIELDS).
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

/// The id of a Tmux window.
///
/// This wraps the raw tmux representation (`@41`).
///
/// ```
/// use std::str::FromStr;
/// use tmux_lib::window_id::WindowId;
///
/// let id = WindowId::from_str("@10").unwrap();
/// assert_eq!(id.as_str(), "@10");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowId(String);

impl FromStr for WindowId {
    type Err = Error;

    /// Parse into WindowId. The `&str` must start with '@' followed by a
    /// `u16`.
    fn from_str(input: &str) -> std::result::Result<Self, Self::Err> {
        let desc = "WindowId";
        let intent = "##{window_id}";

        let (_, window_id) = all_consuming(parse_window_id)
            .parse(input)
            .map_err(|e| map_add_intent(desc, intent, e))?;

        Ok(window_id)
    }
}

impl WindowId {
    /// Extract a string slice containing the raw representation.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub(crate) fn parse_window_id(input: &str) -> IResult<&str, WindowId> {
    let (input, digit) = preceded(char('@'), digit1).parse(input)?;
    let id = format!("@{digit}");
    Ok((input, WindowId(id)))
}

#[cfg(test)]
mod tests {
    use super::Window;
    use super::WindowId;
    use crate::wire::formats::{WINDOW_FIELDS, WINDOW_INTENT};
    use crate::wire::framing::{FIELD_SEPARATOR, RECORD_SEPARATOR};
    use crate::wire::{decode_all, decode_one};

    fn parse_all(input: &[u8]) -> crate::Result<Vec<Window>> {
        decode_all(input, WINDOW_FIELDS, Window::decode)
            .map_err(|e| crate::error::map_byte_parse_error("Window", WINDOW_INTENT.as_str(), e))
    }

    fn parse_one(input: &str) -> crate::Result<Window> {
        decode_one(input.as_bytes(), WINDOW_FIELDS, Window::decode)
            .map_err(|e| crate::error::map_byte_parse_error("Window", WINDOW_INTENT.as_str(), e))
    }
    use super::PaneId;
    use crate::Result;
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
        let sessions: Result<Vec<Window>> = output.iter().map(|line| parse_one(line)).collect();
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
        let window = parse_one(input).expect("Should parse window with single pane");

        assert_eq!(window.id, WindowId::from_str("@5").unwrap());
        assert_eq!(window.index, 0);
        assert!(window.is_active);
        assert_eq!(window.name, "ben");
        assert_eq!(window.sessions, vec!["rust".to_string()]);
    }

    #[test]
    fn parse_window_with_large_index() {
        let input = "@100\x1f99\x1ffalse\x1f64f0,334x85,0,0,11\x1f4\x1ftest\x1f7\x1fsession\n";
        let window = parse_one(input).expect("Should parse window with large index");

        assert_eq!(window.id, WindowId::from_str("@100").unwrap());
        assert_eq!(window.index, 99);
        assert!(!window.is_active);
    }

    #[test]
    fn parse_window_fails_on_missing_id() {
        let input = "bad\x1f0\x1ftrue\x1f64f0,334x85,0,0,11\x1f4\x1fname\x1f7\x1fsession\n";
        let result = parse_one(input);

        assert!(result.is_err());
    }

    #[test]
    fn parse_window_fails_on_invalid_boolean() {
        let input = "@1\x1f0\x1fyes\x1f64f0,334x85,0,0,11\x1f4\x1fname\x1f7\x1fsession\n";
        let result = parse_one(input);

        assert!(result.is_err());
    }

    #[test]
    fn parse_window_fails_on_empty_name() {
        let input = "@1\x1f0\x1ftrue\x1f64f0,334x85,0,0,11\x1f0\x1f\x1f7\x1fsession\n";
        let result = parse_one(input);

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
        assert!(parse_one("@5:0:true:64f0,334x85,0,0,11:'name':'session'").is_err());
    }

    #[test]
    fn parse_framed_window_preserves_arbitrary_utf8_data() {
        let name = "π's: \\\x1f# $;\n";
        let sessions = "session:two\\\x1f\n";
        let window = parse_one(
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
            assert!(parse_all(&record).is_err());
        }

        let invalid_length =
            b"@5\x1f0\x1ftrue\x1f64f0,334x85,0,0,11\x1fnot-a-number\x1fname\x1f7\x1fsession\n";
        assert!(parse_all(invalid_length).is_err());
    }
}

#[cfg(test)]
mod id_tests {
    use super::*;

    #[test]
    fn test_parse_window_id_fn() {
        let actual = parse_window_id("@43");
        let expected = Ok(("", WindowId("@43".into())));
        assert_eq!(actual, expected);

        let actual = parse_window_id("@4");
        let expected = Ok(("", WindowId("@4".into())));
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_parse_window_id_struct() {
        let actual = WindowId::from_str("@43");
        assert!(actual.is_ok());
        assert_eq!(actual.unwrap(), WindowId("@43".into()));

        let actual = WindowId::from_str("4:38");
        assert!(matches!(
            actual,
            Err(Error::ParseError {
                desc: "WindowId",
                intent: "##{window_id}",
                message: _
            })
        ));
    }

    #[test]
    fn test_parse_window_id_with_large_number() {
        let window_id = WindowId::from_str("@99999").unwrap();
        assert_eq!(window_id.as_str(), "@99999");
    }

    #[test]
    fn test_parse_window_id_zero() {
        let window_id = WindowId::from_str("@0").unwrap();
        assert_eq!(window_id.as_str(), "@0");
    }

    #[test]
    fn test_parse_window_id_fails_on_wrong_prefix() {
        // $ is for session, % is for pane
        assert!(WindowId::from_str("$1").is_err());
        assert!(WindowId::from_str("%1").is_err());
    }

    #[test]
    fn test_parse_window_id_fails_on_no_prefix() {
        assert!(WindowId::from_str("123").is_err());
    }

    #[test]
    fn test_parse_window_id_fails_on_empty() {
        assert!(WindowId::from_str("").is_err());
        assert!(WindowId::from_str("@").is_err());
    }

    #[test]
    fn test_parse_window_id_fails_on_non_numeric() {
        assert!(WindowId::from_str("@abc").is_err());
        assert!(WindowId::from_str("@12abc").is_err());
    }

    #[test]
    fn test_parse_window_id_fails_on_extra_content() {
        // all_consuming should reject trailing content
        assert!(WindowId::from_str("@12:extra").is_err());
    }

    #[test]
    fn test_window_id_as_str() {
        let window_id = WindowId::from_str("@42").unwrap();
        assert_eq!(window_id.as_str(), "@42");
    }

    #[test]
    fn test_window_id_leaves_remaining_in_parser() {
        // The parse function (not FromStr) should leave remaining input
        let (remaining, window_id) = parse_window_id("@42:rest").unwrap();
        assert_eq!(remaining, ":rest");
        assert_eq!(window_id, WindowId("@42".into()));
    }
}
