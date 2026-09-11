//! Integration tests for tmux-lib.
//!
//! Each test runs against its own private tmux server, addressed by a unique
//! socket name. Tests therefore neither observe each other nor touch the
//! developer's own tmux server, which is what lets them assert exact counts
//! and names rather than merely "not empty".

use std::process::{Command, Output};
use std::str::FromStr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use tmux_lib::{
    Server, Tmux, session::Session, session_id::SessionId, window::Window, window_id::WindowId,
};

/// Counter for generating unique names.
static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

fn unique_name(prefix: &str) -> String {
    let count = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("test-{prefix}-{}-{count}", std::process::id())
}

/// Check whether tmux is installed at all.
fn tmux_available() -> bool {
    Command::new("tmux").arg("-V").output().is_ok()
}

/// The installed tmux version as `(major, minor)`, for tests that pin a
/// behaviour tmux only grew at some point.
///
/// `tmux -V` prints `tmux 3.3a` or `tmux next-3.6`; the trailing letter is a
/// patch level and the `next-` prefix a pre-release, so neither participates
/// in the comparison.
fn tmux_version() -> Option<(u32, u32)> {
    let output = Command::new("tmux").arg("-V").output().ok()?;
    let stdout = String::from_utf8(output.stdout).ok()?;
    let version = stdout.split_whitespace().nth(1)?.rsplit('-').next()?;

    let mut parts = version.split('.');
    let number = |part: Option<&str>| -> Option<u32> {
        let digits: String = part?.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    };

    Some((number(parts.next())?, number(parts.next())?))
}

/// A private tmux server, killed when the test ends.
struct TestServer {
    socket: String,
    socket_path: String,
    tmux: Tmux,
    session: String,
}

impl TestServer {
    /// Start a private server holding one session, and return a handle to it.
    fn start(prefix: &str) -> Self {
        let socket = unique_name(&format!("sock-{prefix}"));
        let tmux = Tmux::spawning_on(Server::socket_name(&socket));
        let session = unique_name(prefix);

        let mut server = Self {
            socket,
            socket_path: String::new(),
            tmux,
            session,
        };
        server
            .tmux
            .start_server(&server.session)
            .expect("failed to start the private tmux server");

        // Record the socket path while the server is alive: a test may kill it
        // before the guard runs, and `kill-server` leaves the file behind.
        let output = server.raw(&["display-message", "-p", "#{socket_path}"]);
        server.socket_path = String::from_utf8(output.stdout)
            .expect("socket path should be UTF-8")
            .trim_end()
            .to_owned();
        server
    }

    fn tmux(&self) -> &Tmux {
        &self.tmux
    }

    /// The name of the session created with the server.
    fn session_name(&self) -> &str {
        &self.session
    }

    /// Run a raw tmux command against this server, for fixture setup the
    /// library deliberately does not model.
    fn raw(&self, args: &[&str]) -> Output {
        let mut argv = vec!["-L", self.socket.as_str()];
        argv.extend_from_slice(args);
        Command::new("tmux")
            .args(argv)
            .output()
            .expect("failed to run tmux")
    }

    /// A control-transport handle onto this same private server.
    fn control(&self) -> Tmux {
        Tmux::control_on(Server::socket_name(&self.socket))
            .expect("failed to attach a control client")
    }

    /// The only window of the initial session.
    fn window(&self) -> Window {
        let windows = self
            .tmux
            .available_windows()
            .expect("failed to list windows");
        windows
            .into_iter()
            .find(|w| w.sessions.iter().any(|s| s == &self.session))
            .expect("the initial session should have a window")
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.raw(&["kill-server"]);

        if !self.socket_path.is_empty() {
            let _ = std::fs::remove_file(&self.socket_path);
        }
    }
}

/// Skip the body when tmux is not installed.
macro_rules! require_tmux {
    () => {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }
    };
}

