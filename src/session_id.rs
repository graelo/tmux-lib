//! Session Id.

use std::str::FromStr;

use nom::{
    character::complete::{char, digit1},
    combinator::all_consuming,
    sequence::preceded,
    IResult, Parser,
};
use serde::{Deserialize, Serialize};

use crate::error::{map_add_intent, Error};

/// The id of a Tmux session.
///
/// This wraps the raw tmux representation (`$11`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionId(String);

impl FromStr for SessionId {
    type Err = Error;

    /// Parse into SessionId. The `&str` must start with '$' followed by a
    /// `u16`.
    fn from_str(input: &str) -> std::result::Result<Self, Self::Err> {
        let desc = "SessionId";
        let intent = "##{session_id}";

        let (_, sess_id) = all_consuming(parse::session_id)
            .parse(input)
            .map_err(|e| map_add_intent(desc, intent, e))?;

        Ok(sess_id)
    }
}

impl SessionId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub(crate) mod parse {
    use super::*;

    pub fn session_id(input: &str) -> IResult<&str, SessionId> {
        let (input, digit) = preceded(char('$'), digit1).parse(input)?;
        let id = format!("${digit}");
        Ok((input, SessionId(id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_session_id_fn() {
        let actual = parse::session_id("$43");
        let expected = Ok(("", SessionId("$43".into())));
        assert_eq!(actual, expected);

        let actual = parse::session_id("$4");
        let expected = Ok(("", SessionId("$4".into())));
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_parse_session_id_struct() {
        let actual = SessionId::from_str("$43");
        assert!(actual.is_ok());
        assert_eq!(actual.unwrap(), SessionId("$43".into()));

        let actual = SessionId::from_str("4:38");
        assert!(matches!(
            actual,
            Err(Error::ParseError {
                desc: "SessionId",
                intent: "##{session_id}",
                err: _
            })
        ));
    }

    #[test]
    fn test_parse_session_id_with_large_number() {
        let session_id = SessionId::from_str("$99999").unwrap();
        assert_eq!(session_id, SessionId("$99999".into()));
    }

    #[test]
    fn test_parse_session_id_zero() {
        let session_id = SessionId::from_str("$0").unwrap();
        assert_eq!(session_id, SessionId("$0".into()));
    }

    #[test]
    fn test_parse_session_id_fails_on_wrong_prefix() {
        // @ is for window, % is for pane
        assert!(SessionId::from_str("@1").is_err());
        assert!(SessionId::from_str("%1").is_err());
    }

    #[test]
    fn test_parse_session_id_fails_on_no_prefix() {
        assert!(SessionId::from_str("123").is_err());
    }

    #[test]
    fn test_parse_session_id_fails_on_empty() {
        assert!(SessionId::from_str("").is_err());
        assert!(SessionId::from_str("$").is_err());
    }

    #[test]
    fn test_parse_session_id_fails_on_non_numeric() {
        assert!(SessionId::from_str("$abc").is_err());
        assert!(SessionId::from_str("$12abc").is_err());
    }

    #[test]
    fn test_parse_session_id_fails_on_extra_content() {
        // all_consuming should reject trailing content
        assert!(SessionId::from_str("$12:extra").is_err());
    }

    #[test]
    fn test_session_id_leaves_remaining_in_parser() {
        // The parse function (not FromStr) should leave remaining input
        let (remaining, session_id) = parse::session_id("$42:rest").unwrap();
        assert_eq!(remaining, ":rest");
        assert_eq!(session_id, SessionId("$42".into()));
    }
}
