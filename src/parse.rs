use std::str;

use nom::{
    IResult, Parser,
    branch::alt,
    bytes::complete::{escaped, tag},
    character::complete::none_of,
    combinator::value,
    sequence::delimited,
};

// Unit Separator is an ASCII control character intended for structural field
// separation and is unlikely to occur in ordinary tmux metadata. It need not
// be absent from data because data fields are length-prefixed.
pub(crate) const FIELD_SEPARATOR: u8 = 0x1f;

// tmux terminates each list output record with a newline. Newlines inside data
// remain safe because the parser consumes fields by byte length rather than
// splitting the output on newline.
pub(crate) const RECORD_SEPARATOR: u8 = b'\n';

/// Distinguish a framed record from the legacy colon-delimited form.
pub(crate) fn looks_like_framed(input: &[u8]) -> bool {
    match (
        input.iter().position(|&byte| byte == FIELD_SEPARATOR),
        input.iter().position(|&byte| byte == b':'),
    ) {
        (Some(separator), Some(colon)) => separator < colon,
        (Some(_), None) => true,
        _ => false,
    }
}

#[derive(Debug)]
pub(crate) struct ByteParseError {
    message: String,
}

impl ByteParseError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ByteParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
    }
}

/// Read a framed tmux record without treating data bytes as delimiters.
pub(crate) struct ByteCursor<'a> {
    input: &'a [u8],
    position: usize,
}

impl<'a> ByteCursor<'a> {
    pub(crate) fn new(input: &'a [u8]) -> Self {
        Self { input, position: 0 }
    }

    pub(crate) fn is_at_end(&self) -> bool {
        self.position == self.input.len()
    }

    /// Consume an ASCII structural token followed by the field separator.
    pub(crate) fn take_token(&mut self) -> Result<&'a [u8], ByteParseError> {
        let start = self.position;
        let relative_end = self.input[start..]
            .iter()
            .position(|&byte| byte == FIELD_SEPARATOR)
            .ok_or_else(|| {
                ByteParseError::new(format!(
                    "missing field separator after structural token at byte {start}"
                ))
            })?;
        let end = start + relative_end;
        self.position = end + 1;
        Ok(&self.input[start..end])
    }

    pub(crate) fn take_token_str(&mut self, field: &str) -> Result<&'a str, ByteParseError> {
        let token = self.take_token()?;
        Self::utf8(token, field)
    }

    /// Consume a decimal byte length, its data, and the specified terminator.
    pub(crate) fn take_length_prefixed(
        &mut self,
        terminator: u8,
    ) -> Result<&'a [u8], ByteParseError> {
        let length_bytes = self.take_token()?;
        let length_text = str::from_utf8(length_bytes).map_err(|_| {
            ByteParseError::new(format!(
                "field length at byte {} is not ASCII",
                self.position - length_bytes.len() - 1
            ))
        })?;
        if length_bytes.is_empty() || !length_bytes.iter().all(u8::is_ascii_digit) {
            return Err(ByteParseError::new(format!(
                "invalid field length `{length_text}`"
            )));
        }
        let length = length_text
            .parse::<usize>()
            .map_err(|_| ByteParseError::new(format!("invalid field length `{length_text}`")))?;
        let data_end = self
            .position
            .checked_add(length)
            .ok_or_else(|| ByteParseError::new("field length overflows input bounds"))?;
        if data_end > self.input.len() {
            return Err(ByteParseError::new(format!(
                "field length {length} exceeds remaining input at byte {}",
                self.position
            )));
        }
        let data = &self.input[self.position..data_end];
        self.position = data_end;
        if self.input.get(self.position) != Some(&terminator) {
            return Err(ByteParseError::new(format!(
                "missing field terminator at byte {}",
                self.position
            )));
        }
        self.position += 1;
        Ok(data)
    }

    pub(crate) fn take_length_prefixed_string(
        &mut self,
        terminator: u8,
        field: &str,
    ) -> Result<String, ByteParseError> {
        let data = self.take_length_prefixed(terminator)?;
        Ok(Self::utf8(data, field)?.to_owned())
    }

    pub(crate) fn utf8(data: &'a [u8], field: &str) -> Result<&'a str, ByteParseError> {
        str::from_utf8(data).map_err(|_| ByteParseError::new(format!("{field} is not valid UTF-8")))
    }
}

/// Return the `&str` between single quotes. The returned string may be empty.
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
