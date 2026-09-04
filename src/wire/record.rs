//! Reading framed tmux records, driven by a declared field list.
//!
//! Every record type used to carry its own near-identical `mod parse`, each
//! re-deriving which fields are length-prefixed and which field terminates the
//! record. That knowledge now lives here once, and a model type only says what
//! its fields mean.

use super::field::Field;
use super::framing::{ByteCursor, ByteParseError, FIELD_SEPARATOR, RECORD_SEPARATOR};

/// Reads the fields of one record, in the order they were declared.
pub(crate) struct RecordReader<'c, 'a> {
    cursor: &'c mut ByteCursor<'a>,
    fields: &'static [Field],
    next: usize,
}

impl<'c, 'a> RecordReader<'c, 'a> {
    fn new(cursor: &'c mut ByteCursor<'a>, fields: &'static [Field]) -> Self {
        Self {
            cursor,
            fields,
            next: 0,
        }
    }

    /// Check the next declared field is of the expected kind, and report which
    /// byte terminates it.
    ///
    /// A kind mismatch means a model's decoder and its field list have drifted
    /// apart. That is a bug rather than malformed input, but reporting it
    /// beats misreading the stream.
    fn advance(&mut self, expected: &str) -> Result<u8, ByteParseError> {
        let field = self.fields.get(self.next).ok_or_else(|| {
            ByteParseError::new(format!(
                "decoder asked for a {expected} field past the end of a {}-field record",
                self.fields.len()
            ))
        })?;

        let actual = match field {
            Field::Token(_) => "token",
            Field::Flag(_) => "flag",
            Field::Data(_) => "data",
        };
        if actual != expected {
            return Err(ByteParseError::new(format!(
                "field {} is declared as {actual} but was read as {expected}",
                self.next
            )));
        }

        let terminator = if self.next + 1 == self.fields.len() {
            RECORD_SEPARATOR
        } else {
            FIELD_SEPARATOR
        };
        self.next += 1;
        Ok(terminator)
    }

    /// Read the next field as an ASCII structural token.
    pub(crate) fn token(&mut self, label: &str) -> Result<&'a str, ByteParseError> {
        let terminator = self.advance("token")?;
        self.cursor.take_token_str(terminator, label)
    }

    /// Read the next field as a boolean.
    pub(crate) fn flag(&mut self, label: &str) -> Result<bool, ByteParseError> {
        let terminator = self.advance("flag")?;
        match self.cursor.take_token_str(terminator, label)? {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(ByteParseError::new(format!("invalid {label}"))),
        }
    }

    /// Read the next field as length-prefixed user data.
    pub(crate) fn data(&mut self, label: &str) -> Result<String, ByteParseError> {
        let terminator = self.advance("data")?;
        self.cursor.take_length_prefixed_string(terminator, label)
    }

    /// Read the next field as length-prefixed user data, rejecting an empty
    /// value.
    pub(crate) fn required_data(&mut self, label: &str) -> Result<String, ByteParseError> {
        let value = self.data(label)?;
        if value.is_empty() {
            return Err(ByteParseError::new(format!("{label} is empty")));
        }
        Ok(value)
    }

    fn finish(self) -> Result<(), ByteParseError> {
        if self.next != self.fields.len() {
            return Err(ByteParseError::new(format!(
                "decoder read {} of {} declared fields",
                self.next,
                self.fields.len()
            )));
        }
        Ok(())
    }
}

/// Decoder for one record type: reads declared fields in order and builds the
/// model value.
pub(crate) type Decoder<T> = fn(&mut RecordReader<'_, '_>) -> Result<T, ByteParseError>;

/// Decode exactly one record, rejecting anything that follows it.
pub(crate) fn decode_one<T>(
    input: &[u8],
    fields: &'static [Field],
    decode: Decoder<T>,
) -> Result<T, ByteParseError> {
    let mut cursor = ByteCursor::new(input);
    let value = read_one(&mut cursor, fields, decode)?;
    if !cursor.is_at_end() {
        return Err(ByteParseError::new(
            "unexpected trailing bytes after record",
        ));
    }
    Ok(value)
}

