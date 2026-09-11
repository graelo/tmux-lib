//! What a transport hands back, whichever transport ran the command.

use std::process::Output;

use crate::{Result, error::Error};

/// One tmux command's result, in the terms both transports can state.
///
/// The spawning transport has a process to inspect — an exit status, a stdout
/// and a stderr. The control transport has none of those: it has a block of
/// output and a terminator that is either `%end` or `%error`, with the failure
/// message arriving as the block body. What the two genuinely share is this:
/// what the command printed, what it complained about, and whether tmux
/// considered it to have failed.
///
/// Keeping the three separate rather than collapsing them matters, because the
/// operations check two different things. Reads want "did it fail", and
/// tolerate a warning on stderr from a noisy config. Commands that should be
/// silent want "did it say anything at all", where a warning *is* the signal.
pub(crate) struct Reply {
    /// What the command printed.
    body: Vec<u8>,
    /// What the command complained about: stderr, or the `%error` body.
    diagnostic: Vec<u8>,
    /// Whether tmux reported the command as having failed.
    failed: bool,
}

impl Reply {
    /// A command tmux reported as succeeding.
    ///
    /// Only the tests build one directly today; the control transport, which
    /// has a block body and a terminator rather than a process to convert,
    /// is the caller this exists for.
    #[allow(dead_code)]
    pub(crate) fn success(body: Vec<u8>) -> Reply {
        Reply {
            body,
            diagnostic: Vec::new(),
            failed: false,
        }
    }

    /// A command tmux reported as failing, carrying its message.
    ///
    /// See [`Reply::success`] on why this has no caller outside the tests yet.
    #[allow(dead_code)]
    pub(crate) fn failure(diagnostic: Vec<u8>) -> Reply {
        Reply {
            body: Vec::new(),
            diagnostic,
            failed: true,
        }
    }

    /// Whether tmux ran the command without reporting a failure.
    ///
    /// This ignores anything the command printed; use it only where the
    /// question really is "did this work", as when polling a server that may
    /// not be up yet.
    pub(crate) fn succeeded(&self) -> bool {
        !self.failed
    }

    /// The command's output, or an error describing how it failed.
    ///
    /// Checking this before decoding is what keeps a failed command from
    /// surfacing as a confusing parse error over whatever it left on stdout.
    pub(crate) fn output(self, intent: &'static str) -> Result<Vec<u8>> {
        if self.failed {
            return Err(self.into_error(intent));
        }

        Ok(self.body)
    }

    /// Succeed only if the command printed nothing at all.
    ///
    /// tmux answers a command such as `select-pane` with silence, so any
    /// output is a report of something having gone wrong — including on the
    /// paths where it still exits zero, which is why emptiness is checked
    /// rather than the failure flag alone.
    pub(crate) fn no_output(self, intent: &'static str) -> Result<()> {
        if self.failed || !self.body.is_empty() || !self.diagnostic.is_empty() {
            return Err(self.into_error(intent));
        }

        Ok(())
    }

    fn into_error(self, intent: &'static str) -> Error {
        Error::UnexpectedTmuxOutput {
            intent,
            stdout: String::from_utf8_lossy(&self.body).into_owned(),
            stderr: String::from_utf8_lossy(&self.diagnostic).into_owned(),
        }
    }
}

impl From<Output> for Reply {
    fn from(output: Output) -> Reply {
        Reply {
            failed: !output.status.success(),
            body: output.stdout,
            diagnostic: output.stderr,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    fn from_process(code: i32, stdout: &[u8], stderr: &[u8]) -> Reply {
        Output {
            // Unix exit codes are shifted.
            status: ExitStatus::from_raw(code << 8),
            stdout: stdout.to_vec(),
            stderr: stderr.to_vec(),
        }
        .into()
    }

    #[test]
    fn no_output_accepts_silence() {
        assert!(from_process(0, b"", b"").no_output("test-intent").is_ok());
    }

    #[test]
    fn no_output_rejects_anything_printed() {
        let error = from_process(0, b"some output", b"")
            .no_output("test-intent")
            .unwrap_err();

        let Error::UnexpectedTmuxOutput {
            intent,
            stdout,
            stderr,
        } = error
        else {
            panic!("expected UnexpectedTmuxOutput, got {error:?}");
        };
        assert_eq!(intent, "test-intent");
        assert_eq!(stdout, "some output");
        assert_eq!(stderr, "");
    }

    #[test]
    fn no_output_rejects_anything_complained_about() {
        let error = from_process(0, b"", b"error message")
            .no_output("test-intent")
            .unwrap_err();

        let Error::UnexpectedTmuxOutput { stdout, stderr, .. } = error else {
            panic!("expected UnexpectedTmuxOutput, got {error:?}");
        };
        assert_eq!(stdout, "");
        assert_eq!(stderr, "error message");
    }

    #[test]
    fn no_output_reports_both_streams() {
        let error = from_process(0, b"stdout", b"stderr")
            .no_output("test-intent")
            .unwrap_err();

        let Error::UnexpectedTmuxOutput { stdout, stderr, .. } = error else {
            panic!("expected UnexpectedTmuxOutput, got {error:?}");
        };
        assert_eq!(stdout, "stdout");
        assert_eq!(stderr, "stderr");
    }

    #[test]
    fn no_output_rejects_a_silent_failure() {
        // A command that exits non-zero without a word is still a failure.
        // The check this replaced looked only at the two streams and would
        // have let it through.
        assert!(from_process(1, b"", b"").no_output("test-intent").is_err());
    }

    #[test]
    fn output_hands_back_what_the_command_printed() {
        let body = from_process(0, b"output", b"")
            .output("test-intent")
            .unwrap();

        assert_eq!(body, b"output");
    }

    #[test]
    fn output_tolerates_a_warning_from_a_command_that_worked() {
        // A noisy config writes to stderr while the command itself succeeds.
        // Reads must still get their bytes.
        let body = from_process(0, b"output", b"config warning")
            .output("test-intent")
            .unwrap();

        assert_eq!(body, b"output");
    }

    #[test]
    fn output_reports_a_failure_instead_of_returning_bytes() {
        let error = from_process(1, b"", b"command failed")
            .output("test-intent")
            .unwrap_err();

        let Error::UnexpectedTmuxOutput {
            intent,
            stdout,
            stderr,
        } = error
        else {
            panic!("expected UnexpectedTmuxOutput, got {error:?}");
        };
        assert_eq!(intent, "test-intent");
        assert_eq!(stdout, "");
        assert_eq!(stderr, "command failed");
    }

    #[test]
    fn a_control_style_failure_carries_its_message_as_the_diagnostic() {
        // The control transport has no stderr: an `%error` block's body is the
        // message. It must surface the same way a stderr line does.
        let error = Reply::failure(b"unknown command: nosuchcommand".to_vec())
            .output("test-intent")
            .unwrap_err();

        let Error::UnexpectedTmuxOutput { stdout, stderr, .. } = error else {
            panic!("expected UnexpectedTmuxOutput, got {error:?}");
        };
        assert_eq!(stdout, "");
        assert_eq!(stderr, "unknown command: nosuchcommand");
    }

    #[test]
    fn succeeded_ignores_what_the_command_printed() {
        assert!(from_process(0, b"noise", b"warning").succeeded());
        assert!(!from_process(1, b"", b"").succeeded());
        assert!(!Reply::failure(b"nope".to_vec()).succeeded());
        assert!(Reply::success(b"fine".to_vec()).succeeded());
    }
}
