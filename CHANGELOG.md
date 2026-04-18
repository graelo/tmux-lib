# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Bump MSRV 1.85 -> 1.95 and edition 2021 -> 2024
- Replace custom `SliceExt` byte-slice trim with std `trim_ascii` (Rust 1.80)
- Flatten nested `if let` with let chains (edition 2024)
- Remove stale `#[allow(unused)]` on `quoted_string`
- Harden CI workflows per security playbook
- Switch dependency updates from Dependabot to Renovate
- Adopt stricter clippy flags (`--all-targets --all-features`)
- Add `ci/test_full.sh` with MSRV validation and cargo-nextest
- Add Linux ARM to CI test matrix
- Pin poutine-action to release tag v1.1.4
- Move poutine config to `.github/poutine.yml`
- Switch Renovate schedule to daily

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