/// Skip the body when the installed tmux is older than `major.minor`.
macro_rules! require_tmux_version {
    ($major:expr, $minor:expr) => {
        match tmux_version() {
            Some(version) if version >= ($major, $minor) => {}
            Some((major, minor)) => {
                eprintln!(
                    "Skipping test: needs tmux {}.{}, found {major}.{minor}",
                    $major, $minor
                );
                return;
            }
            None => {
                eprintln!("Skipping test: cannot read the tmux version");
                return;
            }
        }
    };
}

// ============================================================================
// Server
// ============================================================================

mod server_tests {
    use super::*;

    #[test]
    fn start_creates_a_session_and_kill_removes_it() {
        require_tmux!();
        let server = TestServer::start("server");
        let tmux = server.tmux();

        let sessions = tmux.available_sessions().unwrap();
        assert_eq!(sessions.len(), 1, "a private server holds only our session");
        assert_eq!(sessions[0].name, server.session_name());

        tmux.kill_session(server.session_name()).unwrap();

        // The last session going away takes the server with it, so listing
        // sessions now fails rather than returning an empty list.
        assert!(tmux.available_sessions().is_err());
    }

    #[test]
    fn show_options_returns_the_global_options() {
        require_tmux!();
        let server = TestServer::start("opts");

        let options = server.tmux().show_options(true).unwrap();

        assert!(!options.is_empty());
        assert!(options.contains_key("status"));
    }

    #[test]
    fn show_option_returns_one_named_option() {
        require_tmux!();
        let server = TestServer::start("opt");
        let tmux = server.tmux();

        server.raw(&["set-option", "-g", "status", "off"]);

        assert_eq!(
            tmux.show_option("status", true).unwrap().as_deref(),
            Some("off")
        );
    }

    #[test]
    fn show_option_reaches_the_window_option_table() {
        require_tmux!();
        let server = TestServer::start("optwin");
        let tmux = server.tmux();

        // `automatic-rename` lives in the window table, not the session one.
        // The query passes no `-w`, so this asserts tmux resolves the name
        // against the table that declares it.
        server.raw(&["set-option", "-g", "-w", "automatic-rename", "off"]);

        assert_eq!(
            tmux.show_option("automatic-rename", true)
                .unwrap()
                .as_deref(),
            Some("off")
        );
    }

    #[test]
    fn show_option_returns_none_for_an_unset_option() {
        require_tmux!();
        let server = TestServer::start("optnone");

        let value = server.tmux().show_option("@no-such-option", true).unwrap();

        assert_eq!(value, None);
    }

    #[test]
    fn default_command_falls_back_to_the_default_shell() {
        require_tmux!();
        let server = TestServer::start("defcmd");

        let command = server.tmux().default_command().unwrap();

        assert!(!command.is_empty());
    }
}

// ============================================================================
// Sessions
// ============================================================================

mod client_tests {
    use super::*;

    #[test]
    fn most_recent_client_name_is_none_without_an_attached_client() {
        require_tmux!();
        let server = TestServer::start("clients");

        // The private server holds a detached session and nothing else. The
        // value is not the point — this asserts tmux accepts the framed
        // `list-clients` format and that an empty reply decodes as no client
        // rather than as an error.
        let name = server.tmux().most_recent_client_name().unwrap();

        assert_eq!(name, None);
    }

    #[test]
    fn current_client_is_none_outside_a_client() {
        require_tmux!();
        let server = TestServer::start("curclient");

        // The test process is not a tmux client of this private server, so
        // tmux has no client to resolve the format against.
        let client = server.tmux().current_client().unwrap();

        assert!(client.is_none());
    }

    #[test]
    fn current_client_name_fails_outside_a_client() {
        require_tmux!();
        let server = TestServer::start("noclient");

        assert!(server.tmux().current_client_name().is_err());
    }

    #[test]
    fn display_message_to_is_best_effort() {
        require_tmux!();
        // tmux 3.2 declares `-c` without an argument, so every call is a usage
        // error there — see `Tmux::display_message_to`.
        require_tmux_version!(3, 3);
        let server = TestServer::start("msgtarget");

        // tmux reports no error for a client name that does not exist — it
        // shows the message on whatever client it can find instead. Callers
        // reporting to a client they picked earlier cannot rely on an error
        // to tell them the client went away, so this pins the behaviour they
        // do get.
        server
            .tmux()
            .display_message_to("/dev/null-no-such-client", "hello")
            .expect("an unknown client target is not an error");
    }
}

