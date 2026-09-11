use std::io;

/// Describes all errors variants from this crate.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// A tmux invocation returned some output where none was expected (actions such as
    /// some `tmux display-message` invocations).
    #[error(
        "unexpected process output: intent: `{intent}`, stdout: `{stdout}`, stderr: `{stderr}`"
    )]
    UnexpectedTmuxOutput {
        intent: &'static str,
        stdout: String,
        stderr: String,
    },

    /// An argument cannot be carried by the transport in use.
    ///
    /// The control transport sends one command per line and tmux does not
    /// continue an unterminated quote across lines, so an argument holding a
    /// newline would be truncated and its remainder read as a command. The
    /// spawning transport has no such limit.
    #[error("argument holds a newline, which the control transport cannot carry: `{argument}`")]
    UnsupportedArgument {
        /// The argument that cannot be sent.
        argument: String,
    },

    /// Indicates Tmux has a weird config, like missing the `"default-shell"`.
    #[error("unexpected tmux config: `{0}`")]
    TmuxConfig(&'static str),

    /// Some parsing error.
    #[error("failed parsing {desc}: {message} (expected `{intent}`)")]
    ParseError {
        /// What was being parsed.
        desc: &'static str,
        /// The shape the parser expected, as a tmux format string.
        intent: &'static str,
        /// Why the parse failed.
        message: String,
    },

    /// Failed parsing the output of a process invocation as utf-8.
    #[error("failed parsing utf-8 string: `{source}`")]
    Utf8 {
        #[from]
        /// Source error.
        source: std::string::FromUtf8Error,
    },

    /// Some IO error.
    #[error("failed with io: `{source}`")]
    Io {
        #[from]
        /// Source error.
        source: io::Error,
    },
}

/// Convert a nom error into an owned error and add the parsing intent.
///
/// # Errors
///
/// This maps to a `Error::ParseError`.
#[must_use]
pub fn map_add_intent(
    desc: &'static str,
    intent: &'static str,
    nom_err: nom::Err<nom::error::Error<&str>>,
) -> Error {
    Error::ParseError {
        desc,
        intent,
        message: nom_err.to_string(),
    }
}

/// Convert a wire-protocol parsing error into the public parse error type.
pub(crate) fn map_byte_parse_error(
    desc: &'static str,
    intent: &'static str,
    message: impl std::fmt::Display,
) -> Error {
    Error::ParseError {
        desc,
        intent,
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_add_intent_creates_parse_error() {
        use nom::error::{Error as NomError, ErrorKind};

        let nom_err: nom::Err<NomError<&str>> =
            nom::Err::Error(NomError::new("remaining input", ErrorKind::Tag));

        let error = map_add_intent("description", "expected format", nom_err);

        match error {
            Error::ParseError { desc, intent, .. } => {
                assert_eq!(desc, "description");
                assert_eq!(intent, "expected format");
            }
            _ => panic!("Expected ParseError"),
        }
    }

    #[test]
    fn error_display_messages() {
        // Test UnexpectedTmuxOutput display
        let err = Error::UnexpectedTmuxOutput {
            intent: "test",
            stdout: "out".to_string(),
            stderr: "err".to_string(),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("unexpected process output"));
        assert!(msg.contains("test"));

        // Test TmuxConfig display
        let err = Error::TmuxConfig("missing default-shell");
        let msg = format!("{}", err);
        assert!(msg.contains("unexpected tmux config"));
        assert!(msg.contains("missing default-shell"));
    }
}