/// Decode every record in the stream.
pub(crate) fn decode_all<T>(
    input: &[u8],
    fields: &'static [Field],
    decode: Decoder<T>,
) -> Result<Vec<T>, ByteParseError> {
    let mut cursor = ByteCursor::new(input);
    let mut values = Vec::new();
    while !cursor.is_at_end() {
        values.push(read_one(&mut cursor, fields, decode)?);
    }
    Ok(values)
}

fn read_one<T>(
    cursor: &mut ByteCursor<'_>,
    fields: &'static [Field],
    decode: Decoder<T>,
) -> Result<T, ByteParseError> {
    let mut reader = RecordReader::new(cursor, fields);
    let value = decode(&mut reader)?;
    reader.finish()?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIELDS: &[Field] = &[
        Field::Token("id"),
        Field::Flag("active"),
        Field::Data("title"),
    ];

    #[derive(Debug, PartialEq, Eq)]
    struct Row {
        id: String,
        active: bool,
        title: String,
    }

    fn decode(reader: &mut RecordReader<'_, '_>) -> Result<Row, ByteParseError> {
        Ok(Row {
            id: reader.token("id")?.to_owned(),
            active: reader.flag("active")?,
            title: reader.data("title")?,
        })
    }

    #[test]
    fn decodes_a_single_record() {
        let row = decode_one(b"%1\x1ftrue\x1f5\x1fhello\n", FIELDS, decode).unwrap();

        assert_eq!(
            row,
            Row {
                id: "%1".into(),
                active: true,
                title: "hello".into()
            }
        );
    }

    #[test]
    fn decodes_every_record_in_a_stream() {
        let rows = decode_all(
            b"%1\x1ftrue\x1f1\x1fa\n%2\x1ffalse\x1f1\x1fb\n",
            FIELDS,
            decode,
        )
        .unwrap();

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].id, "%2");
        assert!(!rows[1].active);
    }

    #[test]
    fn data_may_contain_separators_and_newlines() {
        let row = decode_one(b"%1\x1ftrue\x1f4\x1fa\nb\x1f\n", FIELDS, decode).unwrap();

        assert_eq!(row.title, "a\nb\x1f");
    }

    #[test]
    fn rejects_trailing_bytes_after_a_single_record() {
        let result = decode_one(
            b"%1\x1ftrue\x1f1\x1fa\n%2\x1ftrue\x1f1\x1fb\n",
            FIELDS,
            decode,
        );

        assert!(result.is_err());
    }

    #[test]
    fn rejects_an_invalid_flag() {
        assert!(decode_one(b"%1\x1fyes\x1f1\x1fa\n", FIELDS, decode).is_err());
    }

    #[test]
    fn rejects_a_decoder_that_disagrees_with_the_field_list() {
        fn wrong(reader: &mut RecordReader<'_, '_>) -> Result<String, ByteParseError> {
            reader.data("id").map(|_| String::new())
        }

        let error = decode_one(b"%1\x1ftrue\x1f1\x1fa\n", FIELDS, wrong).unwrap_err();

        assert!(error.to_string().contains("declared as token"));
    }

    #[test]
    fn rejects_a_decoder_that_stops_early() {
        fn short(reader: &mut RecordReader<'_, '_>) -> Result<String, ByteParseError> {
            reader.token("id").map(str::to_owned)
        }

        let error = decode_one(b"%1\x1ftrue\x1f1\x1fa\n", FIELDS, short).unwrap_err();

        assert!(error.to_string().contains("read 1 of 3"));
    }

    #[test]
    fn required_data_rejects_an_empty_value() {
        fn decode_required(reader: &mut RecordReader<'_, '_>) -> Result<Row, ByteParseError> {
            Ok(Row {
                id: reader.token("id")?.to_owned(),
                active: reader.flag("active")?,
                title: reader.required_data("title")?,
            })
        }

        assert!(decode_one(b"%1\x1ftrue\x1f0\x1f\n", FIELDS, decode_required).is_err());
        assert!(decode_one(b"%1\x1ftrue\x1f0\x1f\n", FIELDS, decode).is_ok());
    }
}
