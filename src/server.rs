//! Server management.

use std::{collections::HashMap, time::Duration};

use smol::{Timer, future, process::Command};

use crate::{
    Result,
    error::{Error, check_empty_process_output},
};

/// Maximum time to wait for the server to become ready.
const SERVER_READY_TIMEOUT: Duration = Duration::from_secs(5);

/// Delay between readiness checks.
const SERVER_READY_POLL_INTERVAL: Duration = Duration::from_millis(50);

// ------------------------------
// Ops
// ------------------------------

/// Start the Tmux server if needed, creating a session named `"[placeholder]"` in order to keep the server
/// running.
///
/// This function waits for the server to be fully ready before returning, ensuring
/// subsequent commands can be executed immediately.
///
/// It is ok-ish to already have an existing session named `"[placeholder]"`.
pub async fn start(initial_session_name: &str) -> Result<()> {
    let args = vec!["new-session", "-d", "-s", initial_session_name];

    let output = Command::new("tmux").args(&args).output().await?;
    check_empty_process_output(&output, "new-session")?;

    // Wait for the server to be fully ready to accept commands.
    wait_for_server_ready().await
}

/// Wait for the tmux server to be ready to accept commands.
///
/// This polls the server using `tmux list-sessions` until it succeeds or times out.
async fn wait_for_server_ready() -> Result<()> {
    let poll = async {
        loop {
            let output = Command::new("tmux")
                .args(["list-sessions", "-F", "#{session_name}"])
                .output()
                .await?;

            if output.status.success() {
                return Ok(());
            }

            Timer::after(SERVER_READY_POLL_INTERVAL).await;
        }
    };

    let timeout = async {
        Timer::after(SERVER_READY_TIMEOUT).await;
        Err(Error::UnexpectedTmuxOutput {
            intent: "wait-for-server-ready",
            stdout: String::new(),
            stderr: format!(
                "server did not become ready within {:?}",
                SERVER_READY_TIMEOUT
            ),
        })
    };

    future::or(poll, timeout).await
}

/// Remove the session named `"[placeholder]"` used to keep the server alive.
pub async fn kill_session(name: &str) -> Result<()> {
    let exact_name = format!("={name}");
    let args = vec!["kill-session", "-t", &exact_name];

    let output = Command::new("tmux").args(&args).output().await?;
    check_empty_process_output(&output, "kill-session")
}

/// Return the value of a Tmux option. For instance, this can be used to get Tmux's default
/// command.
pub async fn show_option(option_name: &str, global: bool) -> Result<Option<String>> {
    let mut args = vec!["show-options", "-w", "-q"];
    if global {
        args.push("-g");
    }
    args.push(option_name);

    let output = Command::new("tmux").args(&args).output().await?;
    let buffer = String::from_utf8(output.stdout)?;
    let buffer = buffer.trim_end();

    if buffer.is_empty() {
        return Ok(None);
    }
    Ok(Some(buffer.to_string()))
}

/// Return all Tmux options as a `HashMap`.
pub async fn show_options(global: bool) -> Result<HashMap<String, String>> {
    let args = if global {
        vec!["show-options", "-g"]
    } else {
        vec!["show-options"]
    };

    let output = Command::new("tmux").args(&args).output().await?;
    let buffer = String::from_utf8(output.stdout)?;

    Ok(parse_options(&buffer))
}

/// Parse the output of `tmux show-options` into a `HashMap`.
///
/// Lines without a space (bare flags) are skipped. Values that are empty or
/// equal to `''` are filtered out.
fn parse_options(buffer: &str) -> HashMap<String, String> {
    buffer
        .trim_end()
        .split('\n')
        .filter_map(|s| s.split_once(' '))
        .map(|(k, v)| (k, v.trim_start()))
        .filter(|(_, v)| !v.is_empty() && v != &"''")
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Return the `"default-command"` used to start a pane, falling back to `"default shell"` if none.
///
/// In case of bash, a `-l` flag is added.
pub async fn default_command() -> Result<String> {
    let all_options = show_options(true).await?;

    let default_shell = all_options
        .get("default-shell")
        .ok_or(Error::TmuxConfig("no default-shell"))
        .map(|cmd| cmd.to_owned())
        .map(|cmd| {
            if cmd.ends_with("bash") {
                format!("-l {cmd}")
            } else {
                cmd
            }
        })?;

    all_options
        .get("default-command")
        .or(Some(&default_shell))
        .ok_or(Error::TmuxConfig("no default-command nor default-shell"))
        .map(|cmd| cmd.to_owned())
}

#[cfg(test)]
mod tests {
    use super::parse_options;

    #[test]
    fn parse_options_typical_output() {
        let input = "default-shell /bin/zsh\nstatus on\nhistory-limit 10000\n";
        let opts = parse_options(input);

        assert_eq!(opts.get("default-shell").unwrap(), "/bin/zsh");
        assert_eq!(opts.get("status").unwrap(), "on");
        assert_eq!(opts.get("history-limit").unwrap(), "10000");
    }

    #[test]
    fn parse_options_skips_bare_flags() {
        let input = "destroy-unattached\ndefault-shell /bin/zsh\nsilence-action\n";
        let opts = parse_options(input);

        assert_eq!(opts.len(), 1);
        assert_eq!(opts.get("default-shell").unwrap(), "/bin/zsh");
        assert!(!opts.contains_key("destroy-unattached"));
        assert!(!opts.contains_key("silence-action"));
    }

    #[test]
    fn parse_options_filters_empty_values() {
        let input = "default-command ''\ndefault-shell /bin/zsh\n";
        let opts = parse_options(input);

        assert!(!opts.contains_key("default-command"));
        assert_eq!(opts.get("default-shell").unwrap(), "/bin/zsh");
    }

    #[test]
    fn parse_options_empty_input() {
        let opts = parse_options("");
        assert!(opts.is_empty());
    }

    #[test]
    fn parse_options_value_with_spaces() {
        let input = "status-left [#S] #H\nstatus on\n";
        let opts = parse_options(input);

        assert_eq!(opts.get("status-left").unwrap(), "[#S] #H");
        assert_eq!(opts.get("status").unwrap(), "on");
    }

    #[test]
    fn parse_options_trims_spaces_between_key_and_value() {
        let input = "key   value-with-extra-spaces\n";
        let opts = parse_options(input);

        assert_eq!(opts.get("key").unwrap(), "value-with-extra-spaces");
    }
}
