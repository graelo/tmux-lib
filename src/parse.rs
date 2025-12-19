use nom::{
    branch::alt,
    bytes::complete::{escaped, tag},
    character::complete::none_of,
    combinator::value,
    sequence::delimited,
    IResult, Parser,
};

/// Return the `&str` between single quotes. The returned string may be empty.
#[allow(unused)]
pub(crate) fn quoted_string(input: &str) -> IResult<&str, &str> {
    let esc = escaped(none_of("\\\'"), '\\', tag("'"));
    let esc_or_empty = alt((esc, tag("")));

    delimited(tag("'"), esc_or_empty, tag("'")).parse(input)
}

/// Return the `&str` between single quotes. The returned string may not be empty.
pub(crate) fn quoted_nonempty_string(input: &str) -> IResult<&str, &str> {
    let esc = escaped(none_of("\\\'"), '\\', tag("'"));
    delimited(tag("'"), esc, tag("'")).parse(input)
}

/// Return a bool: allowed values: `"true"` or `"false"`.
pub(crate) fn boolean(input: &str) -> IResult<&str, bool> {
    // This is a parser that returns `true` if it sees the string "true", and
    // an error otherwise.
    let parse_true = value(true, tag("true"));

    let parse_false = value(false, tag("false"));

    alt((parse_true, parse_false)).parse(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quoted_nonempty_string() {
        let (input, res) = quoted_nonempty_string(r#"'foo\' 🤖 bar'"#).unwrap();
        assert!(input.is_empty());
        assert_eq!(res, r#"foo\' 🤖 bar"#);
        let (input, res) = quoted_nonempty_string("'λx → x'").unwrap();
        assert!(input.is_empty());
        assert_eq!(res, "λx → x");
        let (input, res) = quoted_nonempty_string("'  '").unwrap();
        assert!(input.is_empty());
        assert_eq!(res, "  ");

        assert!(quoted_nonempty_string("''").is_err());
    }

    #[test]
    fn test_quoted_string() {
        let (input, res) = quoted_string("''").unwrap();
        assert!(input.is_empty());
        assert!(res.is_empty());
    }

    #[test]
    fn test_quoted_string_with_content() {
        let (input, res) = quoted_string("'hello world'").unwrap();
        assert!(input.is_empty());
        assert_eq!(res, "hello world");
    }

    #[test]
    fn test_quoted_string_with_escaped_quote() {
        let (input, res) = quoted_string(r"'it\'s working'").unwrap();
        assert!(input.is_empty());
        assert_eq!(res, r"it\'s working");
    }

    #[test]
    fn test_quoted_string_leaves_remaining_input() {
        let (input, res) = quoted_string("'first':rest").unwrap();
        assert_eq!(input, ":rest");
        assert_eq!(res, "first");
    }

    #[test]
    fn test_quoted_string_fails_without_quotes() {
        assert!(quoted_string("no quotes").is_err());
    }

    #[test]
    fn test_quoted_string_fails_on_unclosed() {
        assert!(quoted_string("'unclosed").is_err());
    }

    #[test]
    fn test_quoted_nonempty_string_with_special_chars() {
        let (input, res) = quoted_nonempty_string("'path/to/file:with:colons'").unwrap();
        assert!(input.is_empty());
        assert_eq!(res, "path/to/file:with:colons");
    }

    #[test]
    fn test_quoted_nonempty_string_fails_on_empty() {
        assert!(quoted_nonempty_string("''").is_err());
    }

    #[test]
    fn test_boolean_true() {
        let (input, res) = boolean("true").unwrap();
        assert!(input.is_empty());
        assert!(res);
    }

    #[test]
    fn test_boolean_false() {
        let (input, res) = boolean("false").unwrap();
        assert!(input.is_empty());
        assert!(!res);
    }

    #[test]
    fn test_boolean_leaves_remaining_input() {
        let (input, res) = boolean("true:next").unwrap();
        assert_eq!(input, ":next");
        assert!(res);
    }

    #[test]
    fn test_boolean_fails_on_invalid() {
        assert!(boolean("yes").is_err());
        assert!(boolean("no").is_err());
        assert!(boolean("1").is_err());
        assert!(boolean("0").is_err());
        assert!(boolean("TRUE").is_err());
        assert!(boolean("FALSE").is_err());
    }

    #[test]
    fn test_boolean_fails_on_empty() {
        assert!(boolean("").is_err());
    }
}
