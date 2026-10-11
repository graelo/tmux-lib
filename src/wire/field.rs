//! Declarative description of one tmux `-F` output format.
//!
//! A record type declares its fields once, in order, and both the `-F` string
//! sent to tmux and the human-readable intent string used in error messages
//! are derived from that single declaration. Keeping them derived rather than
//! hand-written is what stops the two from drifting apart, which they did
//! while each was maintained separately at its own escape depth.

use super::framing::FIELD_SEPARATOR;

/// One field of a framed tmux record.
///
/// The variant determines how tmux is asked to emit the field, and therefore
/// how the reader must consume it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Field {
    /// An ASCII structural value with no length prefix, such as `pane_id` or
    /// `window_layout`. Safe unprefixed because tmux cannot produce a
    /// separator byte inside one.
    Token(&'static str),
    /// A boolean, emitted as the literal `true` or `false`.
    Flag(&'static str),
    /// Arbitrary user data, emitted as a byte length followed by the value.
    /// The length prefix is what allows the value to contain separators and
    /// newlines.
    Data(&'static str),
}

impl Field {
    /// Render this field as its tmux format expansion.
    fn format(self) -> String {
        match self {
            Field::Token(name) => format!("#{{{name}}}"),
            Field::Flag(name) => format!("#{{?{name},true,false}}"),
            // `#{n:...}` is the byte length. The `#{s|...|...|:...}`
            // substitution doubles every literal backslash in the value,
            // because tmux 3.4 and 3.5 escape command output with
            // `VIS_NOSLASH`, which leaves backslashes untouched and would
            // otherwise make the escaping ambiguous. See
            // `super::framing::normalize_tmux_output`.
            Field::Data(name) => {
                format!(
                    "#{{n:{name}}}{SEP}#{{s|\\\\|\\\\\\\\|:{name}}}",
                    SEP = FIELD_SEPARATOR as char
                )
            }
        }
    }
}

/// Build the `-F` argument for a record type from its field list.
pub(crate) fn format_of(fields: &[Field]) -> String {
    fields
        .iter()
        .map(|field| field.format())
        .collect::<Vec<_>>()
        .join(&(FIELD_SEPARATOR as char).to_string())
}

/// Build the human-readable twin of [`format_of`], used as the `intent` of a
/// parse error.
///
/// It is the same string with the separator byte spelled out and the record
/// terminator made visible, so an error message can be pasted into a shell.
pub(crate) fn intent_of(fields: &[Field]) -> String {
    let mut intent = format_of(fields).replace(FIELD_SEPARATOR as char, "\\x1f");
    intent.push_str("\\n");
    intent
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_renders_as_a_bare_reference() {
        assert_eq!(Field::Token("pane_id").format(), "#{pane_id}");
    }

    #[test]
    fn flag_renders_as_a_conditional() {
        assert_eq!(
            Field::Flag("pane_active").format(),
            "#{?pane_active,true,false}"
        );
    }

    #[test]
    fn data_renders_as_a_length_prefix_and_a_backslash_doubling_value() {
        assert_eq!(
            Field::Data("pane_title").format(),
            "#{n:pane_title}\x1f#{s|\\\\|\\\\\\\\|:pane_title}"
        );
    }

    #[test]
    fn format_joins_fields_with_the_separator() {
        let fields = [Field::Token("session_id"), Field::Data("session_name")];

        assert_eq!(
            format_of(&fields),
            "#{session_id}\x1f#{n:session_name}\x1f#{s|\\\\|\\\\\\\\|:session_name}"
        );
    }

    #[test]
    fn intent_spells_out_the_separators_and_the_terminator() {
        let fields = [Field::Token("session_id"), Field::Data("session_name")];

        assert_eq!(
            intent_of(&fields),
            "#{session_id}\\x1f#{n:session_name}\\x1f#{s|\\\\|\\\\\\\\|:session_name}\\n"
        );
    }
}
