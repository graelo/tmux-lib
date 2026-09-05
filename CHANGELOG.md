# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- `show_option` queries the same scope as `show_options` — the global table
  with `global`, the session table without it. It passed `-w` unconditionally,
  so it asked for window options while `show_options` asked for session ones

### Changed

- No `make` target passes `--locked`. This crate is a library, its
  `Cargo.lock` is gitignored, and the versions that matter are the ones a
  consumer's own lock file picks

## [0.6.0] - 2026-09-05

### Changed

- **Breaking:** every tmux operation is now a synchronous method on a `Tmux`
  handle (`Tmux::spawning()`, or `Tmux::spawning_on(Server::socket_name(..))`
  for a non-default server) instead of an `async` free function. The handle is
  `Send + Sync + Clone`; async callers bridge with their runtime's blocking
  helper. The `smol` dependency is gone, taking the transitive runtime
  dependencies from 42 crates to 12
- **Breaking:** remove legacy quote-delimited `FromStr` input for panes,
  sessions, windows, and clients; framed records are decoded by the crate
  itself and are no longer part of the public API. `Tmux::current_client` and
  `Tmux::client_for_target` replace hand-built format strings fed to
  `Client::from_str`
- **Breaking:** `display_message` and `switch_client` return `Result` instead
  of panicking on failure, and the `display_message` re-export at the crate
  root is now `Tmux::display_message`
- **Breaking:** `Error` is `#[non_exhaustive]`, and `Error::ParseError`
  carries a plain message rather than a `nom` error, which the record decoders
  no longer produce
- `Pane::capture` becomes `Tmux::capture_pane`, and `Pane`, `Window`,
  `Session` and `Client` no longer perform I/O
- Make `README.md` the canonical crate overview and remove its
  `cargo-sync-readme` markers, and document the API there
- Reduce crate-level Rust documentation to a link to the project README

### Fixed

- Pass `-u` to every tmux invocation. Without it, a tmux client started in an
  environment with no locale is treated as non-UTF-8, and tmux replaces every
  non-ASCII byte in its output with `_` — silently mangling pane titles and
  paths under cron, launchd, or a bare systemd unit
- `show_option` returned `"status off"` where it meant `"off"`; it asked tmux
  to print the option name alongside the value
- Use byte-length-prefixed tmux records when reading panes, sessions, windows,
  and client session names, preserving arbitrary UTF-8 values and newlines
- Normalize visually escaped tmux 3.4–3.5 command output before parsing framed
  records, preserving arbitrary UTF-8 values and backslashes across tmux
  versions

### Added

- `Server`, selecting the default tmux server, a socket name (`tmux -L`) or a
  socket path (`tmux -S`). Integration tests now run each case on their own
  private server instead of sharing the developer's
- `src/wire/`, the single module that knows the framed record protocol. Format
  and intent strings are derived from a declared field list per record type
  rather than hand-escaped in four places
- `src/transport/`, the single site that starts a tmux process, so every
  invocation shares one argument prefix
- `tests/architecture.rs` enforces both of the above by walking `src/`
- A tmux version axis (3.2, 3.4, 3.5, 3.6, 3.7c, built from source) in the
  compatibility matrix, with `ci/tmux_wire_probe.sh` checking that the buffer
  round-trip preserves bytes on each
- A `Makefile` defines canonical local verification tasks, with `make check`
  as the pre-push gate and `make check-all` as the pre-PR gate
- `rumdl.toml` applies consistent Markdown linting and formatting rules
- `AGENTS.md` documents the project architecture, verification, and release
  conventions for coding agents

## [0.5.0] - 2026-04-18

### Changed

- Bump MSRV 1.85 -> 1.95 and edition 2021 -> 2024
- Replace custom `SliceExt` byte-slice trim with std `trim_ascii` (Rust 1.80)
- Flatten nested `if let` with let chains (edition 2024)
- Extract `parse_options` for testability and remove dead code in
  `default_command`
- Harden CI workflows per security playbook
- Switch dependency updates from Dependabot to Renovate with automerge
- Adopt cargo-nextest with `ci/test_full.sh` and MSRV validation
- Add Linux ARM to CI test matrix

### Fixed

- Fix panic in `show_options` when tmux returns bare-flag options without
  values

### Added

- Doc tests on all major public types (`PaneId`, `SessionId`, `WindowId`,
  `Pane`, `Session`, `Window`, `parse_window_layout`, `cleanup_captured_buffer`)
- Unit tests for `parse_options` and edge cases for `cleanup_captured_buffer`
- `CHANGELOG.md` covering all releases
- `#[must_use]` on `SessionId::as_str` and `WindowId::as_str`

## [0.4.2] - 2025-12-19

### Fixed

- More robust window creation (target by session ID instead of name)

## [0.4.1] - 2025-12-19

### Added

- Integration tests using real tmux sessions
- Improved test coverage across all modules

### Changed

- Improved error reporting with context in parse and process errors
- Drop Windows CI runner (tmux is Unix-only)

### Fixed

- Wait for server startup before issuing commands

## [0.4.0] - 2025-11-23

### Changed

- Upgrade to Nom 8
- Switch async runtime from tokio to smol
- Drop `Cargo.lock` (this is a library)

### Fixed

- Allow pane titles to be empty strings

## [0.3.1] - 2024-08-16

### Changed

- Bump MSRV to 1.74
- Update dependencies

## [0.3.0] - 2023-08-29

### Fixed

- Drop pseudo-empty options (`==''`) from `show_options`

## [0.2.2] - 2022-11-11

### Changed

- Update dependencies
- Restructure CI workflows

## [0.2.1] - 2022-11-08

Initial tagged release.
