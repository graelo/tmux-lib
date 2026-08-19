# AGENTS.md

This file contains instructions for coding agents working in this repository.

- Repository: <https://github.com/graelo/tmux-lib>
- Prefer `gh` for GitHub operations.
- Do not mention an agent or assistant in issues, pull requests, comments, or
  commit messages.
- Do not expose private local information, including machine-specific paths.

## Project

`tmux-lib` is a Rust library for reading and manipulating tmux state. Rust
1.95.0 or later is required. The crate uses edition 2024.

## Architecture

- `src/client.rs`: client state and client-level tmux operations.
- `src/server.rs`: server lifecycle and option management.
- `src/session.rs`, `src/window.rs`, and `src/pane.rs`: public tmux resource
  types, parsing, and operations.
- `src/{session,window,pane}_id.rs`: strongly typed tmux identifiers and their
  parsers.
- `src/layout.rs`: parser for tmux window-layout strings.
- `src/parse.rs`: shared parsers for tmux command output.
- `src/error.rs`: public error type and command-output validation helpers.
- `src/utils.rs`: helpers for tmux capture-buffer cleanup.
- `tests/integration.rs`: end-to-end tests against a real tmux server; tests
  clean up each session they create and skip when tmux is unavailable.

## Verification

The `Makefile` is the canonical definition of local verification tasks. **Read
it before choosing or running verification commands**; do not duplicate its
command implementations here. `make help` lists every target.

The primary targets are:

- `make check`: pre-push gate (formatting, linting, and tests).
- `make check-all`: pre-PR gate (adds dependency, commit-message, Markdown,
  and GitHub Actions security checks).
- `make fix`: formats code and applies Clippy fixes.
- `make md`: lints Markdown against `rumdl.toml`. Note the 80-column `MD013`
  reflow rule — run this after editing any Markdown file.
- `make ci-security`: runs the Poutine and Zizmor GitHub Actions scans.

The check targets mirror the GitHub workflows and use locked dependency
resolution where applicable. They assume their external tools (for example
`cargo-nextest`, `cargo-deny`, `cargo-pants`, `convco`, `poutine`, `zizmor`,
`rumdl`, and `cargo-llvm-cov`) are already installed locally.

For focused Rust tests, use `cargo nextest run <test_name>` or
`cargo nextest run <module::tests::name>`. The complete CI test sequence is
implemented in `ci/test_full.sh`; its Nextest CI profile is configured in
`.config/nextest.toml`.

## Documentation and releases

Keep user-facing documentation in sync with behavior:

- `README.md` is the canonical crate overview. Keep its installation snippet,
  supported Rust version, and usage guidance current.
- Update `CHANGELOG.md` under `Unreleased` for user-visible changes.
- For a release version bump, update `Cargo.toml`, `Cargo.lock`, and the
  versioned section and comparison links in `CHANGELOG.md`. Create a
  `vX.Y.Z` tag; the release workflow derives the release version from it.
- Commit messages must follow `.convco` Conventional Commit rules. Use
  `make commits` to check them.

`Cargo.toml`, `Cargo.lock`, `deny.toml`, and the GitHub workflows define the
release and supply-chain constraints. Preserve `--locked` behavior in Cargo
commands that resolve dependencies.
