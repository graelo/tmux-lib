//! Integration tests for tmux-lib.
//!
//! These tests require tmux to be installed and available in PATH.
//! They create real tmux sessions/windows/panes and clean them up after each test.

use std::process::Command;
use std::str::FromStr;
use std::sync::atomic::{AtomicU32, Ordering};

use tmux_lib::{
    pane, server, session,
    session::Session,
    session_id::SessionId,
    window::{self, Window},
    window_id::WindowId,
};

/// Counter for generating unique test session names.
static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Generate a unique session name for testing.
fn unique_session_name(prefix: &str) -> String {
    let count = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    format!("test-{}-{}-{}", prefix, pid, count)
}

/// Kill a tmux session by name, ignoring errors.
fn kill_session_sync(name: &str) {
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", &format!("={}", name)])
        .output();
}

/// A guard that ensures tmux sessions are cleaned up even if a test panics.
/// The session is killed when this guard is dropped.
struct SessionGuard {
    names: Vec<String>,
}

impl SessionGuard {
    fn new(name: impl Into<String>) -> Self {
        Self {
            names: vec![name.into()],
        }
    }

    fn add(&mut self, name: impl Into<String>) {
        self.names.push(name.into());
    }
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        for name in &self.names {
            kill_session_sync(name);
        }
    }
}

/// Check if tmux is available.
fn tmux_available() -> bool {
    Command::new("tmux").arg("-V").output().is_ok()
}

/// Helper to run async tests with smol.
fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    smol::block_on(future)
}

// ============================================================================
// Server Tests
// ============================================================================

mod server_tests {
    use super::*;

    #[test]
    fn test_start_and_kill_session() {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }

        let session_name = unique_session_name("server");
        let _guard = SessionGuard::new(&session_name);

        block_on(async {
            // Start a new session
            let result = server::start(&session_name).await;
            assert!(result.is_ok(), "Failed to start session: {:?}", result);

            // Verify the session exists
            let sessions = session::available_sessions().await.unwrap();
            let found = sessions.iter().any(|s| s.name == session_name);
            assert!(found, "Session '{}' should exist", session_name);

            // Kill the session
            let result = server::kill_session(&session_name).await;
            assert!(result.is_ok(), "Failed to kill session: {:?}", result);

            // Verify the session is gone
            let sessions = session::available_sessions().await.unwrap_or_default();
            let found = sessions.iter().any(|s| s.name == session_name);
            assert!(!found, "Session '{}' should be gone", session_name);
        });
    }

    #[test]
    fn test_show_options_global() {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }

        let session_name = unique_session_name("opts");
        let _guard = SessionGuard::new(&session_name);

        block_on(async {
            // Ensure server is running
            let _ = server::start(&session_name).await;

            // Get global options
            let options = server::show_options(true).await;
            assert!(options.is_ok(), "Failed to get options: {:?}", options);

            let options = options.unwrap();
            // Should have some common options
            assert!(!options.is_empty(), "Options should not be empty");
        });
    }

    #[test]
    fn test_show_option() {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }

        let session_name = unique_session_name("opt");
        let _guard = SessionGuard::new(&session_name);

        block_on(async {
            // Ensure server is running
            let _ = server::start(&session_name).await;

            // Get a specific option that should exist
            let result = server::show_option("status", true).await;
            assert!(result.is_ok(), "Failed to get option: {:?}", result);
        });
    }

    #[test]
    fn test_default_command() {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }

        let session_name = unique_session_name("defcmd");
        let _guard = SessionGuard::new(&session_name);

        block_on(async {
            // Ensure server is running
            let _ = server::start(&session_name).await;

            // Get default command
            let result = server::default_command().await;
            assert!(
                result.is_ok(),
                "Failed to get default command: {:?}",
                result
            );

            let cmd = result.unwrap();
            // Should be a non-empty string (typically a shell path)
            assert!(!cmd.is_empty(), "Default command should not be empty");
        });
    }
}

// ============================================================================
// Session Tests
// ============================================================================

mod session_tests {
    use super::*;

    #[test]
    fn test_available_sessions() {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }

        let session_name = unique_session_name("avail");
        let _guard = SessionGuard::new(&session_name);