mod session_tests {
    use super::*;

    #[test]
    fn available_sessions_reports_name_and_id() {
        require_tmux!();
        let server = TestServer::start("avail");

        let sessions = server.tmux().available_sessions().unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, server.session_name());
        assert!(sessions[0].id.as_str().starts_with('$'));
    }

    #[test]
    fn new_session_creates_a_second_session() {
        require_tmux!();
        let server = TestServer::start("new");
        let tmux = server.tmux();

        let window = server.window();
        let panes = tmux.available_panes().unwrap();
        let pane = panes
            .iter()
            .find(|p| window.pane_ids().contains(&p.id))
            .expect("the initial window should have a pane");

        let created_name = unique_name("created");
        let template = Session {
            id: SessionId::from_str("$0").unwrap(),
            name: created_name.clone(),
            dirpath: pane.dirpath.clone(),
        };

        let (_, window_id, pane_id) = tmux.new_session(&template, &window, pane, None).unwrap();

        assert!(window_id.as_str().starts_with('@'));
        assert!(pane_id.as_str().starts_with('%'));

        let sessions = tmux.available_sessions().unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(sessions.iter().any(|s| s.name == created_name));
    }
}

// ============================================================================
// Windows
// ============================================================================

mod window_tests {
    use super::*;

    #[test]
    fn available_windows_reports_one_window_for_a_fresh_session() {
        require_tmux!();
        let server = TestServer::start("win");

        let windows = server.tmux().available_windows().unwrap();

        assert_eq!(windows.len(), 1);
        assert!(windows[0].id.as_str().starts_with('@'));
        assert!(!windows[0].name.is_empty());
        assert!(!windows[0].layout.is_empty());
        assert_eq!(windows[0].sessions, vec![server.session_name().to_owned()]);
    }

    #[test]
    fn new_window_adds_a_named_window_to_the_session() {
        require_tmux!();
        let server = TestServer::start("newwin");
        let tmux = server.tmux();

        let sessions = tmux.available_sessions().unwrap();
        let session = sessions
            .iter()
            .find(|s| s.name == server.session_name())
            .expect("our session");
        let window = server.window();
        let panes = tmux.available_panes().unwrap();
        let pane = panes
            .iter()
            .find(|p| window.pane_ids().contains(&p.id))
            .expect("the initial window should have a pane");

        let template = Window {
            id: WindowId::from_str("@0").unwrap(),
            index: 0,
            is_active: false,
            layout: String::new(),
            name: "test-window".to_owned(),
            sessions: vec![server.session_name().to_owned()],
        };

        let (window_id, pane_id) = tmux.new_window(session, &template, pane, None).unwrap();

        assert!(window_id.as_str().starts_with('@'));
        assert!(pane_id.as_str().starts_with('%'));

        let windows = tmux.available_windows().unwrap();
        assert_eq!(windows.len(), 2);
        assert!(windows.iter().any(|w| w.name == "test-window"));
    }

    #[test]
    fn select_window_makes_it_active() {
        require_tmux!();
        let server = TestServer::start("selwin");
        let tmux = server.tmux();

        server.raw(&["new-window", "-d", "-t", server.session_name()]);
        let windows = tmux.available_windows().unwrap();
        let target = windows
            .iter()
            .find(|w| !w.is_active)
            .expect("the second window should be inactive");

        tmux.select_window(&target.id).unwrap();

        let windows = tmux.available_windows().unwrap();
        let now_active = windows
            .iter()
            .find(|w| w.is_active)
            .expect("an active window");
        assert_eq!(now_active.id, target.id);
    }

    #[test]
    fn set_layout_changes_the_window_layout() {
        require_tmux!();
        let server = TestServer::start("layout");
        let tmux = server.tmux();

        // A layout is only meaningful with more than one pane.
        server.raw(&["split-window", "-v", "-t", server.session_name()]);
        let before = server.window().layout;

        tmux.set_layout("even-horizontal", &server.window().id)
            .unwrap();

        let after = server.window().layout;
        assert_ne!(before, after, "the layout should have been rewritten");
    }
}

