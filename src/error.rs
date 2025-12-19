use std::{io, process::Output};

/// Describes all errors variants from this crate.
#[derive(thiserror::Error, Debug)]
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

    /// Indicates Tmux has a weird config, like missing the `"default-shell"`.
    #[error("unexpected tmux config: `{0}`")]
    TmuxConfig(&'static str),

    /// Some parsing error.
    #[error("failed parsing: `{intent}`")]
    ParseError {
        desc: &'static str,
        intent: &'static str,
        err: nom::Err<nom::error::Error<String>>,
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
        err: nom_err.to_owned(),
    }
}

/// Ensure that the output's stdout and stderr are empty, indicating
/// the command had succeeded.
///
/// # Errors
///
/// Returns a `Error::UnexpectedTmuxOutput` in case .
pub fn check_empty_process_output(
    output: &Output,
    intent: &'static str,
) -> std::result::Result<(), Error> {
    if !output.stdout.is_empty() || !output.stderr.is_empty() {
        let stdout = String::from_utf8_lossy(&output.stdout[..]).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr[..]).to_string();
        return Err(Error::UnexpectedTmuxOutput {
            intent,
            stdout,
            stderr,
        });
    }
    Ok(())
}

/// Ensure that the tmux command succeeded (exit status 0) before parsing its output.
///
/// This prevents confusing parse errors when tmux fails and returns empty or
/// garbage stdout. Instead, we get a clear error with the actual stderr message.
///
/// # Errors
///
/// Returns `Error::UnexpectedTmuxOutput` if the command exited with non-zero status.
pub fn check_process_success(
    output: &Output,
    intent: &'static str,
) -> std::result::Result<(), Error> {
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout[..]).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr[..]).to_string();
        return Err(Error::UnexpectedTmuxOutput {
            intent,
            stdout,
            stderr,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    fn make_output(status_code: i32, stdout: &[u8], stderr: &[u8]) -> Output {
        Output {
            status: ExitStatus::from_raw(status_code << 8), // Unix exit codes are shifted
            stdout: stdout.to_vec(),
            stderr: stderr.to_vec(),
        }
    }

    #[test]
    fn check_empty_process_output_succeeds_when_empty() {
        let output = make_output(0, b"", b"");
        let result = check_empty_process_output(&output, "test-intent");
        assert!(result.is_ok());
    }

    #[test]
    fn check_empty_process_output_fails_when_stdout_not_empty() {
        let output = make_output(0, b"some output", b"");
        let result = check_empty_process_output(&output, "test-intent");
        assert!(result.is_err());

        match result.unwrap_err() {
            Error::UnexpectedTmuxOutput {
                intent,
                stdout,
                stderr,
            } => {
                assert_eq!(intent, "test-intent");
                assert_eq!(stdout, "some output");
                assert_eq!(stderr, "");
            }
            _ => panic!("Expected UnexpectedTmuxOutput error"),
        }
    }

    #[test]
    fn check_empty_process_output_fails_when_stderr_not_empty() {
        let output = make_output(0, b"", b"error message");
        let result = check_empty_process_output(&output, "test-intent");
        assert!(result.is_err());

        match result.unwrap_err() {
            Error::UnexpectedTmuxOutput {
                intent,
                stdout,
                stderr,
            } => {
                assert_eq!(intent, "test-intent");
                assert_eq!(stdout, "");
                assert_eq!(stderr, "error message");
            }
            _ => panic!("Expected UnexpectedTmuxOutput error"),
        }
    }

    #[test]
    fn check_empty_process_output_fails_when_both_not_empty() {
        let output = make_output(0, b"stdout", b"stderr");
        let result = check_empty_process_output(&output, "test-intent");
        assert!(result.is_err());

        match result.unwrap_err() {
            Error::UnexpectedTmuxOutput { stdout, stderr, .. } => {
                assert_eq!(stdout, "stdout");
                assert_eq!(stderr, "stderr");
            }
            _ => panic!("Expected UnexpectedTmuxOutput error"),
        }
    }

    #[test]
    fn check_process_success_succeeds_on_zero_exit() {
        let output = make_output(0, b"output", b"");
        let result = check_process_success(&output, "test-intent");
        assert!(result.is_ok());
    }

    #[test]
    fn check_process_success_fails_on_nonzero_exit() {
        let output = make_output(1, b"", b"command failed");
        let result = check_process_success(&output, "test-intent");
        assert!(result.is_err());

        match result.unwrap_err() {
            Error::UnexpectedTmuxOutput {
                intent,
                stdout,
                stderr,
            } => {
                assert_eq!(intent, "test-intent");
                assert_eq!(stdout, "");
                assert_eq!(stderr, "command failed");
            }
            _ => panic!("Expected UnexpectedTmuxOutput error"),
        }
    }

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