        block_on(async {
            // Create a session
            let _ = server::start(&session_name).await;

            // Get available sessions
            let sessions = session::available_sessions().await;
            assert!(sessions.is_ok(), "Failed to get sessions: {:?}", sessions);

            let sessions = sessions.unwrap();
            let found = sessions.iter().find(|s| s.name == session_name);
            assert!(found.is_some(), "Created session should be in list");

            // Verify session has expected fields
            let sess = found.unwrap();
            assert_eq!(sess.name, session_name);
        });
    }

    #[test]
    fn test_new_session() {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }

        let session_name = unique_session_name("new");
        let new_session_name = unique_session_name("created");
        let mut guard = SessionGuard::new(&session_name);
        guard.add(&new_session_name);

        block_on(async {
            // Start initial session to ensure server is running
            let _ = server::start(&session_name).await;

            // Get windows/panes from our session to use as templates
            let windows = window::available_windows().await.unwrap();
            let our_window = windows
                .iter()
                .find(|w| w.sessions.iter().any(|s| s == &session_name));

            let panes = pane::available_panes().await.unwrap();

            if let Some(window) = our_window {
                let our_pane_ids = window.pane_ids();
                let pane = panes.iter().find(|p| our_pane_ids.contains(&p.id));

                if let Some(pane) = pane {
                    // Create a template session
                    let template_session = Session {
                        id: SessionId::from_str("$0").unwrap(),
                        name: new_session_name.clone(),
                        dirpath: pane.dirpath.clone(),
                    };

                    // Create the new session
                    let result = session::new_session(&template_session, window, pane, None).await;
                    assert!(result.is_ok(), "Failed to create session: {:?}", result);

                    let (sess_id, win_id, pane_id) = result.unwrap();
                    // Just verify they were created (IDs are opaque types)
                    let _ = sess_id;
                    assert!(win_id.as_str().starts_with('@'));
                    assert!(pane_id.as_str().starts_with('%'));

                    // Verify the session exists
                    let sessions = session::available_sessions().await.unwrap();
                    let found = sessions.iter().any(|s| s.name == new_session_name);
                    assert!(found, "New session should exist");
                }
            }
        });
    }
}

// ============================================================================
// Window Tests
// ============================================================================

mod window_tests {
    use super::*;

    #[test]
    fn test_available_windows() {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }

        let session_name = unique_session_name("win");
        let _guard = SessionGuard::new(&session_name);

        block_on(async {
            // Create a session (which creates a window)
            let _ = server::start(&session_name).await;

            // Get available windows
            let windows = window::available_windows().await;
            assert!(windows.is_ok(), "Failed to get windows: {:?}", windows);

            let windows = windows.unwrap();
            assert!(!windows.is_empty(), "Should have at least one window");

            // Check window has expected fields
            let win = &windows[0];
            assert!(win.id.as_str().starts_with('@'));
            assert!(!win.name.is_empty());
            assert!(!win.layout.is_empty());
        });
    }

    #[test]
    fn test_new_window() {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }

        let session_name = unique_session_name("newwin");
        let _guard = SessionGuard::new(&session_name);
        let window_name = "test-window";

        block_on(async {
            // Create a session
            let _ = server::start(&session_name).await;

            // Get current session, window, and pane from our session
            let sessions = session::available_sessions().await.unwrap();
            let session = sessions.iter().find(|s| s.name == session_name).unwrap();

            let windows = window::available_windows().await.unwrap();
            let our_window = windows
                .iter()
                .find(|w| w.sessions.iter().any(|s| s == &session_name));

            let panes = pane::available_panes().await.unwrap();

            if let Some(win) = our_window {
                let our_pane_ids = win.pane_ids();
                let pane = panes.iter().find(|p| our_pane_ids.contains(&p.id));

                if let Some(pane) = pane {
                    // Create a template window
                    let template_window = Window {
                        id: WindowId::from_str("@0").unwrap(),
                        index: 0,
                        is_active: false,
                        layout: String::new(),
                        name: window_name.to_string(),
                        sessions: vec![session_name.clone()],
                    };

                    // Create new window
                    let result = window::new_window(session, &template_window, pane, None).await;
                    assert!(result.is_ok(), "Failed to create window: {:?}", result);

                    let (win_id, pane_id) = result.unwrap();
                    assert!(win_id.as_str().starts_with('@'));
                    assert!(pane_id.as_str().starts_with('%'));

                    // Verify window exists
                    let windows = window::available_windows().await.unwrap();
                    let found = windows.iter().any(|w| w.name == window_name);
                    assert!(found, "New window should exist");
                }
            }
        });
    }

    #[test]
    fn test_select_window() {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }

        let session_name = unique_session_name("selwin");
        let _guard = SessionGuard::new(&session_name);

        block_on(async {
            // Create a session
            let _ = server::start(&session_name).await;

            // Get windows from our session specifically
            let windows = window::available_windows().await.unwrap();
            let our_window = windows
                .iter()
                .find(|w| w.sessions.iter().any(|s| s == &session_name));

            if let Some(win) = our_window {
                // Select the window
                let result = window::select_window(&win.id).await;
                assert!(result.is_ok(), "Failed to select window: {:?}", result);
            }
        });
    }

    #[test]
    fn test_set_layout() {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }

        let session_name = unique_session_name("layout");
        let _guard = SessionGuard::new(&session_name);

        block_on(async {
            // Create a session
            let _ = server::start(&session_name).await;

            // Get windows from our session specifically
            let windows = window::available_windows().await.unwrap();
            let our_window = windows
                .iter()
                .find(|w| w.sessions.iter().any(|s| s == &session_name));

            if let Some(win) = our_window {
                // Try setting a built-in layout
                let result = window::set_layout("even-horizontal", &win.id).await;
                // This may fail if there's only one pane, which is fine
                // Just verify it doesn't panic
                let _ = result;
            }
        });
    }
}

