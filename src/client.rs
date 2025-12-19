//! Client-level functions: for representing client state (`client_session` etc) or reporting information inside Tmux.

use std::str::FromStr;

use nom::{character::complete::char, combinator::all_consuming, Parser};
use serde::{Deserialize, Serialize};
use smol::process::Command;

use crate::{
    error::{map_add_intent, Error},
    parse::{quoted_nonempty_string, quoted_string},
    Result,
};

/// A Tmux client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Client {
    /// The current session.
    pub session_name: String,
    /// The last session.
    pub last_session_name: String,
}

impl FromStr for Client {
    type Err = Error;

    /// Parse a string containing client information into a new `Client`.
    ///
    /// This returns a `Result<Client, Error>` as this call can obviously
    /// fail if provided an invalid format.
    ///
    /// The expected format of the tmux response is
    ///
    /// ```text
    /// name-of-current-session:name-of-last-session
    /// ```
    ///
    /// This status line is obtained with
    ///
    /// ```text
    /// tmux display-message -p -F "'#{client_session}':'#{client_last_session}'"
    /// ```
    ///
    /// For definitions, look at `Pane` type and the tmux man page for
    /// definitions.
    fn from_str(input: &str) -> std::result::Result<Self, Self::Err> {
        let desc = "Client";
        let intent = "'##{client_session}':'##{client_last_session}'";
        let parser = (quoted_nonempty_string, char(':'), quoted_string);

        let (_, (session_name, _, last_session_name)) = all_consuming(parser)
            .parse(input)
            .map_err(|e| map_add_intent(desc, intent, e))?;

        Ok(Client {
            session_name: session_name.to_string(),
            last_session_name: last_session_name.to_string(),
        })
    }
}

// ------------------------------
// Ops
// ------------------------------

/// Return the current client useful attributes.
///
/// # Errors
///
/// Returns an `io::IOError` in the command failed.
pub async fn current() -> Result<Client> {
    let args = vec![
        "display-message",
        "-p",
        "-F",
        "'#{client_session}':'#{client_last_session}'",
    ];

    let output = Command::new("tmux").args(&args).output().await?;
    let buffer = String::from_utf8(output.stdout)?;

    Client::from_str(buffer.trim_end())
}

/// Return a list of all `Pane` from all sessions.
///
/// # Panics
///
/// This function panics if it can't communicate with Tmux.
pub fn display_message(message: &str) {
    let args = vec!["display-message", message];

    std::process::Command::new("tmux")
        .args(&args)
        .output()
        .expect("Cannot communicate with Tmux for displaying message");
}

/// Switch to session exactly named `session_name`.
pub async fn switch_client(session_name: &str) -> Result<()> {
    let exact_session_name = format!("={session_name}");
    let args = vec!["switch-client", "-t", &exact_session_name];

    Command::new("tmux")
        .args(&args)
        .output()
        .await
        .expect("Cannot communicate with Tmux for switching the client");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Client;
    use std::str::FromStr;

    #[test]
    fn parse_client_with_both_sessions() {
        let input = "'current-session':'last-session'";
        let client = Client::from_str(input).expect("Should parse valid client");

        assert_eq!(client.session_name, "current-session");
        assert_eq!(client.last_session_name, "last-session");
    }

    #[test]
    fn parse_client_with_empty_last_session() {
        // When there's no previous session, last_session is empty
        let input = "'my-session':''";
        let client = Client::from_str(input).expect("Should parse client with empty last session");

        assert_eq!(client.session_name, "my-session");
        assert_eq!(client.last_session_name, "");
    }

    #[test]
    fn parse_client_with_special_chars_in_name() {
        let input = "'server: $123':'dev-env'";
        let client = Client::from_str(input).expect("Should parse client with special chars");

        assert_eq!(client.session_name, "server: $123");
        assert_eq!(client.last_session_name, "dev-env");
    }

    #[test]
    fn parse_client_fails_on_empty_current_session() {
        // Current session should not be empty
        let input = "'':'last-session'";
        let result = Client::from_str(input);

        assert!(result.is_err());
    }

    #[test]
    fn parse_client_fails_on_missing_quotes() {
        let input = "current-session:last-session";
        let result = Client::from_str(input);

        assert!(result.is_err());
    }

    #[test]
    fn parse_client_fails_on_missing_colon() {
        let input = "'current-session''last-session'";
        let result = Client::from_str(input);

        assert!(result.is_err());
    }

    #[test]
    fn parse_client_fails_on_extra_content() {
        let input = "'current':'last':extra";
        let result = Client::from_str(input);

        assert!(result.is_err());
    }
}
