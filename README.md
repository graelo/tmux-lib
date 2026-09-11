# `tmux-lib`

[![crate](https://img.shields.io/crates/v/tmux-lib.svg)](https://crates.io/crates/tmux-lib)
[![documentation](https://docs.rs/tmux-lib/badge.svg)](https://docs.rs/tmux-lib)
[![minimum rustc 1.95](https://img.shields.io/badge/rustc-1.95+-red.svg)](https://rust-lang.github.io/rfcs/2495-min-rust-version.html)
[![build status](https://github.com/graelo/tmux-lib/actions/workflows/ci-essentials.yml/badge.svg)](https://github.com/graelo/tmux-lib/actions)

Read or manipulate tmux.

Version requirements: _rustc 1.95.0+_ and _tmux 3.2+_

One operation asks for more: `Tmux::display_message_to` needs _tmux 3.3+_,
because 3.2 declares `display-message -c` as taking no argument and answers
any use of it with its usage string.

```toml
[dependencies]
tmux-lib = "0.7"
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

## Choosing a transport

`Tmux::spawning()` forks a `tmux` client per command. It costs nothing to
construct, cannot fail, attaches no client, and needs no session to exist.

`Tmux::control()` keeps one `tmux -C` client attached and sends every command
down it, so a command costs a round trip on an open pipe rather than a fork,
an exec and a connect. It pays for itself over many commands, and it needs a
session to attach to:

```rust
use tmux_lib::Tmux;

// There is no constructor that falls back on your behalf: which transport you
// ended up with changes what the handle costs, so write it where it shows.
let tmux = Tmux::control().unwrap_or_else(|_| Tmux::spawning());
```

A control handle is not a different API. Every operation exists on both, and
the ones a control connection cannot carry fork a client themselves: captures,
anything creating a pane, window or session, anything about the calling
client, and killing a session. Two consequences are worth knowing:

- an attached control client is visible. It bumps `#{session_attached}` and
  fires the `client-attached` and `client-detached` hooks. Call
  `Tmux::disconnect` to release it;
- while any control client is attached — this crate's, or another tool's —
  tmux resolves "the current client" to it for a caller that is not itself
  inside a tmux client. `current_client` and `current_client_name` report no
  client in that case rather than naming it.

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
