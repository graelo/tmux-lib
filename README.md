# `tmux-lib`

[![crate](https://img.shields.io/crates/v/tmux-lib.svg)](https://crates.io/crates/tmux-lib)
[![documentation](https://docs.rs/tmux-lib/badge.svg)](https://docs.rs/tmux-lib)
[![minimum rustc 1.95](https://img.shields.io/badge/rustc-1.95+-red.svg)](https://rust-lang.github.io/rfcs/2495-min-rust-version.html)
[![build status](https://github.com/graelo/tmux-lib/actions/workflows/ci-essentials.yml/badge.svg)](https://github.com/graelo/tmux-lib/actions)

Read or manipulate tmux.

Version requirements: _rustc 1.95.0+_ and _tmux 3.2+_

```toml
[dependencies]
tmux-lib = "0.6"
```

## Getting started

Every operation hangs off a `Tmux` handle, which owns how the crate reaches
tmux:

```rust
use tmux_lib::{Result, Tmux};

fn main() -> Result<()> {
    // The default server. `Tmux::spawning_on(Server::socket_name("work"))`
    // talks to `tmux -L work` instead.
    let tmux = Tmux::spawning();

    for session in tmux.available_sessions()? {
        println!("{} in {}", session.name, session.dirpath.display());
    }

    for pane in tmux.available_panes()? {
        let contents = tmux.capture_pane(&pane.id)?;
        println!("{} running {}: {} bytes", pane.id, pane.command,
                 contents.len());
    }

    Ok(())
}
```

The handle is `Send + Sync + Clone`, so it can be shared as-is or wrapped in
an `Arc`. Its methods block; from an async caller, bridge with your runtime's
blocking helper, such as `smol::unblock(move || tmux.available_panes())`.

## Caveats

- This is a beta version

## Development

For local verification, read the [`Makefile`](Makefile) for the canonical task
definitions, or run `make help` to list them: run `make check` before pushing
and `make check-all` before opening a pull request.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <https://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
