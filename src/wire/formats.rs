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

/// Fields of a `list-clients` record.
///
/// `client_name` is a terminal path today, but it is user-visible state and is
/// framed like any other, rather than joined with a separator this crate would
/// then have to forbid.
pub(crate) const CLIENT_LIST_FIELDS: &[Field] = &[
    Field::Token("client_activity"),
    Field::Flag("client_control_mode"),
    Field::Data("client_name"),
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
derived_format!(CLIENT_LIST_FORMAT, CLIENT_LIST_INTENT, CLIENT_LIST_FIELDS);
derived_format!(PANE_FORMAT, PANE_INTENT, PANE_FIELDS);
derived_format!(WINDOW_FORMAT, WINDOW_INTENT, WINDOW_FIELDS);
derived_format!(SESSION_FORMAT, SESSION_INTENT, SESSION_FIELDS);

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
