//! Window Id.

use std::str::FromStr;

use nom::{
    character::complete::{char, digit1},
    combinator::all_consuming,
    sequence::preceded,
    IResult, Parser,
};
use serde::{Deserialize, Serialize};

use crate::error::{map_add_intent, Error};

/// The id of a Tmux window.
///
/// This wraps the raw tmux representation (`@41`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowId(String);

impl FromStr for WindowId {
    type Err = Error;

    /// Parse into WindowId. The `&str` must start with '@' followed by a
    /// `u16`.
    fn from_str(input: &str) -> std::result::Result<Self, Self::Err> {
        let desc = "WindowId";
        let intent = "##{window_id}";

        let (_, window_id) = all_consuming(parse::window_id)
            .parse(input)
            .map_err(|e| map_add_intent(desc, intent, e))?;

        Ok(window_id)
    }
}

impl WindowId {
    /// Extract a string slice containing the raw representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub(crate) mod parse {
    use super::*;

    pub(crate) fn window_id(input: &str) -> IResult<&str, WindowId> {
        let (input, digit) = preceded(char('@'), digit1).parse(input)?;
        let id = format!("@{digit}");
        Ok((input, WindowId(id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_window_id_fn() {
        let actual = parse::window_id("@43");
        let expected = Ok(("", WindowId("@43".into())));
        assert_eq!(actual, expected);

        let actual = parse::window_id("@4");
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
                err: _
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
        let (remaining, window_id) = parse::window_id("@42:rest").unwrap();
        assert_eq!(remaining, ":rest");
        assert_eq!(window_id, WindowId("@42".into()));
    }
}
