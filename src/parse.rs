use std::str;

// Unit Separator is an ASCII control character intended for structural field
// separation and is unlikely to occur in ordinary tmux metadata. It need not
// be absent from data because data fields are length-prefixed.
pub(crate) const FIELD_SEPARATOR: u8 = 0x1f;

// tmux terminates each list output record with a newline. Newlines inside data
// remain safe because the parser consumes fields by byte length rather than
// splitting the output on newline.
pub(crate) const RECORD_SEPARATOR: u8 = b'\n';

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