// ============================================================================
// Panes
// ============================================================================

mod pane_tests {
    use super::*;

    #[test]
    fn available_panes_reports_id_and_command() {
        require_tmux!();
        let server = TestServer::start("pane");

        let panes = server.tmux().available_panes().unwrap();

        assert_eq!(panes.len(), 1);
        assert!(panes[0].id.as_str().starts_with('%'));
        assert!(!panes[0].command.is_empty());
    }

    #[test]
    fn available_panes_preserves_a_unicode_title() {
        require_tmux!();
        let server = TestServer::start("pane-title");
        let target = format!("={}:0.0", server.session_name());

        assert!(
            server
                .raw(&["set-option", "-p", "-t", &target, "automatic-rename", "off"])
                .status
                .success()
        );

        let title = "π - Chef d'orchestre";
        assert!(
            server
                .raw(&["select-pane", "-t", &target, "-T", title])
                .status
                .success()
        );

        let panes = server.tmux().available_panes().unwrap();

        assert_eq!(panes.len(), 1);
        assert_eq!(panes[0].title, title);
    }

    #[test]
    fn new_pane_splits_the_window() {
        require_tmux!();
        let server = TestServer::start("newpane");
        let tmux = server.tmux();

        let window = server.window();
        let panes = tmux.available_panes().unwrap();
        let pane = panes
            .iter()
            .find(|p| window.pane_ids().contains(&p.id))
            .expect("the initial window should have a pane");

        let new_pane_id = tmux.new_pane(pane, None, &window.id).unwrap();

        assert!(new_pane_id.as_str().starts_with('%'));

        let panes = tmux.available_panes().unwrap();
        assert_eq!(panes.len(), 2);
        assert!(panes.iter().any(|p| p.id == new_pane_id));
    }

    #[test]
    fn select_pane_makes_it_active() {
        require_tmux!();
        let server = TestServer::start("selpane");
        let tmux = server.tmux();

        server.raw(&["split-window", "-v", "-t", server.session_name()]);
        let panes = tmux.available_panes().unwrap();
        let target = panes
            .iter()
            .find(|p| !p.is_active)
            .expect("the second pane should be inactive");

        tmux.select_pane(&target.id).unwrap();

        let panes = tmux.available_panes().unwrap();
        let now_active = panes.iter().find(|p| p.is_active).expect("an active pane");
        assert_eq!(now_active.id, target.id);
    }

