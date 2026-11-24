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

- `src/tmux.rs`: the public `Tmux` handle and server socket address.
- `src/ops/`: command arguments, transport selection, and reply handling.
- `src/transport/`: run a command by spawning a client or reusing a control
  connection. Both return a `Reply`.
- `src/transport/control/`: `mod.rs` owns the child, reader thread, and
  reconnect; `protocol.rs` reads reply blocks; `quoting.rs` writes command
  lines. Notifications are discarded, not decoded.
- `src/wire/`: build list formats and decode their records. User data uses
  a byte length and raw bytes; newline separates records, not data fields.
- `src/{client,session,window,pane}.rs`: resource types and field decoding.
- `src/{session,window,pane}_id.rs`: typed tmux identifiers and their parsers.
- `src/layout.rs`: window layout parsing, separate from reply framing.
- `tests/integration.rs`: use private tmux servers and exercise both
  transports. Tests skip when tmux is unavailable.

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