// ============================================================================
// Pane Tests
// ============================================================================

mod pane_tests {
    use super::*;

    #[test]
    fn test_available_panes() {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }

        let session_name = unique_session_name("pane");
        let _guard = SessionGuard::new(&session_name);

        block_on(async {
            // Create a session (which creates a pane)
            let _ = server::start(&session_name).await;

            // Get available panes
            let panes = pane::available_panes().await;
            assert!(panes.is_ok(), "Failed to get panes: {:?}", panes);

            let panes = panes.unwrap();
            assert!(!panes.is_empty(), "Should have at least one pane");

            // Check pane has expected fields
            let p = &panes[0];
            assert!(p.id.as_str().starts_with('%'));
            assert!(!p.command.is_empty());
        });
    }

    #[test]
    fn test_new_pane() {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }

        let session_name = unique_session_name("newpane");
        let _guard = SessionGuard::new(&session_name);

        block_on(async {
            // Create a session
            let _ = server::start(&session_name).await;

            // Get windows from our session specifically
            let windows = window::available_windows().await.unwrap();
            let our_window = windows
                .iter()
                .find(|w| w.sessions.iter().any(|s| s == &session_name));

            let panes = pane::available_panes().await.unwrap();

            if let Some(win) = our_window {
                // Find a pane that belongs to our window
                let our_pane_ids = win.pane_ids();
                let our_pane = panes.iter().find(|p| our_pane_ids.contains(&p.id));

                if let Some(p) = our_pane {
                    // Create new pane
                    let result = pane::new_pane(p, None, &win.id).await;
                    assert!(result.is_ok(), "Failed to create pane: {:?}", result);

                    let new_pane_id = result.unwrap();
                    assert!(new_pane_id.as_str().starts_with('%'));

                    // Verify the new pane exists in the pane list
                    let panes_after = pane::available_panes().await.unwrap();
                    let found = panes_after.iter().any(|p| p.id == new_pane_id);
                    assert!(found, "New pane {} should exist", new_pane_id);
                }
            }
        });
    }

    #[test]
    fn test_select_pane() {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }

        let session_name = unique_session_name("selpane");
        let _guard = SessionGuard::new(&session_name);

        block_on(async {
            // Create a session
            let _ = server::start(&session_name).await;

            // Get windows from our session to find its pane IDs
            let windows = window::available_windows().await.unwrap();
            let our_window = windows
                .iter()
                .find(|w| w.sessions.iter().any(|s| s == &session_name));

            if let Some(win) = our_window {
                let our_pane_ids = win.pane_ids();
                let panes = pane::available_panes().await.unwrap();

                // Find a pane that belongs to our window
                if let Some(p) = panes.iter().find(|p| our_pane_ids.contains(&p.id)) {
                    // Select the pane
                    let result = pane::select_pane(&p.id).await;
                    assert!(result.is_ok(), "Failed to select pane: {:?}", result);
                }
            }
        });
    }

    #[test]
    fn test_pane_capture() {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }

        let session_name = unique_session_name("capture");
        let _guard = SessionGuard::new(&session_name);

        block_on(async {
            // Create a session
            let _ = server::start(&session_name).await;

            // Get windows from our session to find its pane IDs
            let windows = window::available_windows().await.unwrap();
            let our_window = windows
                .iter()
                .find(|w| w.sessions.iter().any(|s| s == &session_name));

            if let Some(win) = our_window {
                let our_pane_ids = win.pane_ids();
                let panes = pane::available_panes().await.unwrap();

                // Find a pane that belongs to our window
                if let Some(p) = panes.iter().find(|p| our_pane_ids.contains(&p.id)) {
                    // Capture pane content
                    let result = p.capture().await;
                    assert!(result.is_ok(), "Failed to capture pane: {:?}", result);

                    // Result is raw bytes, just verify it doesn't error
                    let _content = result.unwrap();
                }
            }
        });
    }
}

// ============================================================================
// Window pane_ids Method Tests
// ============================================================================

mod window_pane_ids_tests {
    use super::*;

    #[test]
    fn test_window_pane_ids_integration() {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }

        let session_name = unique_session_name("paneids");
        let _guard = SessionGuard::new(&session_name);

        block_on(async {
            // Create a session
            let _ = server::start(&session_name).await;

            // Get windows from our session specifically
            let windows = window::available_windows().await.unwrap();
            let our_window = windows
                .iter()
                .find(|w| w.sessions.iter().any(|s| s == &session_name));

            if let Some(win) = our_window {
                // Get pane IDs from window layout
                let pane_ids = win.pane_ids();
                assert!(!pane_ids.is_empty(), "Window should have at least one pane");

                // Verify pane IDs match actual panes
                let panes = pane::available_panes().await.unwrap();
                for pane_id in &pane_ids {
                    let found = panes.iter().any(|p| &p.id == pane_id);
                    assert!(found, "Pane ID {:?} should exist in panes list", pane_id);
                }
            }
        });
    }
}
