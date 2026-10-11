//! Rendering an argument vector as one control-mode command line.
//!
//! The control connection takes one command per line, so every argument has
//! to survive tmux's own command lexer intact — including spaces, quotes of
//! both kinds, backslashes, `$`, `#{…}` and `;`.
//!
//! Single-quoting does that: inside single quotes tmux processes no escapes at
//! all, and outside them a backslash escapes the next character, so an
//! embedded single quote is written the way a shell writes it — close, escape,
//! reopen.

use crate::{Result, error::Error};

/// Render `argv` as the one line to write to the control connection.
///
/// # Errors
///
/// Returns [`Error::UnsupportedArgument`] for an argument containing a
/// newline. Control mode is one command per line and tmux does not continue an
/// unterminated quote across lines, so such an argument would be truncated at
/// the newline and the remainder read as a command of its own. Refusing it is
/// the only honest option on this transport; the caller's recourse is the
/// spawning transport, which has no such limit.
pub(crate) fn command_line(argv: &[&str]) -> Result<String> {
    let mut line = String::new();

    for argument in argv {
        if argument.contains('\n') {
            return Err(Error::UnsupportedArgument {
                argument: (*argument).to_owned(),
            });
        }

        if !line.is_empty() {
            line.push(' ');
        }
        quote_into(argument, &mut line);
    }

    Ok(line)
}

/// Append `argument` to `line`, single-quoted.
fn quote_into(argument: &str, line: &mut String) {
    line.push('\'');
    for c in argument.chars() {
        if c == '\'' {
            // Leave the quoted run, write an escaped quote, start a new run.
            line.push_str("'\\''");
        } else {
            line.push(c);
        }
    }
    line.push('\'');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(argv: &[&str]) -> String {
        command_line(argv).unwrap()
    }

    #[test]
    fn every_argument_is_quoted_even_when_it_needs_nothing() {
        // Quoting unconditionally is what makes the rule reviewable: there is
        // no set of characters to keep in sync with tmux's lexer.
        assert_eq!(rendered(&["list-panes", "-a"]), "'list-panes' '-a'");
    }

    #[test]
    fn spaces_stay_inside_one_argument() {
        assert_eq!(
            rendered(&["display-message", "two words"]),
            "'display-message' 'two words'"
        );
    }

    #[test]
    fn a_single_quote_closes_escapes_and_reopens() {
        assert_eq!(rendered(&["it's here"]), r"'it'\''s here'");
    }

    #[test]
    fn characters_tmux_would_otherwise_act_on_are_left_alone() {
        // A bare `;` separates commands, `#{…}` is a format, `$` a variable,
        // `#` a comment, `"` another quote. Verified on tmux 3.7c to round
        // trip through `set-option` and `show-options -v`.
        assert_eq!(rendered(&["a ; b"]), "'a ; b'");
        assert_eq!(rendered(&["#{?a,b,c}"]), "'#{?a,b,c}'");
        assert_eq!(rendered(&["$HOME"]), "'$HOME'");
        assert_eq!(rendered(&["# comment?"]), "'# comment?'");
        assert_eq!(rendered(&[r#"say "hi""#]), r#"'say "hi"'"#);
    }

    #[test]
    fn a_trailing_backslash_does_not_escape_the_closing_quote() {
        // Inside single quotes tmux processes no escapes, so the backslash is
        // literal and the quote that follows still closes the argument.
        assert_eq!(rendered(&[r"ends with \"]), r"'ends with \'");
    }

    #[test]
    fn an_empty_argument_survives_as_an_empty_argument() {
        assert_eq!(rendered(&["select-pane", ""]), "'select-pane' ''");
    }

    #[test]
    fn an_empty_argv_renders_an_empty_line() {
        assert_eq!(rendered(&[]), "");
    }

    #[test]
    fn a_newline_is_refused_rather_than_truncated() {
        let error = command_line(&["set-option", "@x", "line1\nline2"]).unwrap_err();

        let Error::UnsupportedArgument { argument } = error else {
            panic!("expected UnsupportedArgument, got {error:?}");
        };
        assert_eq!(argument, "line1\nline2");
    }

    #[test]
    fn a_newline_anywhere_in_the_argv_is_refused() {
        assert!(command_line(&["a\n"]).is_err());
        assert!(command_line(&["\nb"]).is_err());
        assert!(command_line(&["ok", "also ok", "not\nok"]).is_err());
    }
}