    #[test]
    fn capture_pane_returns_the_pane_contents() {
        require_tmux!();
        let server = TestServer::start("capture");
        let tmux = server.tmux();

        let pane_id = tmux.available_panes().unwrap()[0].id.clone();
        let marker = "tmux-lib-capture-marker";
        server.raw(&["send-keys", "-t", pane_id.as_str(), marker]);

        // The pane redraws asynchronously, so poll rather than sleep once.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let captured = tmux.capture_pane(&pane_id).unwrap();
            if String::from_utf8_lossy(&captured).contains(marker) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "marker never appeared in the capture"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

// ============================================================================
// Window::pane_ids against the live server
// ============================================================================

mod window_pane_ids_tests {
    use super::*;

    #[test]
    fn pane_ids_match_the_panes_tmux_reports() {
        require_tmux!();
        let server = TestServer::start("paneids");
        let tmux = server.tmux();

        server.raw(&["split-window", "-v", "-t", server.session_name()]);

        let pane_ids = server.window().pane_ids();
        assert_eq!(pane_ids.len(), 2);

        let panes = tmux.available_panes().unwrap();
        for pane_id in &pane_ids {
            assert!(
                panes.iter().any(|p| &p.id == pane_id),
                "pane {pane_id:?} should be listed"
            );
        }
    }
}

// ============================================================================
// Control transport
// ============================================================================

mod control_tests {
    use super::*;

    #[test]
    fn a_control_handle_answers_what_a_spawning_handle_answers() {
        require_tmux!();
        let server = TestServer::start("ctl");

        let spawned = server.tmux().available_sessions().unwrap();
        let controlled = server.control().available_sessions().unwrap();

        assert_eq!(spawned.len(), 1);
        assert_eq!(
            spawned.iter().map(|s| &s.name).collect::<Vec<_>>(),
            controlled.iter().map(|s| &s.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_control_client_is_a_client_not_a_session() {
        require_tmux!();
        let server = TestServer::start("ctlvis");

        let control = server.control();

        // Attaching must not show up as something to back up, which is what
        // would have made a tool-owned placeholder session the wrong answer.
        assert_eq!(control.available_sessions().unwrap().len(), 1);
        assert_eq!(server.tmux().available_sessions().unwrap().len(), 1);
    }

    #[test]
    fn a_failing_command_is_reported_rather_than_parsed() {
        require_tmux!();
        let server = TestServer::start("ctlerr");

        let error = server
            .control()
            .command(&["nosuchcommand"])
            .expect_err("tmux should reject an unknown command");

        assert!(
            format!("{error}").contains("nosuchcommand"),
            "the error should carry what tmux said, got: {error}"
        );
    }

    #[test]
    fn an_argument_the_command_lexer_would_mangle_arrives_intact() {
        require_tmux!();
        let server = TestServer::start("ctlquote");
        let control = server.control();

        let value = r#"it's "two words" \ and ; a #{format}"#;
        control
            .command(&["set-option", "-g", "@probe", value])
            .unwrap();

        // Read back over the spawning transport, where the argument never met
        // the control-mode lexer, so a difference can only have come from the
        // write.
        assert_eq!(
            server
                .tmux()
                .show_option("@probe", true)
                .unwrap()
                .as_deref(),
            Some(value)
        );
    }

    #[test]
    fn an_argument_holding_a_newline_is_refused() {
        require_tmux!();
        let server = TestServer::start("ctlnl");

        let error = server
            .control()
            .command(&["set-option", "-g", "@probe", "line1\nline2"])
            .expect_err("a newline cannot be carried by the control transport");

        assert!(
            matches!(error, tmux_lib::error::Error::UnsupportedArgument { .. }),
            "expected UnsupportedArgument, got: {error}"
        );

        // Refused before anything was written, so the connection still works.
        assert!(server.control().available_sessions().is_ok());
    }

    #[test]
    fn attaching_fails_when_there_is_no_session() {
        require_tmux!();

        // A socket no server is listening on: a control client is a client,
        // so there is nothing for it to attach to. Addressed by path rather
        // than by name, because the attempt leaves the socket file behind and
        // this way the test knows where to find it.
        let path = std::env::temp_dir().join(unique_name("sock-absent"));
        let nowhere = Server::socket_path(path.to_str().unwrap());

        let error = Tmux::control_on(nowhere).expect_err("there is no session to attach to");
        let _ = std::fs::remove_file(&path);

        let tmux_lib::error::Error::ControlAttachFailed { message } = &error else {
            panic!("expected ControlAttachFailed, got: {error}");
        };
        assert!(
            message.contains("no sessions"),
            "the error should carry tmux's own reason, got: {message}"
        );
    }

    #[test]
    fn the_connection_comes_back_after_it_is_cut() {
        require_tmux!();
        let server = TestServer::start("ctlcut");

        // A second session, so that killing the attached one leaves the server
        // running and something to re-attach to.
        let survivor = unique_name("survivor");
        server.raw(&["new-session", "-d", "-s", &survivor]);

        let control = server.control();
        let attached = String::from_utf8(
            control
                .command(&["display-message", "-p", "-F", "#{client_session}"])
                .unwrap(),
        )
        .unwrap()
        .trim_end()
        .to_owned();

        // Kill it from the outside, the way a user or a restore would.
        server.raw(&["kill-session", "-t", &format!("={attached}")]);

        // The in-flight connection is gone, so the next command may report the
        // disconnection; the one after it must work.
        let _ = control.available_sessions();
        let sessions = control
            .available_sessions()
            .expect("the handle should attach again after the connection ends");

        assert!(sessions.iter().all(|s| s.name != attached));
        assert!(!sessions.is_empty());
    }

    #[test]
    fn disconnecting_releases_the_client_and_the_next_command_attaches_again() {
        require_tmux!();
        let server = TestServer::start("ctldrop");
        let control = server.control();

        assert!(control.available_sessions().is_ok());
        control.disconnect();

        assert!(
            control.available_sessions().is_ok(),
            "a command after disconnect should attach again"
        );
    }
}

// ============================================================================
// Operations a control handle must not carry over its connection
// ============================================================================

mod control_routing_tests {
    use super::*;

    #[test]
    fn the_attached_control_client_is_never_offered_as_a_target() {
        require_tmux!();
        let server = TestServer::start("ctlpick");

        let control = server.control();

        // The control client this handle attached is a client, it is listed
        // like any other, and it is the most recently active one on the
        // server. It has no status line, so there is no client to report to.
        assert_eq!(control.most_recent_client_name().unwrap(), None);
        assert_eq!(server.tmux().most_recent_client_name().unwrap(), None);
    }

    #[test]
    fn killing_the_attached_session_succeeds_rather_than_cutting_the_reply() {
        require_tmux!();
        let server = TestServer::start("ctlkill");
        let survivor = unique_name("survivor");
        server.raw(&["new-session", "-d", "-s", &survivor]);

        let control = server.control();
        let attached = String::from_utf8(
            control
                .command(&["display-message", "-p", "-F", "#{client_session}"])
                .unwrap(),
        )
        .unwrap()
        .trim_end()
        .to_owned();

        // Over the connection this would end the connection by succeeding,
        // and the caller would be told the command failed.
        control
            .kill_session(&attached)
            .expect("killing the attached session should succeed");

        // `attach` with no target picks the most recently used session, so
        // which of the two it landed on is not the test's business; that it is
        // gone and the other is not, is.
        let names: Vec<_> = server
            .tmux()
            .available_sessions()
            .unwrap()
            .into_iter()
            .map(|session| session.name)
            .collect();
        assert_eq!(names.len(), 1, "one of the two sessions should remain");
        assert!(!names.contains(&attached));
        assert!(names.contains(&survivor) || names.contains(&server.session_name().to_owned()));
    }

    #[test]
    fn a_control_handle_creates_panes_windows_and_sessions() {
        require_tmux!();
        let server = TestServer::start("ctlnew");
        let control = server.control();

        let window = server.window();
        let pane = control
            .available_panes()
            .unwrap()
            .into_iter()
            .next()
            .expect("the initial session has a pane");

        let new_pane = control.new_pane(&pane, None, &window.id).unwrap();
        assert!(
            control
                .available_panes()
                .unwrap()
                .iter()
                .any(|p| p.id == new_pane)
        );

        let session = control
            .available_sessions()
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let (new_window, _) = control.new_window(&session, &window, &pane, None).unwrap();
        assert!(
            control
                .available_windows()
                .unwrap()
                .iter()
                .any(|w| w.id == new_window)
        );
    }

    #[test]
    fn a_control_handle_captures_a_pane() {
        require_tmux!();
        let server = TestServer::start("ctlcap");
        let control = server.control();

        let pane = control
            .available_panes()
            .unwrap()
            .into_iter()
            .next()
            .expect("the initial session has a pane");

        let by_control = control.capture_pane(&pane.id).unwrap();
        let by_spawning = server.tmux().capture_pane(&pane.id).unwrap();

        assert_eq!(by_control, by_spawning);
    }

    #[test]
    fn a_control_handle_reports_no_current_client_outside_one() {
        require_tmux!();
        let server = TestServer::start("ctlcur");

        // Asked over the connection, this would describe the connection's own
        // client and report a session. The test runner is not inside a tmux
        // client of this server, so the answer is that there is none.
        assert!(server.control().current_client().unwrap().is_none());
        assert!(server.tmux().current_client().unwrap().is_none());
    }
}
