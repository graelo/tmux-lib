//! The tmux output formats this crate asks for, declared once each.
//!
//! Every `-F` argument the crate sends lives here. Adding a field to a record
//! is a one-line change in one list: the `-F` string, the error-message intent
//! string, and the reader's terminator choice all follow from the declaration.

use std::sync::LazyLock;

use super::field::{Field, format_of, intent_of};

/// Fields of a `display-message` client record.
pub(crate) const CLIENT_FIELDS: &[Field] = &[
    Field::Data("client_session"),
    Field::Data("client_last_session"),
];

/// Fields of a `list-panes` record.
pub(crate) const PANE_FIELDS: &[Field] = &[
    Field::Token("pane_id"),
    Field::Token("pane_index"),
    Field::Flag("pane_active"),
    Field::Data("pane_title"),
    Field::Data("pane_current_command"),
    Field::Data("pane_current_path"),
];

/// Fields of a `list-windows` record.
pub(crate) const WINDOW_FIELDS: &[Field] = &[
    Field::Token("window_id"),
    Field::Token("window_index"),
    Field::Flag("window_active"),
    Field::Token("window_layout"),
    Field::Data("window_name"),
    Field::Data("window_linked_sessions_list"),
];

/// Fields of a `list-sessions` record.
pub(crate) const SESSION_FIELDS: &[Field] = &[
    Field::Token("session_id"),
    Field::Data("session_name"),
    Field::Data("session_path"),
];

macro_rules! derived_format {
    ($format:ident, $intent:ident, $fields:ident) => {
        pub(crate) static $format: LazyLock<String> = LazyLock::new(|| format_of($fields));
        pub(crate) static $intent: LazyLock<String> = LazyLock::new(|| intent_of($fields));
    };
}

derived_format!(CLIENT_FORMAT, CLIENT_INTENT, CLIENT_FIELDS);
derived_format!(PANE_FORMAT, PANE_INTENT, PANE_FIELDS);
derived_format!(WINDOW_FORMAT, WINDOW_INTENT, WINDOW_FIELDS);
derived_format!(SESSION_FORMAT, SESSION_INTENT, SESSION_FIELDS);

/// Format asked of `list-sessions` when only probing server readiness.
pub(crate) const SESSION_NAME_FORMAT: &str = "#{session_name}";

/// Formats asked of the creation commands via `-P -F`, which report the ids of
/// what they just created. These are not framed records: tmux ids cannot
/// contain `:`, so a plain join is unambiguous.
pub(crate) const NEW_PANE_FORMAT: &str = "#{pane_id}";
pub(crate) const NEW_WINDOW_FORMAT: &str = "#{window_id}:#{pane_id}";
pub(crate) const NEW_SESSION_FORMAT: &str = "#{session_id}:#{window_id}:#{pane_id}";

/// Intent twins of the creation formats. `#` is doubled the way tmux itself
/// escapes it, matching what these strings looked like when they were written
/// by hand at each call site.
pub(crate) const NEW_WINDOW_INTENT: &str = "##{window_id}:##{pane_id}";
pub(crate) const NEW_SESSION_INTENT: &str = "##{session_id}:##{window_id}:##{pane_id}";

#[cfg(test)]
mod tests {
    use super::*;

    // These four assertions pin the derived strings to the literals that were
    // maintained by hand before the field lists existed. They are the proof
    // that introducing `Field` changed no bytes on the wire.

    #[test]
    fn client_format_matches_the_hand_written_literal() {
        assert_eq!(
            *CLIENT_FORMAT,
            "#{n:client_session}\x1f#{s|\\\\|\\\\\\\\|:client_session}\x1f#{n:client_last_session}\x1f#{s|\\\\|\\\\\\\\|:client_last_session}"
        );
        assert_eq!(
            *CLIENT_INTENT,
            "#{n:client_session}\\x1f#{s|\\\\|\\\\\\\\|:client_session}\\x1f#{n:client_last_session}\\x1f#{s|\\\\|\\\\\\\\|:client_last_session}\\n"
        );
    }

    #[test]
    fn pane_format_matches_the_hand_written_literal() {
        assert_eq!(
            *PANE_FORMAT,
            "#{pane_id}\x1f#{pane_index}\x1f#{?pane_active,true,false}\x1f#{n:pane_title}\x1f#{s|\\\\|\\\\\\\\|:pane_title}\x1f#{n:pane_current_command}\x1f#{s|\\\\|\\\\\\\\|:pane_current_command}\x1f#{n:pane_current_path}\x1f#{s|\\\\|\\\\\\\\|:pane_current_path}"
        );
        assert_eq!(
            *PANE_INTENT,
            "#{pane_id}\\x1f#{pane_index}\\x1f#{?pane_active,true,false}\\x1f#{n:pane_title}\\x1f#{s|\\\\|\\\\\\\\|:pane_title}\\x1f#{n:pane_current_command}\\x1f#{s|\\\\|\\\\\\\\|:pane_current_command}\\x1f#{n:pane_current_path}\\x1f#{s|\\\\|\\\\\\\\|:pane_current_path}\\n"
        );
    }

    #[test]
    fn window_format_matches_the_hand_written_literal() {
        assert_eq!(
            *WINDOW_FORMAT,
            "#{window_id}\x1f#{window_index}\x1f#{?window_active,true,false}\x1f#{window_layout}\x1f#{n:window_name}\x1f#{s|\\\\|\\\\\\\\|:window_name}\x1f#{n:window_linked_sessions_list}\x1f#{s|\\\\|\\\\\\\\|:window_linked_sessions_list}"
        );
        assert_eq!(
            *WINDOW_INTENT,
            "#{window_id}\\x1f#{window_index}\\x1f#{?window_active,true,false}\\x1f#{window_layout}\\x1f#{n:window_name}\\x1f#{s|\\\\|\\\\\\\\|:window_name}\\x1f#{n:window_linked_sessions_list}\\x1f#{s|\\\\|\\\\\\\\|:window_linked_sessions_list}\\n"
        );
    }

    #[test]
    fn session_format_matches_the_hand_written_literal() {
        assert_eq!(
            *SESSION_FORMAT,
            "#{session_id}\x1f#{n:session_name}\x1f#{s|\\\\|\\\\\\\\|:session_name}\x1f#{n:session_path}\x1f#{s|\\\\|\\\\\\\\|:session_path}"
        );
        assert_eq!(
            *SESSION_INTENT,
            "#{session_id}\\x1f#{n:session_name}\\x1f#{s|\\\\|\\\\\\\\|:session_name}\\x1f#{n:session_path}\\x1f#{s|\\\\|\\\\\\\\|:session_path}\\n"
        );
    }
}
